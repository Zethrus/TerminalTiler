//! `terminaltiler-mcp` — the Kanban board MCP server.
//!
//! Ships bundled with TerminalTiler. AI clients (Claude/Codex) spawn it as a stdio
//! subprocess in a project directory; it serves the board at
//! `<cwd>/.terminaltiler/board.json`. All logic lives in `terminaltiler::mcp`.

fn main() {
    terminaltiler::mcp::run_stdio();
}
