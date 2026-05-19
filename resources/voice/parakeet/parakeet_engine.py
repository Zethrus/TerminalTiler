#!/usr/bin/env python3
"""TerminalTiler local NVIDIA Parakeet helper.

Line protocol on stdin/stdout:
  start <sample_rate_hz>
  audio-pcm16-hex <little-endian signed PCM16 bytes as hex>
  audio-final-pcm16-hex <final buffered PCM16 bytes without partial inference>
  stop
  warm
  health
  shutdown

The helper intentionally keeps all heavyweight ASR dependencies out of the Rust
process. A voice pack/venv supplies NVIDIA NeMo and PyTorch; NeMo downloads or
uses cached NVIDIA ASR checkpoints. The default profile keeps a CTC-style
streaming model resident for low-latency English command dictation and falls
back to the existing Parakeet TDT v2 offline path if streaming initialization
fails.
"""

from __future__ import annotations

import os
import sys
import tempfile
import time
import wave
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

DEFAULT_OFFLINE_MODEL = "nvidia/parakeet-tdt-0.6b-v2"
DEFAULT_STREAMING_MODEL = "nvidia/parakeet-ctc-0.6b"
PROTOCOL_STDOUT = sys.stdout
# NeMo, PyTorch, Hugging Face, and Xet may print progress/log lines to stdout
# while loading or downloading the model. Keep stdout reserved for the framed
# TerminalTiler protocol and send third-party chatter to stderr instead.
sys.stdout = sys.stderr


def emit(kind: str, payload: str = "") -> None:
    safe = payload.replace("\n", "\\n")
    print(f"{kind} {safe}" if safe else kind, file=PROTOCOL_STDOUT, flush=True)


def decode_payload(payload: str) -> str:
    return payload.replace("\\n", "\n")


@dataclass
class CaptureBuffer:
    sample_rate_hz: int = 16_000
    pcm: bytearray = field(default_factory=bytearray)


class ParakeetEngine:
    def __init__(self) -> None:
        self.offline_model_name = os.environ.get(
            "TERMINALTILER_PARAKEET_MODEL", DEFAULT_OFFLINE_MODEL
        )
        self.streaming_model_name = os.environ.get(
            "TERMINALTILER_PARAKEET_STREAMING_MODEL", DEFAULT_STREAMING_MODEL
        )
        self.engine_mode = os.environ.get("TERMINALTILER_VOICE_ENGINE_MODE", "auto").lower()
        self.profile = os.environ.get("TERMINALTILER_VOICE_PROFILE", "streaming").lower()
        self.partial_min_ms = int(
            os.environ.get("TERMINALTILER_VOICE_PARTIAL_MIN_MS", "225")
        )
        model_cache_env = os.environ.get("TERMINALTILER_VOICE_MODEL_PATH")
        self.model_cache: Optional[Path] = (
            Path(model_cache_env).expanduser() if model_cache_env else None
        )
        if self.model_cache is not None:
            self.model_cache.mkdir(parents=True, exist_ok=True)
            os.environ.setdefault("HF_HOME", str(self.model_cache))
            os.environ.setdefault("NEMO_CACHE_DIR", str(self.model_cache / "nemo"))
            os.environ.setdefault("TORCH_HOME", str(self.model_cache / "torch"))
        self._streaming_model = None
        self._offline_model = None
        self._torch = None
        self._quantized = False
        self._streaming_error: Optional[str] = None
        self.capture = CaptureBuffer()
        self.latest_partial = ""
        self._last_partial_at = 0.0

    def health(self) -> None:
        try:
            import torch  # type: ignore
            import nemo  # type: ignore
            import nemo.collections.asr as nemo_asr  # type: ignore  # noqa: F401
        except Exception as exc:  # pragma: no cover - depends on user pack
            emit("health", f"error: {exc}")
            return

        cuda = bool(torch.cuda.is_available())
        if self.engine_mode == "cuda" and not cuda:
            emit("health", "error: CUDA requested but torch.cuda.is_available() is false")
            return

        device = self._device_name(torch, cuda)
        cuda_device = "none"
        if cuda:
            try:
                cuda_device = torch.cuda.get_device_name(0)
            except Exception:
                cuda_device = "available"

        emit(
            "health",
            "ok: "
            f"NeMo available, dependencies ready, streaming={self.streaming_available()}, "
            f"warm={self.warm()}, device={device}, cuda_device={cuda_device}, "
            f"model={self.active_model_name()}, cache={self.model_cache}, "
            f"python={sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}, "
            f"torch={getattr(torch, '__version__', 'unknown')}, "
            f"nemo={getattr(nemo, '__version__', 'unknown')}",
        )

    def warm_up(self) -> None:
        try:
            import torch  # type: ignore
            import nemo  # type: ignore
        except Exception as exc:  # pragma: no cover - depends on user pack
            emit("health", f"error: {exc}")
            return

        cuda = bool(torch.cuda.is_available())
        if self.engine_mode == "cuda" and not cuda:
            emit("health", "error: CUDA requested but torch.cuda.is_available() is false")
            return

        device = self._device_name(torch, cuda)
        cuda_device = "none"
        if cuda:
            try:
                cuda_device = torch.cuda.get_device_name(0)
            except Exception:
                cuda_device = "available"
        try:
            model = self._load_preferred_model()
        except Exception as exc:  # pragma: no cover - depends on user pack/model cache
            emit("health", f"error: model load failed: {exc}")
            return

        emit(
            "health",
            "ok: "
            f"NeMo available, model loaded, streaming={self.streaming_available()}, "
            f"device={device}, cuda_device={cuda_device}, "
            f"quantized={self._quantized}, model={model}, cache={self.model_cache}, "
            f"python={sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}, "
            f"torch={getattr(torch, '__version__', 'unknown')}, "
            f"nemo={getattr(nemo, '__version__', 'unknown')}",
        )

    def capabilities(self) -> None:
        device = "unknown"
        if self._torch is not None:
            try:
                device = self._device_name(self._torch, bool(self._torch.cuda.is_available()))
            except Exception:
                device = "unavailable"
        model = (
            self.streaming_model_name
            if self.streaming_available()
            else self.offline_model_name
        )
        emit(
            "capabilities",
            f"streaming={str(self.streaming_available()).lower()}, "
            f"model={model}, device={device}, warm={str(self.warm()).lower()}",
        )

    def start(self, sample_rate_hz: int) -> None:
        self.capture = CaptureBuffer(sample_rate_hz=sample_rate_hz, pcm=bytearray())
        self.latest_partial = ""
        self._last_partial_at = 0.0
        emit("ready", f"sample_rate_hz={sample_rate_hz}")

    def append_pcm16_hex(self, payload: str, emit_partial: bool = True) -> None:
        try:
            chunk = bytes.fromhex(payload.strip())
            self.capture.pcm.extend(chunk)
            if emit_partial and chunk:
                self._emit_streaming_partial()
        except ValueError as exc:
            emit("error", f"invalid pcm16 hex payload: {exc}")

    def stop(self) -> None:
        if not self.capture.pcm:
            emit("final", "")
            return
        started = time.perf_counter()
        try:
            text = self._final_transcript(bytes(self.capture.pcm), self.capture.sample_rate_hz)
        except Exception as exc:  # pragma: no cover - depends on user pack/GPU
            emit("error", f"Parakeet transcription failed: {exc}")
            return
        elapsed_ms = int((time.perf_counter() - started) * 1000)
        emit("partial", f"NVIDIA Parakeet final after release: {elapsed_ms}ms")
        emit("final", text)

    def shutdown(self) -> None:
        emit("ready", "shutdown")
        raise SystemExit(0)

    def streaming_available(self) -> bool:
        return self._streaming_model is not None and self._streaming_error is None

    def warm(self) -> bool:
        return self._streaming_model is not None or self._offline_model is not None

    def active_model_name(self) -> str:
        if self.streaming_available():
            return self.streaming_model_name
        if self._offline_model is not None:
            return self.offline_model_name
        if self._streaming_error is not None:
            return self.offline_model_name
        return self.streaming_model_name if self.profile != "offline" else self.offline_model_name

    def _load_preferred_model(self) -> str:
        if self.profile != "offline":
            try:
                self._load_streaming_model()
                return self.streaming_model_name
            except Exception as exc:
                self._streaming_error = str(exc)
                emit("partial", f"Streaming ASR unavailable; falling back to offline TDT: {exc}")
        self._load_offline_model()
        return self.offline_model_name

    def _load_streaming_model(self):
        if self._streaming_model is not None:
            return self._streaming_model
        self._streaming_model = self._load_model_by_name(self.streaming_model_name)
        self._streaming_error = None
        return self._streaming_model

    def _load_offline_model(self):
        if self._offline_model is not None:
            return self._offline_model
        self._offline_model = self._load_model_by_name(self.offline_model_name)
        return self._offline_model

    def _load_model_by_name(self, model_name: str):
        import torch  # type: ignore
        import nemo.collections.asr as nemo_asr  # type: ignore

        model = nemo_asr.models.ASRModel.from_pretrained(model_name=model_name)
        cuda = bool(torch.cuda.is_available())
        device = self._device_name(torch, cuda)
        model = model.to(device)
        if device == "cpu" and os.environ.get("TERMINALTILER_VOICE_CPU_QUANTIZE", "1") != "0":
            try:
                quantization = getattr(torch, "ao", torch).quantization
                quantized = quantization.quantize_dynamic(
                    model,
                    {torch.nn.Linear},
                    dtype=torch.qint8,
                    inplace=True,
                )
                if quantized is not None:
                    model = quantized
                self._quantized = True
            except Exception as exc:  # pragma: no cover - model dependent
                emit("partial", f"CPU quantization unavailable; using fp32: {exc}")
        model.eval()
        self._torch = torch
        return model

    def _device_name(self, torch, cuda_available: bool) -> str:
        if self.engine_mode == "cpu":
            return "cpu"
        if self.engine_mode == "cuda":
            if not cuda_available:
                raise RuntimeError("CUDA requested but torch.cuda.is_available() is false")
            return "cuda"
        return "cuda" if cuda_available else "cpu"

    def _captured_seconds(self) -> float:
        if self.capture.sample_rate_hz <= 0:
            return 0.0
        return len(self.capture.pcm) / 2.0 / float(self.capture.sample_rate_hz)

    def _emit_streaming_partial(self) -> None:
        now = time.perf_counter()
        if self._last_partial_at and (now - self._last_partial_at) * 1000 < self.partial_min_ms:
            return
        self._last_partial_at = now
        if self.profile == "offline":
            emit("partial", f"Captured {self._captured_seconds():.1f}s of voice audio…")
            return
        try:
            model = self._load_streaming_model()
            text = self._transcribe_pcm_array(
                model, bytes(self.capture.pcm), self.capture.sample_rate_hz
            )
        except Exception as exc:
            if self._streaming_error is None:
                self._streaming_error = str(exc)
                emit("partial", f"Streaming ASR unavailable; using offline TDT on release: {exc}")
            return
        partial = stable_partial(self.latest_partial, text)
        if partial:
            self.latest_partial = partial
            emit("partial", partial)

    def _final_transcript(self, pcm: bytes, sample_rate_hz: int) -> str:
        if self.profile != "offline":
            if self.latest_partial.strip():
                return self.latest_partial.strip()
            if self._streaming_error is None:
                return ""
            emit(
                "partial",
                f"Streaming ASR unavailable; using offline TDT: {self._streaming_error}",
            )
        emit("partial", "Transcribing with NVIDIA Parakeet TDT offline fallback…")
        return self._transcribe_pcm_wav(pcm, sample_rate_hz)

    def _transcribe_pcm_array(self, model, pcm: bytes, sample_rate_hz: int) -> str:
        import numpy as np  # type: ignore

        if not pcm:
            return ""
        audio = np.frombuffer(pcm, dtype="<i2").astype("float32") / 32768.0
        # NeMo transcription signatures vary by release/model family. Prefer
        # in-memory audio so the streaming path avoids temp WAV creation.
        call_variants = (
            lambda: model.transcribe(audio, batch_size=1, timestamps=False, verbose=False),
            lambda: model.transcribe([audio], batch_size=1, timestamps=False, verbose=False),
            lambda: model.transcribe(audio, timestamps=False),
            lambda: model.transcribe([audio], timestamps=False),
        )
        last_error: Optional[Exception] = None
        for call in call_variants:
            try:
                return transcript_text(call())
            except TypeError as exc:
                last_error = exc
                continue
        if last_error is not None:
            raise last_error
        return ""

    def _transcribe_pcm_wav(self, pcm: bytes, sample_rate_hz: int) -> str:
        model = self._load_offline_model()
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as temp:
            wav_path = Path(temp.name)
        try:
            with wave.open(str(wav_path), "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(sample_rate_hz)
                wav.writeframes(pcm)
            return transcript_text(model.transcribe([str(wav_path)], timestamps=False))
        finally:
            try:
                wav_path.unlink()
            except FileNotFoundError:
                pass


def transcript_text(output) -> str:
    first = output[0] if isinstance(output, (list, tuple)) and output else output
    if first is None:
        return ""
    return getattr(first, "text", str(first)).strip()


def normalize_transcript(text: str) -> str:
    return " ".join(text.strip().split())


def stable_partial(previous: str, candidate: str) -> str:
    """Return a readable cumulative partial only when it adds useful signal."""

    previous = normalize_transcript(previous)
    candidate = normalize_transcript(candidate)
    if not candidate or candidate == previous:
        return ""
    # ASR may revise the tail of a partial. Keep the candidate as the cumulative
    # transcript but only after normalization so the HUD never shows repeated
    # duplicated prefixes such as "cargo cargo test".
    doubled = f"{previous} {previous}"
    if candidate.startswith(doubled):
        candidate = candidate[len(previous) :].strip()
    if not previous or candidate.startswith(previous):
        return candidate
    return candidate


def main() -> int:
    engine = ParakeetEngine()
    emit("ready", "parakeet-helper")
    for raw_line in sys.stdin:
        line = raw_line.rstrip("\r\n")
        if not line:
            continue
        kind, _, payload = line.partition(" ")
        payload = decode_payload(payload)
        if kind == "health":
            engine.health()
        elif kind == "warm":
            engine.warm_up()
        elif kind == "start":
            try:
                sample_rate = int(payload.strip() or "16000")
            except ValueError:
                emit("error", f"invalid start sample rate: {payload}")
                continue
            engine.start(sample_rate)
        elif kind == "audio-pcm16-hex":
            engine.append_pcm16_hex(payload)
        elif kind == "audio-final-pcm16-hex":
            engine.append_pcm16_hex(payload, emit_partial=False)
        elif kind == "stop":
            engine.stop()
        elif kind == "capabilities":
            engine.capabilities()
        elif kind == "shutdown":
            engine.shutdown()
        else:
            emit("error", f"unknown command: {kind}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
