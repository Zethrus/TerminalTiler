#!/usr/bin/env python3
"""TerminalTiler local NVIDIA Parakeet helper.

Line protocol on stdin/stdout:
  start <sample_rate_hz>
  audio-pcm16-hex <little-endian signed PCM16 bytes as hex>
  stop
  health
  shutdown

The helper intentionally keeps all heavyweight ASR dependencies out of the Rust
process. A voice pack/venv supplies NVIDIA NeMo and PyTorch; NeMo downloads or
uses the cached nvidia/parakeet-tdt-0.6b-v2 checkpoint.
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

DEFAULT_MODEL = "nvidia/parakeet-tdt-0.6b-v2"
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
        self.model_name = os.environ.get("TERMINALTILER_PARAKEET_MODEL", DEFAULT_MODEL)
        self.engine_mode = os.environ.get("TERMINALTILER_VOICE_ENGINE_MODE", "auto").lower()
        model_cache_env = os.environ.get("TERMINALTILER_VOICE_MODEL_PATH")
        self.model_cache: Optional[Path] = (
            Path(model_cache_env).expanduser() if model_cache_env else None
        )
        if self.model_cache is not None:
            self.model_cache.mkdir(parents=True, exist_ok=True)
            os.environ.setdefault("HF_HOME", str(self.model_cache))
            os.environ.setdefault("NEMO_CACHE_DIR", str(self.model_cache / "nemo"))
            os.environ.setdefault("TORCH_HOME", str(self.model_cache / "torch"))
        self._model = None
        self._torch = None
        self._quantized = False
        self.capture = CaptureBuffer()

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
        try:
            self._load_model()
        except Exception as exc:  # pragma: no cover - depends on user pack/model cache
            emit("health", f"error: model load failed: {exc}")
            return

        emit(
            "health",
            "ok: "
            f"NeMo available, model loaded, device={device}, cuda_device={cuda_device}, "
            f"quantized={self._quantized}, model={self.model_name}, cache={self.model_cache}, "
            f"python={sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}, "
            f"torch={getattr(torch, '__version__', 'unknown')}, "
            f"nemo={getattr(nemo, '__version__', 'unknown')}",
        )

    def start(self, sample_rate_hz: int) -> None:
        self.capture = CaptureBuffer(sample_rate_hz=sample_rate_hz, pcm=bytearray())
        emit("ready", f"sample_rate_hz={sample_rate_hz}")

    def append_pcm16_hex(self, payload: str) -> None:
        try:
            chunk = bytes.fromhex(payload.strip())
            self.capture.pcm.extend(chunk)
            if chunk:
                emit(
                    "partial",
                    f"Captured {self._captured_seconds():.1f}s of voice audio…",
                )
        except ValueError as exc:
            emit("error", f"invalid pcm16 hex payload: {exc}")

    def stop(self) -> None:
        if not self.capture.pcm:
            emit("final", "")
            return
        emit("partial", "Transcribing with NVIDIA Parakeet…")
        started = time.perf_counter()
        try:
            text = self._transcribe_pcm(bytes(self.capture.pcm), self.capture.sample_rate_hz)
        except Exception as exc:  # pragma: no cover - depends on user pack/GPU
            emit("error", f"Parakeet transcription failed: {exc}")
            return
        elapsed_ms = int((time.perf_counter() - started) * 1000)
        emit("partial", f"NVIDIA Parakeet finalized in {elapsed_ms}ms")
        emit("final", text)

    def shutdown(self) -> None:
        emit("ready", "shutdown")
        raise SystemExit(0)

    def _load_model(self):
        if self._model is not None:
            return self._model
        import torch  # type: ignore
        import nemo.collections.asr as nemo_asr  # type: ignore

        model = nemo_asr.models.ASRModel.from_pretrained(model_name=self.model_name)
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
        self._model = model
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

    def _transcribe_pcm(self, pcm: bytes, sample_rate_hz: int) -> str:
        model = self._load_model()
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as temp:
            wav_path = Path(temp.name)
        try:
            with wave.open(str(wav_path), "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(sample_rate_hz)
                wav.writeframes(pcm)
            output = model.transcribe([str(wav_path)], timestamps=False)
            first = output[0] if output else ""
            return getattr(first, "text", str(first)).strip()
        finally:
            try:
                wav_path.unlink()
            except FileNotFoundError:
                pass


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
        elif kind == "start":
            try:
                sample_rate = int(payload.strip() or "16000")
            except ValueError:
                emit("error", f"invalid start sample rate: {payload}")
                continue
            engine.start(sample_rate)
        elif kind == "audio-pcm16-hex":
            engine.append_pcm16_hex(payload)
        elif kind == "stop":
            engine.stop()
        elif kind == "shutdown":
            engine.shutdown()
        else:
            emit("error", f"unknown command: {kind}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
