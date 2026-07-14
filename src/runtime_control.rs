//! Product-neutral live workspace control contracts.
//!
//! The existing MCP server is intentionally board-only and is backed by a
//! project JSON file.  Voice and future trusted automations need a separate,
//! revisioned control surface for the live GTK runtime.  This module contains
//! only typed contracts and deterministic policy helpers; Pro-specific
//! licensing, provider clients, and conversation logic stay outside Core.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub type WorkspaceId = String;
pub type TileId = String;
pub type ActionId = String;

pub const RUNTIME_SCHEMA_VERSION: u32 = 1;
pub const MAX_OUTPUT_LINES: usize = 40;
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputPolicy {
    MetadataOnly,
    Sanitized,
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self::MetadataOnly
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub output_policy: OutputPolicy,
    #[serde(default = "default_max_lines")]
    pub max_lines: usize,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: usize,
}

impl Default for SnapshotRequest {
    fn default() -> Self {
        Self {
            workspace_id: None,
            output_policy: OutputPolicy::MetadataOnly,
            max_lines: MAX_OUTPUT_LINES,
            max_bytes: MAX_OUTPUT_BYTES,
        }
    }
}

fn default_max_lines() -> usize {
    MAX_OUTPUT_LINES
}

fn default_max_bytes() -> usize {
    MAX_OUTPUT_BYTES
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub schema_version: u32,
    pub workspace_id: WorkspaceId,
    pub workspace_revision: u64,
    pub generated_at_unix_ms: u128,
    pub focused_tile_id: Option<TileId>,
    pub layout: LayoutSnapshot,
    pub tiles: Vec<TileSnapshot>,
    #[serde(default)]
    pub active_agents: Vec<AgentSnapshot>,
}

impl WorkspaceSnapshot {
    pub fn new(workspace_id: WorkspaceId, workspace_revision: u64) -> Self {
        Self {
            schema_version: RUNTIME_SCHEMA_VERSION,
            workspace_id,
            workspace_revision,
            generated_at_unix_ms: now_unix_ms(),
            focused_tile_id: None,
            layout: LayoutSnapshot::default(),
            tiles: Vec::new(),
            active_agents: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LayoutSnapshot {
    /// Tile ids in visual reading order (top-to-bottom, then left-to-right).
    pub visual_order: Vec<TileId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TileSnapshot {
    pub tile_id: TileId,
    pub visual_ordinal: usize,
    pub title: String,
    pub agent_label: Option<String>,
    pub kind: TileKind,
    pub focused: bool,
    pub working_directory: Option<String>,
    pub process: ProcessSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_output: Option<OutputSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileKind {
    Terminal,
    Web,
    Unknown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub state: ProcessState,
    pub child_pid: Option<u32>,
    pub foreground_command: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    #[default]
    Unknown,
    Idle,
    Running,
    Exited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputSnapshot {
    pub text: String,
    pub line_count: usize,
    pub truncated: bool,
    pub redacted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub agent_run_id: String,
    pub provider: String,
    pub status: String,
    pub tile_id: Option<TileId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceEvent {
    pub cursor: u64,
    pub workspace_revision: u64,
    pub timestamp_unix_ms: u128,
    pub event_type: WorkspaceEventType,
    pub tile_id: Option<TileId>,
    pub safe_summary: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventType {
    FocusChanged,
    TileCreated,
    TileRemoved,
    ManualInput,
    ProcessStarted,
    ProcessExited,
    OutputActivity,
    AgentChanged,
    ActionCompleted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub after_cursor: u64,
    #[serde(default = "default_event_limit")]
    pub limit: usize,
}

fn default_event_limit() -> usize {
    100
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventResponse {
    pub workspace_id: WorkspaceId,
    pub next_cursor: u64,
    pub events: Vec<WorkspaceEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FocusTileRequest {
    pub workspace_id: WorkspaceId,
    pub tile_id: TileId,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateTerminalTileRequest {
    pub workspace_id: WorkspaceId,
    pub split_target: Option<TileId>,
    pub axis: SplitAxis,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrepareActionRequest {
    pub workspace_id: WorkspaceId,
    pub tile_id: TileId,
    pub command: String,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PreparedAction {
    pub action_id: ActionId,
    pub workspace_id: WorkspaceId,
    pub tile_id: TileId,
    pub command_hash: String,
    pub display_command: String,
    pub risk: ActionRisk,
    pub confirmation: ConfirmationRequirement,
    pub expires_at_unix_ms: u128,
    pub workspace_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRisk {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationRequirement {
    None,
    ExactAction,
    SpokenNonce,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_id: ActionId,
    pub confirmation_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InterruptTileRequest {
    pub workspace_id: WorkspaceId,
    pub tile_id: TileId,
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub workspace_revision: u64,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeControlError {
    Unauthorized,
    InvalidRequest(String),
    NotFound(String),
    RevisionConflict { expected: u64, actual: u64 },
    ExpiredAction,
    ConfirmationRequired,
    Internal(String),
}

impl std::fmt::Display for RuntimeControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "runtime capability is not authorized"),
            Self::InvalidRequest(message) => write!(f, "invalid runtime request: {message}"),
            Self::NotFound(message) => write!(f, "runtime target not found: {message}"),
            Self::RevisionConflict { expected, actual } => {
                write!(
                    f,
                    "workspace changed (expected revision {expected}, actual {actual})"
                )
            }
            Self::ExpiredAction => write!(f, "prepared action expired"),
            Self::ConfirmationRequired => write!(f, "confirmation is required"),
            Self::Internal(message) => write!(f, "runtime control failed: {message}"),
        }
    }
}

impl std::error::Error for RuntimeControlError {}

/// Main-thread-safe facade implemented by the desktop host.
pub trait WorkspaceControlPort: Send + Sync {
    fn workspace_snapshot(
        &self,
        request: SnapshotRequest,
    ) -> Result<WorkspaceSnapshot, RuntimeControlError>;

    fn workspace_events(&self, request: EventRequest)
    -> Result<EventResponse, RuntimeControlError>;

    fn focus_tile(&self, request: FocusTileRequest) -> Result<ActionResult, RuntimeControlError>;

    fn create_terminal_tile(
        &self,
        request: CreateTerminalTileRequest,
    ) -> Result<ActionResult, RuntimeControlError>;

    fn prepare_terminal_action(
        &self,
        request: PrepareActionRequest,
    ) -> Result<PreparedAction, RuntimeControlError>;

    fn execute_terminal_action(
        &self,
        request: ExecuteActionRequest,
    ) -> Result<ActionResult, RuntimeControlError>;

    fn interrupt_tile(
        &self,
        request: InterruptTileRequest,
    ) -> Result<ActionResult, RuntimeControlError>;
}

#[derive(Debug)]
pub enum RuntimeOperation {
    Snapshot(SnapshotRequest),
    Events(EventRequest),
    Focus(FocusTileRequest),
    Create(CreateTerminalTileRequest),
    Prepare(PrepareActionRequest),
    Execute(ExecuteActionRequest),
    Interrupt(InterruptTileRequest),
}

struct QueuedRequest {
    operation: RuntimeOperation,
    response: mpsc::SyncSender<Result<Value, RuntimeControlError>>,
}

/// A bounded cross-thread request queue. The desktop drains this queue from
/// the GTK main loop and executes operations against `WorkspaceRuntime`, while
/// the Pro companion can safely call the `WorkspaceControlPort` from an audio
/// or provider worker thread.
pub struct WorkspaceControlQueue {
    sender: mpsc::SyncSender<QueuedRequest>,
    receiver: Mutex<mpsc::Receiver<QueuedRequest>>,
}

impl WorkspaceControlQueue {
    pub fn new() -> (Arc<Self>, Arc<dyn WorkspaceControlPort>) {
        let (sender, receiver) = mpsc::sync_channel(64);
        let queue = Arc::new(Self {
            sender,
            receiver: Mutex::new(receiver),
        });
        let port: Arc<dyn WorkspaceControlPort> = Arc::new(QueuedWorkspaceControl {
            sender: queue.sender.clone(),
        });
        (queue, port)
    }

    /// Drain at most `limit` requests from the main loop. The handler must be
    /// short; long-running commands are represented by action preparation and
    /// are handled asynchronously by the terminal runtime.
    pub fn drain<F>(&self, limit: usize, mut handler: F) -> usize
    where
        F: FnMut(RuntimeOperation) -> Result<Value, RuntimeControlError>,
    {
        let Ok(receiver) = self.receiver.lock() else {
            return 0;
        };
        let mut drained = 0;
        while drained < limit {
            let Ok(request) = receiver.try_recv() else {
                break;
            };
            let result = handler(request.operation);
            let _ = request.response.send(result);
            drained += 1;
        }
        drained
    }
}

struct QueuedWorkspaceControl {
    sender: mpsc::SyncSender<QueuedRequest>,
}

impl QueuedWorkspaceControl {
    fn call(&self, operation: RuntimeOperation) -> Result<Value, RuntimeControlError> {
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        self.sender
            .send(QueuedRequest {
                operation,
                response: response_tx,
            })
            .map_err(|_| {
                RuntimeControlError::Internal("workspace control is unavailable".into())
            })?;
        response_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| RuntimeControlError::Internal("workspace control timed out".into()))?
    }
}

impl WorkspaceControlPort for QueuedWorkspaceControl {
    fn workspace_snapshot(
        &self,
        request: SnapshotRequest,
    ) -> Result<WorkspaceSnapshot, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Snapshot(request))?)
    }

    fn workspace_events(
        &self,
        request: EventRequest,
    ) -> Result<EventResponse, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Events(request))?)
    }

    fn focus_tile(&self, request: FocusTileRequest) -> Result<ActionResult, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Focus(request))?)
    }

    fn create_terminal_tile(
        &self,
        request: CreateTerminalTileRequest,
    ) -> Result<ActionResult, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Create(request))?)
    }

    fn prepare_terminal_action(
        &self,
        request: PrepareActionRequest,
    ) -> Result<PreparedAction, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Prepare(request))?)
    }

    fn execute_terminal_action(
        &self,
        request: ExecuteActionRequest,
    ) -> Result<ActionResult, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Execute(request))?)
    }

    fn interrupt_tile(
        &self,
        request: InterruptTileRequest,
    ) -> Result<ActionResult, RuntimeControlError> {
        decode_value(self.call(RuntimeOperation::Interrupt(request))?)
    }
}

fn decode_value<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, RuntimeControlError> {
    serde_json::from_value(value).map_err(|error| {
        RuntimeControlError::Internal(format!("invalid control response: {error}"))
    })
}

/// The paid companion supplies this authorizer. Core never infers Pro status
/// from local preferences or a cached provider token.
pub trait RuntimeCapabilityAuthorizer: Send + Sync {
    fn allows_runtime_session(&self) -> bool;
    fn allows_mutation(&self) -> bool;
}

/// A transport-neutral runtime MCP dispatcher. The in-process Pro client and
/// any future local broker use the same operation names and validation.
pub struct RuntimeMcpService {
    control: Arc<dyn WorkspaceControlPort>,
    authorizer: Arc<dyn RuntimeCapabilityAuthorizer>,
}

impl RuntimeMcpService {
    pub fn new(
        control: Arc<dyn WorkspaceControlPort>,
        authorizer: Arc<dyn RuntimeCapabilityAuthorizer>,
    ) -> Self {
        Self {
            control,
            authorizer,
        }
    }

    pub fn list_tools() -> Value {
        json!([
            "list_runtime_workspaces",
            "get_workspace_snapshot",
            "get_workspace_events",
            "focus_tile",
            "create_terminal_tile",
            "prepare_terminal_action",
            "execute_terminal_action",
            "interrupt_tile"
        ])
    }

    pub fn call(&self, name: &str, arguments: Value) -> Result<Value, RuntimeControlError> {
        if !self.authorizer.allows_runtime_session() {
            return Err(RuntimeControlError::Unauthorized);
        }
        match name {
            "get_workspace_snapshot" => {
                let request: SnapshotRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.workspace_snapshot(request))
            }
            "get_workspace_events" => {
                let request: EventRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.workspace_events(request))
            }
            "focus_tile" => {
                let request: FocusTileRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.focus_tile(request))
            }
            "create_terminal_tile" => {
                let request: CreateTerminalTileRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.create_terminal_tile(request))
            }
            "prepare_terminal_action" => {
                let request: PrepareActionRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.prepare_terminal_action(request))
            }
            "execute_terminal_action" => {
                if !self.authorizer.allows_mutation() {
                    return Err(RuntimeControlError::Unauthorized);
                }
                let request: ExecuteActionRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.execute_terminal_action(request))
            }
            "interrupt_tile" => {
                if !self.authorizer.allows_mutation() {
                    return Err(RuntimeControlError::Unauthorized);
                }
                let request: InterruptTileRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.interrupt_tile(request))
            }
            "list_runtime_workspaces" => {
                let request = SnapshotRequest::default();
                let snapshot = self.control.workspace_snapshot(request)?;
                Ok(json!([snapshot]))
            }
            _ => Err(RuntimeControlError::NotFound(format!(
                "unknown runtime tool '{name}'"
            ))),
        }
    }
}

fn result_value<T: Serialize>(
    result: Result<T, RuntimeControlError>,
) -> Result<Value, RuntimeControlError> {
    match result {
        Ok(value) => serde_json::to_value(value)
            .map_err(|error| RuntimeControlError::Internal(error.to_string())),
        Err(error) => Err(error),
    }
}

pub fn classify_command(command: &str) -> ActionRisk {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return ActionRisk::Destructive;
    }
    if contains_shell_metacharacter(&normalized)
        || [
            "rm -rf",
            "sudo",
            "shutdown",
            "reboot",
            "mkfs",
            "dd if=",
            "git push --force",
        ]
        .iter()
        .any(|pattern| normalized.contains(pattern))
    {
        return ActionRisk::Destructive;
    }
    if [
        "git pull",
        "git checkout",
        "git commit",
        "npm install",
        "cargo test",
        "cargo build",
        "make",
        "docker run",
        "docker compose",
    ]
    .iter()
    .any(|pattern| normalized.starts_with(pattern))
    {
        return ActionRisk::Mutating;
    }
    ActionRisk::ReadOnly
}

fn contains_shell_metacharacter(command: &str) -> bool {
    command
        .chars()
        .any(|character| matches!(character, ';' | '|' | '&' | '>' | '<' | '`' | '$'))
}

pub fn confirmation_for(risk: ActionRisk) -> ConfirmationRequirement {
    match risk {
        ActionRisk::ReadOnly => ConfirmationRequirement::None,
        ActionRisk::Mutating => ConfirmationRequirement::ExactAction,
        ActionRisk::Destructive => ConfirmationRequirement::SpokenNonce,
    }
}

/// Redacts common credential-shaped values and bounds output before it can be
/// included in a provider prompt. This is deliberately conservative: false
/// positives are preferable to sending secrets to a third party.
pub fn sanitize_output(input: &str, max_lines: usize, max_bytes: usize) -> OutputSnapshot {
    let max_lines = max_lines.clamp(1, MAX_OUTPUT_LINES);
    let max_bytes = max_bytes.clamp(1, MAX_OUTPUT_BYTES);
    let mut redacted = false;
    let input_lines = input.lines().collect::<Vec<_>>();
    let start = input_lines.len().saturating_sub(max_lines);
    let mut lines = Vec::new();
    for line in input_lines.into_iter().skip(start) {
        let mut sanitized = line.to_string();
        for prefix in [
            "OPENAI_API_KEY=",
            "ANTHROPIC_API_KEY=",
            "AWS_SECRET_ACCESS_KEY=",
            "TOKEN=",
        ] {
            if let Some(index) = sanitized.find(prefix) {
                sanitized.truncate(index + prefix.len());
                sanitized.push_str("[REDACTED]");
                redacted = true;
            }
        }
        if sanitized.contains("-----BEGIN ") {
            sanitized = "[REDACTED PRIVATE KEY]".to_string();
            redacted = true;
        }
        lines.push(sanitized);
    }
    let mut text = lines.join("\n");
    let truncated = text.len() > max_bytes || input.lines().count() > max_lines;
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("…");
    }
    OutputSnapshot {
        line_count: text.lines().count(),
        text,
        truncated,
        redacted,
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_metacharacters_fail_closed() {
        assert_eq!(
            classify_command("echo ok; rm -rf /"),
            ActionRisk::Destructive
        );
        assert_eq!(
            classify_command("cat output | less"),
            ActionRisk::Destructive
        );
    }

    #[test]
    fn ordinary_read_only_and_mutating_commands_are_distinguished() {
        assert_eq!(classify_command("docker ps"), ActionRisk::ReadOnly);
        assert_eq!(classify_command("git pull"), ActionRisk::Mutating);
    }

    #[test]
    fn output_is_bounded_and_redacts_credentials() {
        let output = sanitize_output(
            "first\nOPENAI_API_KEY=secret\n-----BEGIN PRIVATE KEY-----\nlast",
            3,
            80,
        );
        assert!(output.redacted);
        assert!(output.text.contains("[REDACTED]"));
        assert!(!output.text.contains("secret"));
    }

    #[test]
    fn snapshot_defaults_to_metadata_only() {
        assert_eq!(
            SnapshotRequest::default().output_policy,
            OutputPolicy::MetadataOnly
        );
        assert_eq!(RUNTIME_SCHEMA_VERSION, 1);
    }
}
