# Live workspace runtime control

`src/runtime_control.rs` is the product-neutral control boundary for trusted
companions and future local automations. It is deliberately separate from the
board MCP server (`src/mcp/`): board tools operate on persisted project data,
while runtime tools operate on the currently focused GTK workspace.

## Boundary

```text
provider / Pro worker thread
          |
          v
RuntimeMcpService -- capability authorizer --> WorkspaceControlPort
                                                    |
                                                    v
                                      bounded WorkspaceControlQueue
                                                    |
                                      GTK main-loop dispatcher
                                                    |
                                                    v
                                      focused WorkspaceView + tiles
```

The queue is bounded and has a five-second response timeout. Core never
decides whether a caller is paid: the companion supplies
`RuntimeCapabilityAuthorizer`, and Core fails closed when it is absent or
denies a request.

## Tool contract (schema version 1)

| Tool | Purpose | Mutation / safety |
| --- | --- | --- |
| `list_runtime_workspaces` | Return the focused workspace snapshot | Read-only |
| `get_workspace_snapshot` | Return tile layout, focus, process metadata, and optionally bounded sanitized output | Read-only; output is metadata-only by default |
| `get_workspace_events` | Read event cursor for incremental clients | Read-only; event journal is currently an empty compatibility response |
| `focus_tile` | Focus a terminal tile | Mutation, revision checked |
| `create_terminal_tile` | Add and focus a terminal tile | Mutation, revision checked |
| `prepare_terminal_action` | Validate and classify a command, returning a short-lived action | No shell execution |
| `execute_terminal_action` | Submit a prepared command to a tile | Mutation; Pro authorizer plus confirmation token required |
| `interrupt_tile` | Send Ctrl-C to a tile | Mutation; revision checked |

Commands are bounded to 4 KiB and classified as read-only, mutating, or
destructive. Shell metacharacters and high-risk command patterns fail closed.
Terminal output is capped at 40 lines/8 KiB and common credential/private-key
markers are redacted before a provider prompt can receive it.

## Compatibility

`CORE_EXTENSION_API_VERSION` is 2. API-1 companions continue to work because
the new `CompanionIntegration` methods have no-op defaults. The Pro companion
must publish against a Core revision that includes this module and must provide
an authorizer before attaching `RuntimeMcpService`.

