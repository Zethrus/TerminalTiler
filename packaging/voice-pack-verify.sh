#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE_PATH="$ROOT_DIR/resources/voice/parakeet/parakeet_engine.py"
MODEL_NAME="nvidia/parakeet-tdt-0.6b-v2"
MODEL_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/terminaltiler/parakeet-verify"
ENGINE_MODE="auto"
AUDIO_PATH=""
EXPECT_TEXT=""
MAX_FINAL_MS=""
ALLOW_UNHEALTHY=0
TIMEOUT_BIN="${TIMEOUT_BIN:-timeout}"
TIMEOUT_DURATION="${TIMEOUT_DURATION:-300s}"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [options]

Verifies a TerminalTiler NVIDIA Parakeet helper/voice pack from the command line.
By default it requires a successful helper health check. With --audio it also
sends a 16 kHz mono PCM16 WAV through the helper and requires a final transcript.

Options:
  --engine PATH        Helper path (default: resources/voice/parakeet/parakeet_engine.py)
  --model NAME         NeMo model name (default: nvidia/parakeet-tdt-0.6b-v2)
  --cache PATH         Model/cache directory (default: ~/.cache/terminaltiler/parakeet-verify)
  --mode MODE          auto, cuda, or cpu (default: auto)
  --audio PATH         Optional 16 kHz mono PCM16 WAV to transcribe
  --expect-text TEXT   Require final transcript to contain TEXT (requires --audio)
  --max-final-ms MS    Require helper's reported finalization time to be <= MS (requires --audio)
  --timeout DURATION   Command timeout (default: 300s)
  --allow-unhealthy    Print diagnostics but do not fail on health/transcription failure
  -h, --help           Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --engine) ENGINE_PATH="$2"; shift 2 ;;
    --model) MODEL_NAME="$2"; shift 2 ;;
    --cache) MODEL_CACHE="$2"; shift 2 ;;
    --mode) ENGINE_MODE="$2"; shift 2 ;;
    --audio) AUDIO_PATH="$2"; shift 2 ;;
    --expect-text) EXPECT_TEXT="$2"; shift 2 ;;
    --max-final-ms) MAX_FINAL_MS="$2"; shift 2 ;;
    --timeout) TIMEOUT_DURATION="$2"; shift 2 ;;
    --allow-unhealthy) ALLOW_UNHEALTHY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ ! -f "$ENGINE_PATH" ]]; then
  echo "voice helper not found: $ENGINE_PATH" >&2
  exit 1
fi

if [[ "$ENGINE_MODE" != "auto" && "$ENGINE_MODE" != "cuda" && "$ENGINE_MODE" != "cpu" ]]; then
  echo "--mode must be auto, cuda, or cpu" >&2
  exit 2
fi

if [[ -n "$EXPECT_TEXT" && -z "$AUDIO_PATH" ]]; then
  echo "--expect-text requires --audio" >&2
  exit 2
fi

if [[ -n "$MAX_FINAL_MS" && -z "$AUDIO_PATH" ]]; then
  echo "--max-final-ms requires --audio" >&2
  exit 2
fi

if [[ -n "$MAX_FINAL_MS" && ! "$MAX_FINAL_MS" =~ ^[0-9]+$ ]]; then
  echo "--max-final-ms must be an integer number of milliseconds" >&2
  exit 2
fi

if ! command -v "$TIMEOUT_BIN" >/dev/null 2>&1; then
  echo "missing timeout command: $TIMEOUT_BIN" >&2
  exit 1
fi

PACK_ROOT="$(cd "$(dirname "$ENGINE_PATH")" && pwd)"
PYTHON_BIN="python3"
if [[ -x "$PACK_ROOT/.venv/bin/python" ]]; then
  PYTHON_BIN="$PACK_ROOT/.venv/bin/python"
elif [[ -x "$PACK_ROOT/.venv/Scripts/python.exe" ]]; then
  PYTHON_BIN="$PACK_ROOT/.venv/Scripts/python.exe"
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN="python"
else
  echo "python is required to run the Parakeet helper" >&2
  exit 1
fi

mkdir -p "$MODEL_CACHE"

generate_requests() {
  printf 'health\n'
  if [[ -n "$AUDIO_PATH" ]]; then
    "$PYTHON_BIN" - "$AUDIO_PATH" <<'PY'
import sys
import wave
from pathlib import Path

path = Path(sys.argv[1])
with wave.open(str(path), "rb") as wav:
    channels = wav.getnchannels()
    sample_width = wav.getsampwidth()
    sample_rate = wav.getframerate()
    if channels != 1 or sample_width != 2 or sample_rate != 16000:
        raise SystemExit(
            f"{path} must be 16 kHz mono PCM16 WAV; got "
            f"channels={channels}, sample_width={sample_width}, sample_rate={sample_rate}"
        )
    payload = wav.readframes(wav.getnframes()).hex()
print("start 16000")
if payload:
    print(f"audio-pcm16-hex {payload}")
print("stop")
PY
  fi
  printf 'shutdown\n'
}

export TERMINALTILER_PARAKEET_MODEL="$MODEL_NAME"
export TERMINALTILER_VOICE_MODEL_PATH="$MODEL_CACHE"
export TERMINALTILER_VOICE_ENGINE_MODE="$ENGINE_MODE"

echo "==> helper: $ENGINE_PATH"
echo "==> python: $PYTHON_BIN"
echo "==> model:  $MODEL_NAME"
echo "==> cache:  $MODEL_CACHE"
echo "==> mode:   $ENGINE_MODE"
if [[ -n "$AUDIO_PATH" ]]; then
  echo "==> audio:  $AUDIO_PATH"
fi

output_file="$(mktemp)"
trap 'rm -f "$output_file"' EXIT

set +e
generate_requests | "$TIMEOUT_BIN" "$TIMEOUT_DURATION" "$PYTHON_BIN" "$ENGINE_PATH" | tee "$output_file"
status=${PIPESTATUS[1]}
set -e

if [[ $status -ne 0 ]]; then
  echo "voice helper failed with status $status" >&2
  [[ $ALLOW_UNHEALTHY == 1 ]] || exit "$status"
fi

if ! grep -q '^health ok:' "$output_file"; then
  echo "voice helper health check did not report ok" >&2
  [[ $ALLOW_UNHEALTHY == 1 ]] || exit 1
fi

if [[ -n "$AUDIO_PATH" ]]; then
  if ! grep -q '^final ' "$output_file"; then
    echo "voice helper did not emit a final transcript" >&2
    [[ $ALLOW_UNHEALTHY == 1 ]] || exit 1
  fi

  if [[ -n "$EXPECT_TEXT" ]] && ! grep -F '^final ' "$output_file" | grep -F -q "$EXPECT_TEXT"; then
    echo "final transcript did not contain expected text: $EXPECT_TEXT" >&2
    [[ $ALLOW_UNHEALTHY == 1 ]] || exit 1
  fi

  if [[ -n "$MAX_FINAL_MS" ]]; then
    reported_ms="$(grep -Eo '^partial NVIDIA Parakeet finalized in [0-9]+ms' "$output_file" | tail -1 | grep -Eo '[0-9]+' || true)"
    if [[ -z "$reported_ms" ]]; then
      echo "helper did not report finalization timing" >&2
      [[ $ALLOW_UNHEALTHY == 1 ]] || exit 1
    elif (( reported_ms > MAX_FINAL_MS )); then
      echo "finalization time ${reported_ms}ms exceeded ${MAX_FINAL_MS}ms" >&2
      [[ $ALLOW_UNHEALTHY == 1 ]] || exit 1
    fi
  fi
fi
