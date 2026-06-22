# TerminalTiler Core Boundary

TerminalTiler Core is the public, MIT-licensed foundation of TerminalTiler. It should remain useful, buildable, and understandable without external repositories, external services, external credentials, or unpublished build steps.

TerminalTiler follows an open-core product model: the core app stays public and useful, while this repository stays focused on the public desktop application. The public repository remains the source of truth for the open-source core.

## Core Stays Public

- Local launch deck and workspace templates
- Local presets and builtin starter presets
- Split layouts, tile editing, tile swapping, closing, and reconnecting
- Per-tile working directories and startup commands
- Local session restore and recovery flows
- Local settings, theme, density, zoom, and keyboard shortcuts
- Local per-project Kanban boards, task metadata, notes, knowledge entries, and attachments
- Bundled `terminaltiler-mcp` server for local MCP-connected agent workflows
- Public release packaging for Linux and Windows
- Public issue tracking and source history

Already-public features should not be hidden retroactively. Keep public functionality available in this repository.

## External Boundaries

External materials must stay outside this repository. Public Core APIs may be used by other projects, but Core must remain independent of external code, credentials, services, and unpublished build steps.

Optional agent integrations must stay local and replaceable. TerminalTiler Core may register the bundled MCP server with local Claude Code or Codex configuration, but the app and board remain useful without those CLIs, network services, external credentials, or private repositories.

## Stable Public Identity

Use `TerminalTiler Core` as display wording in the app and docs. Keep the binary name, app ID, package name, desktop ID, and config paths stable unless there is a deliberate migration plan.
