# Windows GTK Visual QA

TerminalTiler treats the Ubuntu/Linux GTK shell as the canonical visual baseline. The Windows GTK shell must load the same `resources/style.css`, TerminalTiler logo, hover icons, visual role classes, density classes, and light/dark theme classes. Only unavoidable OS differences are acceptable: font rasterization, titlebar/window-frame behavior, compositor shadows, and external system dialogs.

## Automated preflight before screenshots

Run these on a native, interactive Windows 11 desktop with the MSVC Rust target installed:

```powershell
./packaging/setup-windows-gtk.ps1 -InstallWithGvsbuild -SkipBuildIfPresent
cargo check --target x86_64-pc-windows-msvc --features voice-cpal,windows-gtk-shell
./packaging/build-windows.ps1 -UseGtkShell -GtkRuntimeRoot $env:TERMINALTILER_GTK_RUNTIME_ROOT -RequireInstallers
./packaging/windows-smoke-test.ps1 -UseGtkShell -GtkRuntimeRoot $env:TERMINALTILER_GTK_RUNTIME_ROOT -SmokeProfileKind terminal-only -SkipBuild
```

`setup-windows-gtk.ps1` accepts `TERMINALTILER_GTK_RUNTIME_ROOT` if you already have a GTK runtime, and otherwise can build one with gvsbuild. The script exports `PATH`, `LIB`, `INCLUDE`, and `PKG_CONFIG_PATH` for gtk-rs/MSVC builds.

GitHub-hosted Windows runners do not expose an interactive desktop: `GtkApplication`
can create its application object but exits before activation. CI therefore verifies
the GTK build, packaged payload, ZIP, NSIS, and MSI paths with `-SkipLaunchSmoke`.
Run the command above without that switch on an interactive Windows host before
approving GTK visual or runtime changes.

## Capture helpers

Capture the Ubuntu/Linux GTK reference first with the same seeded profiles:

```bash
cargo build --release --features voice-cpal
./packaging/capture-linux-gtk-visuals.sh \
  --exe ./target/release/terminaltiler \
  --theme dark \
  --density compact
```

The Linux helper writes PNGs under
`packaging/.build/linux-gtk-visuals/`. It uses the same scenario names,
profile seed data, theme values, and density values as the Windows helper so
review bundles can compare matching windows directly.

After building a GTK package, capture starter screenshots with:

```powershell
./packaging/capture-windows-gtk-visuals.ps1 `
  -ExePath .\dist\TerminalTiler-latest-portable-x86_64.exe `
  -Theme dark `
  -Density compact
```

The helper writes PNGs under `packaging/.build/windows-gtk-visuals/`. It seeds isolated profiles for:

- `launch-dashboard`: clean first-run launch deck.
- `saved-workspaces`: launch deck with seeded saved workspace cards and mixed terminal/web tile badges.
- `restored-workspace`: restored 3-pane workspace in the shared interactive GTK workspace shell. This verifies the canonical release no longer opens the legacy Win32 workspace host for restored sessions.
- `workspace-with-web`: restored workspace with one terminal tile and one `about:blank` web tile so WebKit/WebView2 framing, header chrome, and settings affordances can be compared without relying on external network content.

The capture helper follows the launched process tree, so it works with the
published self-extracting portable `.exe` as well as an unpacked
`TerminalTiler.exe`.

To prove the full release set is visually equivalent, capture every Windows
artifact form from the same staged GTK payload:

```powershell
./packaging/capture-windows-release-gtk-visuals.ps1 `
  -PackageVersion $env:PACKAGE_VERSION `
  -Theme dark `
  -Density compact
```

The release helper captures these artifact labels into separate subdirectories
under `packaging/.build/windows-gtk-release-visuals/`:

- `portable-exe`: published self-extracting portable executable.
- `portable-zip`: extracted portable zip payload.
- `nsis-install`: silent NSIS install location.
- `msi-install`: silent MSI install location.

Repeat with `-Theme light` and each density (`comfortable`, `standard`, `compact`) when preparing a complete review bundle.

## Automated screenshot comparison

After both capture helpers have produced matching Linux and Windows PNGs, run:

```bash
./packaging/compare-gtk-visuals.sh \
  --linux-dir ./packaging/.build/linux-gtk-visuals \
  --windows-dir ./packaging/.build/windows-gtk-visuals \
  --theme dark \
  --density compact \
  --threshold 0.035
```

The comparison helper pairs captures by scenario, index, theme, and density; writes diff
PNGs under `packaging/.build/gtk-visual-diffs/`; and emits
`packaging/.build/gtk-visual-diffs/report.tsv`. It fails when dimensions differ,
when a matching capture for the same theme/density is missing, or when
normalized RMSE exceeds the chosen threshold. Use the TSV plus diff PNGs as the objective evidence for parity
bugs before marking a screenshot pair `pass`, `minor`, or `fail`.

## Manual screenshot checklist

Capture these Windows GTK screens and pair each with the current Ubuntu reference at the same app size:

1. Launch dashboard / launch deck.
2. Saved workspace cards.
3. New/edit wizard: setup, appearance, layout, and tiles steps.
4. Active/restored 3-pane workspace in the shared GTK shell.
5. Restored terminal + web workspace in the shared GTK shell.
6. Tab strip and command/app chrome.
7. Active workspace toolbar / summary controls.
8. Terminal tile card headers, pane chips, focus states, hover states.
9. Buttons/chips in primary, secondary, ghost, surface, destructive, disabled, and focused states.
10. Dark and light themes.
11. Comfortable, standard, and compact density modes.
12. Release artifact parity across `portable-exe`, `portable-zip`, `nsis-install`, and `msi-install`.
13. Taskbar, window, installer, and portable-exe icons all show the TerminalTiler icon instead of the generic GTK/Windows fallback.
14. Portable-exe clean first-run uses the wrapper directory/current working folder as the launch deck workspace root, never an `nsx*.tmp` self-extraction directory.

## Naming convention for review bundles

Use this form so screenshots can be diffed and discussed unambiguously:

```text
<platform>-<theme>-<density>-<screen>-<state>.png
```

Examples:

```text
ubuntu-dark-compact-launch-dashboard-default.png
windows-dark-compact-launch-dashboard-default.png
ubuntu-light-standard-workspace-3pane-focused-terminal.png
windows-light-standard-workspace-3pane-focused-terminal.png
```

## Acceptance rubric

Mark each screenshot pair as:

- `pass`: visually equivalent except for allowed OS-level differences.
- `minor`: small spacing/rasterization/state difference that does not change hierarchy or affordance.
- `fail`: layout, spacing, color, contrast, typography, density, state, or component mismatch that would make Windows feel like a different design system.

Record failures with:

- screenshot pair names;
- exact component/region;
- expected Ubuntu behavior;
- observed Windows behavior;
- whether the fix belongs in shared CSS/classes, Windows GTK resource/runtime packaging, or a platform adapter.
