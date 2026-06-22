# TerminalTiler

TerminalTiler is a native desktop application for launching polished multi-terminal workspaces from reusable templates and coordinating per-project Kanban task boards for human and AI-agent work. Linux builds use Rust, GTK4, libadwaita, and VTE. Windows 11 release builds use the GTK/libadwaita parity shell with interactive Windows terminal panes, and prefer WSL2 while falling back to PowerShell when WSL2 is unavailable; the native Win32 shell remains an explicit compatibility fallback.

- Product site: <https://terminaltiler.app>
- Source code: <https://github.com/Zethrus/TerminalTiler>
- Releases: <https://github.com/Zethrus/TerminalTiler/releases>

## Current scope

- Native libadwaita application shell
- Launch dashboard with saved workspace cards, saved Kanban board cards, and preset templates
- Exact tile-count preset editing from the launch deck
- Recursive split-pane layout rendering
- Per-tile working directory resolution
- Per-tile startup command execution through VTE
- Per-tile agent labeling with editable preset save and update flows
- Per-project Kanban boards with To Do, In Progress, In Review, Complete, and Cancelled columns
- Task cards with descriptions, assignees, notes, additional instructions, knowledge entries, and file attachments
- Live Claude/Codex task dispatch from the board into terminal panes
- Bundled `terminaltiler-mcp` server so MCP-connected agents can claim tasks, add progress notes, record knowledge, and move work to review
- XDG config seeding for user-editable presets
- Native Windows 11 workspace host with WSL2-first and PowerShell-fallback runtime selection
- Linux `.deb` and AppImage packaging
- Windows `.exe` installer and portable zip packaging

## License

This repository is the public TerminalTiler core and is released under the MIT License. See [LICENSE](LICENSE) for details.

TerminalTiler follows an open-core product model: the core app stays public and useful, while this repository stays focused on the public desktop application. The public repository remains the source of truth for the open-source core.

## Open-core boundary

TerminalTiler Core is and will remain the free, public, MIT-licensed desktop application. This repository contains the public desktop application and must remain buildable and useful on its own. Core must remain buildable and useful without external code, external credentials, or unpublished build steps.

The dependency direction is intentionally one-way: External projects may use Core public APIs, but Core must remain independent. Existing Core features should not be removed or degraded.

## TerminalTiler Core

This public repository contains TerminalTiler Core: the MIT-licensed desktop app, local workspace launcher, release packaging, and public development history. Core should remain useful without external repositories, external services, external credentials, or unpublished build steps.

External materials must stay outside this repository; this repository remains the source of truth for the open-source core.

## Kanban boards and AI agents

TerminalTiler includes local, per-project Kanban boards for coordinating work beside a terminal workspace. Create a board from the launch dashboard with **New Kanban Board**, or open the board for the active workspace with **Ctrl+Shift+K** or the command palette. Board data is stored in the project itself:

```text
<project-root>/.terminaltiler/board.json
```

Saved board cards on the launch dashboard are only shortcuts back to project roots. They are stored in the app config directory and deleting a shortcut does not delete the project board file.

Each board has five columns: To Do, In Progress, In Review, Complete, and Cancelled. Cards can be created in any column, dragged between columns, advanced with the card action button, deleted, or opened for detail editing. Task details include:

- additional instructions injected into agent prompts
- knowledge entries captured by agents through MCP
- file attachments copied under `.terminaltiler/attachments/<task_id>/`
- progress notes and assignee metadata

The **Connect Agent** action registers the bundled `terminaltiler-mcp` server with Claude Code or Codex. Claude uses a project `.mcp.json`; Codex uses `~/.codex/config.toml`. The board health panel calls out missing binaries, missing config, and wrong-project-root registrations so the same Connect/Repair action can fix them. Once connected, agents can use the board tools to list tasks, inspect queue summaries, claim work, append notes, capture research findings, and move implementation-ready tasks to `in_review`.

Board-launched implementation runs first claim/start the task with TerminalTiler's soft-lease guard, then spawn Claude or Codex in a live terminal pane rooted at the project directory. Fresh lease conflicts show a board banner instead of launching; the card **Run agent** action can explicitly take over, while drag-to-**In Progress** only reports the conflict. The header **Run next** action uses the same available-task selection as the MCP `start_next_work` tool. Moving a task to In Review can start one duplicate-gated review run, and review launch failures are saved in task review metadata plus notes for diagnosis. Completion remains a manual board decision after review.

See [docs/kanban-board.md](docs/kanban-board.md) for the full board workflow, MCP tool reference, storage paths, and packaging notes.

## Build-time dependencies

On Ubuntu 24.04 LTS (recommended) or Debian 12, install Rust and the GTK/VTE development packages before building from source:

```bash
# Install Rust (rustup is the recommended way)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

sudo apt update
sudo apt install -y \
  build-essential \
  pkg-config \
  patchelf \
  file \
  wget \
  ca-certificates \
  libgtk-4-dev \
  libadwaita-1-dev \
  libvte-2.91-gtk4-dev \
  libgraphene-1.0-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-6.0-dev \
  libwebkitgtk-6.0-dev \
  libasound2-dev

# Download appimagetool (required for AppImage packaging; not available in apt)
sudo wget -q https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
  -O /usr/local/bin/appimagetool \
  && sudo chmod +x /usr/local/bin/appimagetool
```


## Local voice-to-text

TerminalTiler includes local NVIDIA Parakeet voice-to-text support behind the
optional `voice-cpal` feature. The app captures microphone audio locally, runs a
settings-managed Parakeet helper/voice pack, and inserts finalized transcript
chunks only into the focused TerminalTiler terminal pane. See
[docs/voice-parakeet.md](docs/voice-parakeet.md) for setup, verification, and
manual release checks. Windows voice-pack installation requires a 64-bit Python
3.10–3.13 host interpreter (3.12 or 3.13 recommended; 3.14+ is unsupported by
the Parakeet/NeMo dependency set) and repairs/recreates the pack-local `.venv`
when pip is damaged or the venv Python is incompatible.

## Development

```bash
cargo check
cargo test
cargo run --bin terminaltiler
```

The Kanban MCP server is built as a second binary and normally runs under an MCP client. For local protocol testing, point it at a project root:

```bash
cargo run --bin terminaltiler-mcp -- --project-root "$PWD"
```

For a Windows-targeted compile from a Windows machine:

```powershell
cargo check --target x86_64-pc-windows-msvc
cargo run --bin terminaltiler --target x86_64-pc-windows-msvc
```

The first launch seeds presets at the XDG config location for the app. On Linux this is typically:

```text
~/.config/TerminalTiler/presets.toml
```

Application logs and crash reports are written to the platform state directory. On Linux this is typically:

```text
~/.local/state/terminaltiler/logs/terminaltiler.log
```

On Windows this is typically:

```text
%LOCALAPPDATA%\Zethrus\TerminalTiler\state\logs\terminaltiler.log
```

Launcher stderr from desktop or packaged starts is also appended separately to:

```text
~/.local/state/terminaltiler/logs/launcher-stderr.log
```

Project board data lives under each project root:

```text
<project-root>/.terminaltiler/board.json
<project-root>/.terminaltiler/attachments/
<project-root>/.terminaltiler/reviews/
```

Saved Kanban board shortcuts are stored with user configuration. On Linux this is typically:

```text
~/.config/TerminalTiler/board-workspaces.toml
```

## Packaging

The repo includes release tooling for:

- `.deb` packaging under `packaging/deb`
- AppImage packaging under `packaging/appimage`
- Windows installer packaging under `packaging/windows`

The packaging scripts produce self-contained runtime bundles:

- `packaging/build-appimage.sh` generates a fresh AppDir under `packaging/.build/appimage`, bundles GTK/libadwaita/VTE shared libraries, GSettings schemas, gdk-pixbuf loaders, and `terminaltiler-mcp`, then runs `appimagetool`
- `packaging/build-deb.sh` stages the application under `packaging/.build/deb-root/opt/terminaltiler`, bundles the same runtime payload plus `terminaltiler-mcp`, and installs a launcher at `/usr/bin/terminaltiler`
- `packaging/build-linux-release.sh` builds the release binary once, then emits both Linux artifacts with one shared semantic version
- `packaging/build-in-container.sh` runs the Linux packaging flow inside a pinned Debian 12 build container for reproducible release artifacts
- `packaging/release-smoke-test.sh` validates AppStream metadata, builds or reuses both Linux artifacts, inspects their payloads, and performs timed headless launch smoke tests when `xvfb-run` is available
- `packaging/resolve-package-version.sh` resolves deterministic package versions for local builds, default-branch snapshot builds, and tagged releases
- `packaging/build-windows.ps1` builds `TerminalTiler.exe`, stages the Windows payload with `terminaltiler-mcp.exe`, emits a direct portable `.exe`, a portable zip, an NSIS installer `.exe`, and a WiX `.msi`
- `packaging/windows-smoke-test.ps1` validates the direct portable `.exe`, the portable zip payload, the NSIS installer, and the MSI package before launch smoke coverage

Local packaging runs still derive a clean semantic version from the most recent successful build within the same `major.minor` line. If `Cargo.toml` is at `0.2.0` and no prior successful local packaging run has been recorded, the first build emits `0.2.0`, then `0.2.1`, then `0.2.2`, and so on.

If you change `Cargo.toml` to a new `major.minor` base such as `0.2.0` or `1.1.0`, the stored patch counter is ignored and the next successful build starts again from that exact base version, for example `0.2.0` or `1.1.0`. Later successful builds on that line continue with `0.2.1`, `0.2.2`, or `1.1.1`, `1.1.2`.

The last successful local build version is stored in `packaging/.build/versioning/last-successful-version`, which is already ignored by git. That file is only updated after a package build completes successfully, so failed runs do not consume a version number.

GitHub Actions does not rely on that local file. CI resolves versions with `packaging/resolve-package-version.sh`:

- tagged releases use the exact `vX.Y.Z` tag value as `X.Y.Z`
- default-branch snapshot builds keep the `major.minor` line from `Cargo.toml` and use the GitHub Actions run number as the patch version

For example, if `Cargo.toml` is `0.2.0` and the packaging workflow run number is `156`, the snapshot artifacts are built as `0.2.156`.

By default the scripts write versioned artifacts such as `dist/terminaltiler_0.2.2_amd64.deb`, `dist/TerminalTiler-0.2.2-x86_64.AppImage`, `dist/TerminalTiler-0.2.2-portable-x86_64.exe`, `dist/TerminalTiler-0.2.2-windows-x86_64.zip`, `dist/TerminalTiler-setup-0.2.2-x86_64.exe`, and `dist/TerminalTiler-setup-0.2.2-x86_64.msi`. Linux builds refresh `dist/terminaltiler_latest_amd64.deb` and `dist/TerminalTiler-latest-x86_64.AppImage` symlinks, while Windows builds refresh `dist/TerminalTiler-latest-portable-x86_64.exe`, `dist/TerminalTiler-latest-windows-x86_64.zip`, `dist/TerminalTiler-setup-latest-x86_64.exe`, and `dist/TerminalTiler-setup-latest-x86_64.msi` copies.

You can override the generated version inputs when needed:

```bash
PACKAGE_VERSION=0.2.0 bash packaging/build-linux-release.sh
PACKAGE_VERSION=0.2.0 bash packaging/build-deb.sh
LAST_SUCCESSFUL_VERSION=0.3.9 bash packaging/build-appimage.sh
```

```powershell
$env:PACKAGE_VERSION = "0.2.0"
./packaging/build-windows.ps1 -RequireInstallers
```

The resulting AppImage and `.deb` are intended to run on supported Ubuntu and Debian desktops without separately installing GTK, libadwaita, or VTE runtime packages.

The resulting Windows installer artifacts, MSI, portable `.exe`, and portable zip target Windows 11. At runtime they prefer WSL2 when a valid distro is available and fall back to PowerShell otherwise. Browser tiles on Windows use Microsoft Edge WebView2 Runtime (Evergreen). The setup `.exe` installs the Evergreen runtime automatically when it is missing; portable and MSI artifacts require WebView2 to already be installed.

Both artifact formats now also ship reverse-DNS desktop metadata at `usr/share/applications/app.terminaltiler.desktop`, AppStream metadata at `usr/share/metainfo/app.terminaltiler.metainfo.xml`, and rasterized hicolor icons (64/128/256 px) so GNOME Software / Ubuntu App Center render the icon, publisher (BootKode Corp.), and license correctly.

For local paired Linux artifacts, use:

```bash
bash packaging/build-linux-release.sh
```

The preferred pinned release path is:

```bash
bash packaging/release-verify.sh
```

GitHub Actions also publishes tagged releases automatically. Push a semver tag in the form `vX.Y.Z`, for example `v0.2.0`, and the `Release` workflow will:

- set `PACKAGE_VERSION` from the tag value
- build the Linux `.deb` and AppImage artifacts through the pinned Debian 12 container path
- build the Windows portable `.exe`, NSIS installer `.exe`, and WiX `.msi` artifacts
- run the Linux and Windows smoke coverage already in the repo
- attach `dist/terminaltiler_X.Y.Z_amd64.deb`, `dist/TerminalTiler-X.Y.Z-x86_64.AppImage`, `dist/TerminalTiler-X.Y.Z-portable-x86_64.exe`, `dist/TerminalTiler-setup-X.Y.Z-x86_64.exe`, and `dist/TerminalTiler-setup-X.Y.Z-x86_64.msi` to the GitHub Release for that tag

Pushes to the repository default branch also trigger the `Package Artifacts` workflow. That workflow resolves a snapshot package version automatically, builds the Linux and Windows distributables, runs the same smoke coverage, and uploads the resulting `.deb`, `.AppImage`, portable `.exe`, installer `.exe`, and `.msi` files as GitHub Actions artifacts for that run.

Patch release tags can be generated automatically from the latest matching git tag on the current `major.minor` line in `Cargo.toml`. If `Cargo.toml` still says `0.2.0` and the latest tag is `v0.2.157`, the next generated tag will be `v0.2.158`. When you intentionally start a new release line, update `version = "..."` in `Cargo.toml` first.

Local release tagging:

```bash
bash packaging/create-release-tag.sh --dry-run
bash packaging/create-release-tag.sh
```

The local script:

- fetches `origin` tags for the default branch
- requires a clean checkout on `main` that matches `origin/main`
- derives the next patch tag automatically
- runs `packaging/release-verify.sh` by default before creating the tag
- creates and pushes an annotated tag, which triggers the `Release` workflow

GitHub Actions also exposes a manual `Create Release Tag` workflow. Run it from the Actions UI when you want GitHub to create and push the next tag for you. The workflow tags with the standard `github-actions[bot]` identity and then explicitly dispatches the `Release` workflow for the new tag. By default it skips the local Linux preflight and lets the downstream `Release` workflow handle build validation, but you can enable the preflight input when you want that extra gate before tagging.

Manual equivalent:

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```
