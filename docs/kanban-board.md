# Kanban Board and MCP Agents

TerminalTiler's Kanban board is local-first project state for coordinating tasks beside a terminal workspace. The GTK board UI and the `terminaltiler-mcp` server use the same data model and service functions, so humans and MCP-connected agents operate on one shared board.

## Storage model

Each project owns its board:

```text
<project-root>/.terminaltiler/board.json
```

The board schema is versioned and currently stores:

- tasks with title, description, status, assignee, timestamps, and progress notes
- additional instructions for agent prompts
- knowledge entries captured by agents
- attachment metadata for files copied into `.terminaltiler/attachments/<task_id>/`
- review metadata used to avoid duplicate automatic review dispatch, including the latest review launch error when a claimed review could not start
- board automation defaults for implementation agent, reviewer, and YOLO mode

All board mutations go through a cross-process lock at `.terminaltiler/board.lock` and are written atomically. This keeps the desktop UI, live agent terminals, and headless MCP clients from overwriting each other.

Saved board cards in the launch dashboard are shortcuts, not the board itself. They are stored in the app config directory as `board-workspaces.toml` and point back to project roots. Deleting a saved shortcut leaves `.terminaltiler/board.json` on disk.

## Opening a board

Use one of these entry points:

- From the launch dashboard, choose **New Kanban Board**, select a project directory, name the board shortcut, review the summary, and open it.
- From a saved board card on the launch dashboard, choose **Open**.
- From an active workspace, press **Ctrl+Shift+K** or use the command palette action to open the board for that workspace root.

The board opens as its own tab. Closing a board tab terminates any live agent terminals owned by that board tab after the normal close confirmation.

## Board workflow

The board has five columns:

- To Do
- In Progress
- In Review
- Complete
- Cancelled

Use **New Task** to create a card with title, description, and initial column. Cards can be dragged between columns, advanced to the next column with the card action button, refreshed from disk, deleted, or opened for task details.

Use a card's **Run agent** menu to dispatch an implementation agent. TerminalTiler repairs/checks the agent MCP setup, claims the task with the selected agent assignee, and only then opens the live terminal. If another assignee has a fresh active lease, the board shows a conflict banner and does not launch automatically; the **Run agent** flow is the only place that offers **Take over and run**.

The board header also includes **Run next**, which uses the same queue rules as the MCP `start_next_work` tool: first unblocked To Do task in board order, skipping fresh active leases. Dragging a card into **In Progress** uses the same safe claim path but never forces takeover; conflicts and missing tasks are shown in the status banner. Moving a card to **In Review** can start one automatic review. Moving a card to **Cancelled** stops live agent runs for that task.

## Task details

Click a card body to open the task detail dialog. The dialog has three tabs:

- **Instructions** saves extra instructions that are injected into implementation and review prompts.
- **Knowledge** displays entries recorded by agents through `add_task_knowledge`.
- **Attachments** imports local files for agent context. Supported images appear as inline
  thumbnails; select one to view that image at full size without cropping. Other file types
  have an **Open** action that launches the desktop default application.

Attachments are copied under:

```text
<project-root>/.terminaltiler/attachments/<task_id>/
```

Current attachment limits are 10 files per task, 25 MB per file, and these extensions: `png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `svg`, `pdf`, `doc`, `docx`, `xls`, `xlsx`, `csv`, `txt`, `md`, `json`, and `zip`.

## Connecting agents

Use **Connect Agent** from a board tab to register the bundled `terminaltiler-mcp` server and set board automation defaults.

Supported agent CLIs:

- Claude Code, registered in the project `.mcp.json`
- Codex, registered in `~/.codex/config.toml`

The connection step is idempotent and preserves other MCP servers in the existing config. The generated server entry passes `--project-root <project-root>`, so the MCP server always serves the selected project board rather than whichever directory the agent process starts in. The MCP health panel distinguishes these setup states:

- MCP binary present vs. missing/PATH lookup failure
- Claude config ready, missing, unreadable, or targeting the wrong project root
- Codex config ready, missing, unreadable, or targeting the wrong project root

Use **Connect Agent** again to repair a missing or mismatched registration.

Example Claude config shape:

```json
{
  "mcpServers": {
    "terminaltiler": {
      "command": "/path/to/terminaltiler-mcp",
      "args": ["--project-root", "/path/to/project"]
    }
  }
}
```

Example Codex config shape:

```toml
[mcp_servers.terminaltiler]
command = "/path/to/terminaltiler-mcp"
args = ["--project-root", "/path/to/project"]
```

## Agent runs and reviews

Board-launched implementation runs spawn a live terminal pane running Claude or Codex in the project root. The prompt tells the agent to:

- research relevant docs, APIs, and code context
- record useful findings with `add_task_knowledge`
- claim the task before implementation
- post progress notes
- move the task to `in_review` when implementation is ready
- leave final completion as a manual board decision

Board-launched review runs use the same live-terminal flow, but the prompt asks for a concise severity-rated review note and tells the reviewer to leave the task in In Review.

When an MCP client moves a task to `in_review` through `update_task_status` or `ready_for_review`, TerminalTiler performs the status transition, ownership guard, lifecycle cleanup, handoff note, and duplicate-gated review metadata claim under one board lock. It then starts one headless reviewer and writes its log under:

```text
<project-root>/.terminaltiler/reviews/
```

The duplicate gate is stored in task review metadata. If the review process cannot be launched, TerminalTiler records `review.last_error`, appends a task note, and returns `review_error` in MCP `structuredContent`; manual UI review retries can still be requested.

## MCP server

The MCP server binary is `terminaltiler-mcp`. It speaks newline-delimited JSON-RPC over stdio and implements the MCP protocol version advertised by the binary at initialization.

Run it locally for protocol testing:

```bash
cargo run --bin terminaltiler-mcp -- --project-root /path/to/project
```

Without `--project-root`, the server falls back to the current working directory for older configs. New generated configs should always pass the explicit project root.

### Tools

The server exposes these tools:

- `get_board_summary`: compact counts plus `queues.available`, `queues.stale`, `queues.blocked`, and `queues.in_review` slices for mission-control triage. The legacy top-level `available` field remains.
- `get_my_work`: resume-focused owned work grouped as active, stale, paused, and in review.
- `list_tasks`: list tasks or filter by status, assignee, blocked state, or availability.
- `get_task` / `get_task_brief`: return full JSON or a concise Markdown task brief.
- `diagnose_mcp`: inspect project root, board file, MCP binary, Claude config, and Codex config without changing files.
- `create_task`: create a task, defaulting to To Do.
- `start_work` / `start_next_work`: lifecycle-aware claim helpers with soft-lease conflicts and stale/paused takeover warnings. Prefer these over legacy `claim_task`.
- `heartbeat_task`, `pause_work`, `release_task`, `reassign_task`, `block_task`, `unblock_task`: active-work lifecycle and blocker helpers.
- `ready_for_review`: append a handoff summary, move to In Review, clear active lifecycle metadata, and trigger the duplicate-gated review path.
- `update_task_status`: legacy-compatible status changes; moving to `in_review` uses the same review transition helper and may return `review_started` or `review_error`.
- `submit_review`: append a structured review verdict while leaving completion manual.
- `complete_task`: mark a task Complete, optionally with a closing note. Agents should use this only when explicitly instructed.
- `add_task_note`: append a progress note.
- `add_task_knowledge`: append a captured finding with a short title and detail.

All advertised tools include `title`, `inputSchema`, and `outputSchema`. Tool failures remain JSON-RPC successes with `isError: true` and structured conflict details where applicable; invalid resource or prompt requests return JSON-RPC `-32602` invalid-params errors.

### Resources and prompts

Resources:

- `terminaltiler://board/summary`
- `terminaltiler://board/tasks`
- `terminaltiler://workflow/guide`
- `terminaltiler://task/<task_id>.json`
- `terminaltiler://task/<task_id>.md`

Prompts:

- `implement_task`
- `work_next_task`
- `review_task`
- `triage_board`

Status wire IDs are:

```text
todo
in_progress
in_review
complete
cancelled
```

## Packaging notes

Release packages bundle the MCP server next to the desktop application:

- Linux `.deb` and AppImage artifacts include `terminaltiler-mcp`.
- Windows installer, portable `.exe`, portable zip, and MSI payloads include `terminaltiler-mcp.exe`.
- AppImage runs copy the MCP binary to a stable per-user data path before writing agent configs, because the AppImage mount path is temporary.

The release smoke tests check that the bundled MCP binary remains self-contained and does not link GTK/WebKit/VTE.

## Developer checks

Use the standard project gates for board changes:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Focused checks for board and MCP work:

```bash
cargo test board
cargo test mcp
cargo test agent_config
cargo test review_dispatch
```
