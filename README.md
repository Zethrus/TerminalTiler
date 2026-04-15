# TerminalTiler

TerminalTiler is a native desktop application for launching premium multi-terminal workspaces from reusable templates. Linux builds use Rust, GTK4, libadwaita, and VTE. Windows 11 builds use a native Win32 shell with ConPTY and prefer WSL2, falling back to PowerShell when WSL2 is unavailable.

## Current scope

- Native libadwaita application shell
- Launch deck with preset templates
- Exact tile-count preset editing from the launch deck
- Recursive split-pane layout rendering
- Per-tile working directory resolution
- Per-tile startup command execution through VTE
- Per-tile agent labeling with editable preset save and update flows
- XDG config seeding for user-editable presets
- Native Windows 11 workspace host with WSL2-first and PowerShell-fallback runtime selection
- Linux `.deb` and AppImage packaging
- Windows `.exe` installer and portable zip packaging

## Build-time dependencies

On Ubuntu or Debian, install the GTK and VTE development packages before building from source:

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  pkg-config \
  libgtk-4-dev \
  libadwaita-1-dev \
  libvte-2.91-gtk4-dev \
  libgraphene-1.0-dev
```

## Development

```bash
cargo check
cargo run
```

For a Windows-targeted compile from a Windows machine:

```powershell
cargo check --target x86_64-pc-windows-msvc
cargo run --target x86_64-pc-windows-msvc
```

The first launch seeds presets at the XDG config location for the app. On Linux this is typically:

```text
~/.config/TerminalTiler/presets.toml
```

Application logs and crash reports are written to the XDG state directory. On Linux this is typically:

```text
~/.local/state/terminaltiler/logs/terminaltiler.log
```

Launcher stderr from desktop or packaged starts is also appended separately to:

```text
~/.local/state/terminaltiler/logs/launcher-stderr.log
```

## Packaging

The repo includes release tooling for:

- `.deb` packaging under `packaging/deb`
- AppImage packaging under `packaging/appimage`
- Windows installer packaging under `packaging/windows`

The packaging scripts produce self-contained runtime bundles:

- `packaging/build-appimage.sh` generates a fresh AppDir under `packaging/.build/appimage`, bundles GTK/libadwaita/VTE shared libraries, GSettings schemas, and gdk-pixbuf loaders, then runs `appimagetool`
- `packaging/build-deb.sh` stages the application under `packaging/.build/deb-root/opt/terminaltiler`, bundles the same runtime payload, and installs a launcher at `/usr/bin/terminaltiler`
- `packaging/build-linux-release.sh` builds the release binary once, then emits both Linux artifacts with one shared semantic version
- `packaging/build-in-container.sh` runs the Linux packaging flow inside a pinned Debian 12 build container for reproducible release artifacts
- `packaging/release-smoke-test.sh` validates AppStream metadata, builds both artifacts, inspects their payloads, and performs timed headless launch smoke tests when `xvfb-run` is available
- `packaging/build-windows.ps1` builds `TerminalTiler.exe`, stages the Windows payload, emits a portable zip, and generates an NSIS installer
- `packaging/windows-smoke-test.ps1` validates the portable zip and installer, then performs timed launch smoke tests against both packaged Windows outputs

Each packaging run now derives a clean semantic version from the most recent successful build within the same `major.minor` line. If `Cargo.toml` is at `0.2.0` and no prior successful packaging run has been recorded, the first build emits `0.2.0`, then `0.2.1`, then `0.2.2`, and so on.

If you change `Cargo.toml` to a new `major.minor` base such as `0.2.0` or `1.1.0`, the stored patch counter is ignored and the next successful build starts again from that exact base version, for example `0.2.0` or `1.1.0`. Later successful builds on that line continue with `0.2.1`, `0.2.2`, or `1.1.1`, `1.1.2`.

The last successful build version is stored in `packaging/.build/versioning/last-successful-version`, which is already ignored by git. That file is only updated after a package build completes successfully, so failed runs do not consume a version number.

By default the scripts write versioned artifacts such as `dist/terminaltiler_0.2.2_amd64.deb`, `dist/TerminalTiler-0.2.2-x86_64.AppImage`, `dist/TerminalTiler-0.2.2-windows-x86_64.zip`, and `dist/TerminalTiler-setup-0.2.2-x86_64.exe`. Linux builds refresh `dist/terminaltiler_latest_amd64.deb` and `dist/TerminalTiler-latest-x86_64.AppImage` symlinks, while Windows builds refresh `dist/TerminalTiler-latest-windows-x86_64.zip` and `dist/TerminalTiler-setup-latest-x86_64.exe` copies.

You can override the generated version inputs when needed:

```bash
PACKAGE_VERSION=0.2.0 bash packaging/build-linux-release.sh
PACKAGE_VERSION=0.2.0 bash packaging/build-deb.sh
LAST_SUCCESSFUL_VERSION=0.3.9 bash packaging/build-appimage.sh
```

```powershell
$env:PACKAGE_VERSION = "0.2.0"
./packaging/build-windows.ps1
```

The resulting AppImage and `.deb` are intended to run on supported Ubuntu and Debian desktops without separately installing GTK, libadwaita, or VTE runtime packages.

The resulting Windows installer and portable zip target Windows 11. At runtime they prefer WSL2 when a valid distro is available and fall back to PowerShell otherwise. Browser tiles on Windows require Microsoft Edge WebView2 Runtime (Evergreen): install it before opening any preset or restored session that contains web tiles.

Both artifact formats now also ship reverse-DNS desktop metadata at `usr/share/applications/dev.zethrus.terminaltiler.desktop` and AppStream metadata at `usr/share/metainfo/dev.zethrus.terminaltiler.appdata.xml` for cleaner distribution tooling integration.

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
- build the versioned `.deb`, AppImage, Windows zip, and Windows installer artifacts
- run the Linux and Windows smoke coverage already in the repo
- attach `dist/terminaltiler_X.Y.Z_amd64.deb`, `dist/TerminalTiler-X.Y.Z-x86_64.AppImage`, `dist/TerminalTiler-X.Y.Z-windows-x86_64.zip`, and `dist/TerminalTiler-setup-X.Y.Z-x86_64.exe` to the GitHub Release for that tag

Example:

```bash
git tag v0.2.0
git push origin v0.2.0
```
