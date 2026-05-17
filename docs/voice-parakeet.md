# NVIDIA Parakeet voice-to-text

TerminalTiler's voice-to-text support is local-only. The desktop process owns
preferences, hotkeys, microphone capture, terminal targeting, and HUD/status UI;
the ASR runtime runs in a settings-managed helper process supplied by the voice
pack.

## User flow

1. Open **Settings → Voice input**.
2. Enable local NVIDIA Parakeet voice-to-text.
3. Pick a microphone or keep **System default**.
4. Choose push-to-talk or toggle activation.
5. Choose engine mode:
   - **Auto** prefers CUDA when PyTorch reports CUDA availability, otherwise CPU.
   - **CUDA** fails health checks if CUDA is unavailable.
   - **CPU** uses CPU and attempts dynamic quantization.
6. Install / reinstall the voice pack.
7. Run **Health Check**. A healthy pack reports Python, Torch, NeMo, selected
   device, CUDA device, quantization, model, and cache path.

Final transcript chunks are inserted only when a TerminalTiler terminal pane is
focused. Web tiles, settings dialogs, and other surfaces show a “no terminal
target” status and receive no transcript text. No newline or Enter key is sent.

## Voice pack contents

The built-in pack scaffold lives under `resources/voice/parakeet/`:

- `manifest.toml` declares `nvidia/parakeet-tdt-0.6b-v2`, helper path, model
  cache path, and pinned Python dependency ranges.
- `parakeet_engine.py` implements the line-oriented helper protocol:
  - `health`
  - `start <sample_rate_hz>`
  - `audio-pcm16-hex <hex little-endian PCM16>`
  - `stop`
  - `shutdown`

The Settings install path copies these files into the app data voice-pack
directory, creates a pack-local Python virtual environment, installs the pinned
requirements, and runs a helper health check before marking the pack installed.

## Verification on provisioned machines

Use the verifier when Torch/NeMo/model downloads are available:

```bash
packaging/voice-pack-verify.sh --mode auto --timeout 300s
```

Transcribe a known-good 16 kHz mono PCM16 WAV and validate content/latency:

```bash
packaging/voice-pack-verify.sh \
  --mode cuda \
  --audio /path/to/sample-16khz-mono-pcm16.wav \
  --expect-text "expected phrase" \
  --max-final-ms 2500 \
  --timeout 600s
```

Diagnostic mode prints protocol output without failing when dependencies are
missing:

```bash
packaging/voice-pack-verify.sh --allow-unhealthy --timeout 30s
```

## Manual release checks

Run these checks on Linux X11, Linux Wayland, and Windows before claiming a full
voice release:

- Settings lists microphones and preserves the selected device.
- Pack install creates a pack-local venv and health check loads the Parakeet
  model.
- Auto mode uses CUDA when available and falls back to CPU otherwise.
- CUDA mode fails clearly when CUDA is unavailable.
- CPU mode runs without CUDA and reports whether quantization was applied.
- Push-to-talk starts on key down and flushes on key up.
- Toggle starts and stops on repeated hotkey activations.
- Linux app-scoped hotkeys work.
- Linux X11 global hotkey works when available.
- Linux Wayland reports global hotkeys unavailable and falls back to app-scoped.
- Windows app-scoped and best-effort global hotkeys work.
- HUD shows listening, captured audio activity, transcribing, finalization time,
  engine errors, and no-target status.
- Focused terminal receives finalized transcript text only.
- Focused web tiles/settings/other surfaces receive no transcript text.

## Build gates

Standard gates:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --no-run
```

Voice capture gate on Linux requires ALSA development headers:

```bash
sudo apt install -y libasound2-dev
cargo check --features voice-cpal
```

Windows cross-target gates:

```bash
cargo check --target x86_64-pc-windows-gnu --features voice-cpal
cargo check --target x86_64-pc-windows-msvc --features voice-cpal
```
