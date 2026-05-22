# Copilot instructions for TerminalTiler

TerminalTiler is a Rust desktop app with platform-specific frontends: Linux uses GTK4/libadwaita/VTE/WebKit and Windows release builds use the GTK/libadwaita parity shell with interactive Windows terminal panes, WSL2-first terminal launching, PowerShell fallback, and a Win32 compatibility shell only when explicitly selected.

## Build, test, and lint commands

- Install Linux CI/build dependencies: `bash packaging/install-ubuntu-workflow-deps.sh`
- Check the default target: `cargo check`
- Run the Linux app: `cargo run`
- Format check: `cargo fmt --check`
- Test suite: `cargo test`
- Run one test by substring or full module path, for example: `cargo test cycles_density_in_expected_order` or `cargo test model::preset::tests::cycles_density_in_expected_order`
- Clippy gate used by CI: `cargo clippy --all-targets --all-features -- -D warnings`
- Dependency audit used by CI: `cargo audit`
- Shell script lint used by CI:
  `shellcheck -x packaging/build-linux-release.sh packaging/build-appimage.sh packaging/build-deb.sh packaging/build-in-container.sh packaging/resolve-package-version.sh packaging/release-smoke-test.sh packaging/release-verify.sh packaging/run-bundled.sh packaging/versioning.sh`
- Windows target check from a Windows machine: `cargo check --target x86_64-pc-windows-msvc`
- Windows run from a Windows machine: `cargo run --target x86_64-pc-windows-msvc`
- Preferred pinned Linux release verification: `bash packaging/release-verify.sh`
- Local paired Linux artifacts: `bash packaging/build-linux-release.sh`
- Windows release artifacts from PowerShell: `./packaging/build-windows.ps1 -RequireInstallers`
- Windows smoke test from PowerShell: `./packaging/windows-smoke-test.ps1 -SkipLaunchSmoke -SmokeProfileKind terminal-only -SkipBuild`

CI pins Rust through `rust-toolchain.toml`/`dtolnay/rust-toolchain` to 1.92.0 with `rustfmt` and `clippy`.

## High-level architecture

- `src/lib.rs` is the public entrypoint and gates platform modules with `#[cfg(target_os = "...")]`. Linux delegates to `app::run()`, Windows delegates to `windows::run()`, and unsupported platforms only initialize logging and report unsupported status.
- Shared domain data lives under `src/model/`: presets, recursive split layouts, tile specs, workspace assets, runbooks, snippets, connection profiles, inventory hosts, output helper rules, and workspace config.
- Shared business logic lives under `src/services/`: launch resolution, layout editing, tile draft resizing, project suggestion detection, session restore decisions, alerts, broadcasts, snippets, runbooks, assets editing, and template variable handling. Keep launch transport decisions centralized in `services/launch_resolution.rs` instead of duplicating them in UI code.
- Shared persistence lives under `src/storage/`: versioned user documents are stored through `directories::ProjectDirs::from("dev", "Zethrus", "TerminalTiler")`, written with `atomic_write_private`, and corrupt files are moved aside with `preserve_corrupt_file` before falling back to safe defaults.
- Linux app flow starts in `src/app/mod.rs`: initialize logging, configure the WebKit process environment, start the tray controller, seed preset/assets stores, load saved sessions, and present the GTK window. GTK UI composition is under `src/ui/`, VTE terminal spawning is under `src/terminal/`, and tray behavior is in `src/tray.rs`.
- Windows app flow starts in `src/windows/app.rs`: native Win32 windows, launcher/settings UI, tray handling, runtime probing, and workspace launch orchestration. Terminal/runtime details are split across `src/windows/workspace.rs`, `src/windows/wsl.rs`, and `src/windows/vt.rs`.
- `resources/` contains desktop/AppStream metadata, SVG icon, and GTK CSS. Packaging scripts bundle these resources plus runtime dependencies into `.deb`, AppImage, Windows portable `.exe`/zip, NSIS installer, and WiX MSI outputs.

## Key conventions

- Preserve the open-core boundary from `README.md` and `docs/core-boundary.md`: this public Core repo must stay buildable and useful without external code, external credentials, external services, or unpublished build steps.
- Keep stable public identity unless there is a deliberate migration plan: display wording `TerminalTiler Core`, binary/package name `terminaltiler`, app ID/desktop ID `dev.zethrus.terminaltiler`, and existing config/state paths.
- Prefer shared model/service/storage modules for cross-platform behavior, then adapt platform presentation separately in `src/ui/` and `src/windows/`.
- Serialized user-facing documents commonly use Serde kebab-case enums/fields, tagged enums with `#[serde(tag = "...", content = "...")]`, defaults for newly added fields, and aliases for migrations. Preserve backward compatibility when changing preset, preference, asset, session, or workspace config formats.
- Store documents carry a `version` and should return explicit warnings or `io::Result` errors when config directories, reads, writes, or parsing fail. Do not silently ignore persistence failures; follow the existing logging and recovery-copy pattern.
- Builtin presets/assets are seeded into user-editable config on first launch. Reset flows should preserve user-created entries and only replace builtin entries identified by known builtin IDs.
- Layouts are recursive `LayoutNode::Split`/`Tile` trees. When changing tile metadata without changing structure, use existing helpers such as `tile_specs()` and `with_tile_specs()` to preserve traversal order.
- Terminal launching starts from `TileSpec` plus workspace root/assets. Linux resolves to VTE argv/env, while Windows resolves to WSL, PowerShell, or SSH command structs; keep quoting/path translation in the existing platform helpers.
- Log through `src/logging.rs` rather than ad hoc output when reporting app/runtime errors. README documents logs under the XDG state directory on Linux.
- Avoid treating generated artifacts as source: `target/`, `dist/`, `squashfs-root/`, and `packaging/.build/` are build outputs.
