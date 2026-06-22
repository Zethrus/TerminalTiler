//! `terminaltiler-mcp` — the Kanban board MCP server.
//!
//! Ships bundled with TerminalTiler. AI clients (Claude/Codex) spawn it as a stdio
//! subprocess with an explicit `--project-root`; it serves the board at
//! `<project-root>/.terminaltiler/board.json`. For older configs without the flag, it
//! falls back to `<cwd>/.terminaltiler/board.json`. All logic lives in
//! `terminaltiler::mcp`.

fn main() {
    terminaltiler::mcp::run_stdio();
}
