# TerminalTiler Core Boundary

TerminalTiler Core is the public, MIT-licensed foundation of TerminalTiler. It should remain useful, buildable, and understandable without private repositories, paid services, cloud credentials, or closed-source build steps.

TerminalTiler follows an open-core product model: the core app stays public and useful, while future Pro offerings may add paid workflow packs, convenience features, official support, or private extensions. The public repository remains the source of truth for the open-source core.

## Core Stays Public

- Local launch deck and workspace templates
- Local presets and builtin starter presets
- Split layouts, tile editing, tile swapping, closing, and reconnecting
- Per-tile working directories and startup commands
- Local session restore and recovery flows
- Local settings, theme, density, zoom, and keyboard shortcuts
- Public release packaging for Linux and Windows
- Public issue tracking and source history

Already-public features should not be hidden retroactively just to create a paid tier. If paid differentiation is needed, build additive Pro value around polished packs, sync, support, or private extensions.

## Pro Can Be Additive

A future private `TerminalTiler-Pro` repository can contain paid code or paid content when there is something real to protect:

- Workflow packs and curated templates
- Paid runbook, snippet, role, and output-helper packs
- License and checkout tooling
- Cloud sync or team-sharing services
- Commercial support documents
- Private extension modules
- Signed or convenience release automation

The dependency direction should stay one-way: Pro may depend on Core, but Core must never require Pro.

## Stable Public Identity

Use `TerminalTiler Core` as display wording in the app and docs. Keep the binary name, app ID, package name, desktop ID, and config paths stable unless there is a deliberate migration plan.
