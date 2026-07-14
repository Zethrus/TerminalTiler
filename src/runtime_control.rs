//! Product-neutral live workspace control contracts.
//!
//! The existing MCP server is intentionally board-only and is backed by a
//! project JSON file.  Voice and future trusted automations need a separate,
//! revisioned control surface for the live GTK runtime.  This module contains
//! only typed contracts and deterministic policy helpers; Pro-specific
//! licensing, provider clients, and conversation logic stay outside Core.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub type WorkspaceId = String;
pub type TileId = String;
pub type ActionId = String;

pub const RUNTIME_SCHEMA_VERSION: u32 = 2;
pub const MAX_OUTPUT_LINES: usize = 40;
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024;
pub const EVENT_JOURNAL_CAPACITY: usize = 4_096;
const OUTPUT_COALESCE_WINDOW_MS: u128 = 250;
const MAX_EVENT_SUMMARY_BYTES: usize = 256;

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
    /// Event cursor for the state represented by this snapshot. Consumers can
    /// request events after this cursor to observe subsequent changes.
    #[serde(default)]
    pub latest_event_cursor: u64,
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
            workspace_revision: workspace_revision.max(1),
            latest_event_cursor: 0,
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
    TileMoved,
    LayoutChanged,
    ActionSubmitted,
    ActionInterrupted,
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
    #[serde(default)]
    pub oldest_cursor: u64,
    pub next_cursor: u64,
    #[serde(default)]
    pub latest_revision: u64,
    #[serde(default)]
    pub truncated: bool,
    pub events: Vec<WorkspaceEvent>,
}

/// Descriptive alias used by the v2 wire contract; retained alongside the
/// original `EventResponse` name for source compatibility.
pub type WorkspaceEventResponse = EventResponse;

/// Bounded in-memory event journal for one live workspace. The journal is
/// intentionally process-local: it contains only sanitized metadata and
/// never raw terminal output.
#[derive(Debug)]
pub struct WorkspaceEventJournal {
    events: VecDeque<WorkspaceEvent>,
    next_cursor: u64,
    last_output: HashMap<TileId, u128>,
}

impl Default for WorkspaceEventJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceEventJournal {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(EVENT_JOURNAL_CAPACITY),
            next_cursor: 1,
            last_output: HashMap::new(),
        }
    }

    pub fn latest_cursor(&self) -> u64 {
        self.next_cursor.saturating_sub(1)
    }

    pub fn oldest_cursor(&self) -> u64 {
        self.events
            .front()
            .map(|event| event.cursor)
            .unwrap_or(self.next_cursor)
    }

    pub fn append(
        &mut self,
        workspace_revision: u64,
        event_type: WorkspaceEventType,
        tile_id: Option<TileId>,
        safe_summary: impl AsRef<str>,
    ) -> Option<u64> {
        let timestamp = now_unix_ms();
        if event_type == WorkspaceEventType::OutputActivity {
            if let Some(tile) = tile_id.as_ref() {
                if let Some(previous_timestamp) = self.last_output.get(tile)
                    && timestamp.saturating_sub(*previous_timestamp) < OUTPUT_COALESCE_WINDOW_MS
                {
                    return None;
                }
                self.last_output.insert(tile.clone(), timestamp);
                if self.last_output.len() > EVENT_JOURNAL_CAPACITY {
                    self.last_output.clear();
                }
            }
        }

        let cursor = self.next_cursor;
        self.next_cursor = self.next_cursor.saturating_add(1);
        if self.events.len() >= EVENT_JOURNAL_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(WorkspaceEvent {
            cursor,
            workspace_revision,
            timestamp_unix_ms: timestamp,
            event_type,
            tile_id,
            safe_summary: cap_event_summary(&redact_event_summary(safe_summary.as_ref())),
        });
        Some(cursor)
    }

    pub fn response(
        &self,
        workspace_id: WorkspaceId,
        after_cursor: u64,
        limit: usize,
        latest_revision: u64,
    ) -> EventResponse {
        let oldest_cursor = self.oldest_cursor();
        let latest_cursor = self.latest_cursor();
        // A non-zero cursor older than the retained ring means the consumer
        // cannot reconstruct state and must refresh its snapshot.
        let truncated = after_cursor > 0 && after_cursor.saturating_add(1) < oldest_cursor;
        if truncated {
            return EventResponse {
                workspace_id,
                oldest_cursor,
                next_cursor: latest_cursor.saturating_add(1),
                latest_revision,
                truncated: true,
                events: Vec::new(),
            };
        }

        let limit = limit.clamp(1, EVENT_JOURNAL_CAPACITY);
        let events = self
            .events
            .iter()
            .filter(|event| event.cursor > after_cursor)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = events
            .last()
            .map(|event| event.cursor.saturating_add(1))
            .unwrap_or_else(|| after_cursor.saturating_add(1));
        EventResponse {
            workspace_id,
            oldest_cursor,
            next_cursor,
            latest_revision,
            truncated: false,
            events,
        }
    }
}

fn cap_event_summary(summary: &str) -> String {
    if summary.len() <= MAX_EVENT_SUMMARY_BYTES {
        return summary.to_string();
    }
    let mut end = MAX_EVENT_SUMMARY_BYTES.saturating_sub("…".len());
    while end > 0 && !summary.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &summary[..end])
}

fn redact_event_summary(summary: &str) -> String {
    let mut redacted = summary.to_string();
    for prefix in [
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "AWS_SECRET_ACCESS_KEY=",
        "TOKEN=",
        "PASSWORD=",
        "Authorization: Bearer ",
    ] {
        if let Some(index) = redacted.find(prefix) {
            let value_start = index + prefix.len();
            let value_end = redacted[value_start..]
                .find(char::is_whitespace)
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(value_start..value_end, "[REDACTED]");
        }
    }
    if redacted.contains("-----BEGIN ") {
        return "[REDACTED PRIVATE KEY]".into();
    }
    redacted
}

fn redact_command_preview(command: &str) -> String {
    let mut redacted = command.to_string();
    for prefix in [
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "AWS_SECRET_ACCESS_KEY=",
        "TOKEN=",
        "PASSWORD=",
    ] {
        if let Some(index) = redacted.find(prefix) {
            let value_start = index + prefix.len();
            let value_end = redacted[value_start..]
                .find(char::is_whitespace)
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(value_start..value_end, "[REDACTED]");
        }
    }
    if redacted.contains("-----BEGIN ") {
        return "[REDACTED PRIVATE KEY]".into();
    }
    redacted
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PreparedAction {
    pub action_id: ActionId,
    pub workspace_id: WorkspaceId,
    pub tile_id: TileId,
    /// SHA-256 digest used for auditing. This is not a confirmation secret.
    pub command_digest: String,
    /// Human-readable, redacted preview shown by the trusted local UI.
    pub redacted_preview: String,
    #[serde(skip)]
    command: String,
    pub risk: ActionRisk,
    #[serde(skip, default = "default_confirmation_requirement")]
    pub confirmation: ConfirmationRequirement,
    pub requires_confirmation: bool,
    pub expires_at_unix_ms: u128,
    pub prepared_revision: u64,
    /// Kept only in the desktop process.  A command hash is useful for
    /// display/auditing, but must never double as a confirmation secret: the
    /// caller already knows it at prepare time.
    #[serde(skip)]
    confirmation_nonce: String,
}

impl PreparedAction {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action_id: ActionId,
        workspace_id: WorkspaceId,
        tile_id: TileId,
        command_digest: String,
        display_command: String,
        risk: ActionRisk,
        confirmation: ConfirmationRequirement,
        expires_at_unix_ms: u128,
        prepared_revision: u64,
    ) -> Self {
        Self {
            action_id,
            workspace_id,
            tile_id,
            command_digest,
            redacted_preview: redact_command_preview(&display_command),
            command: display_command,
            risk,
            confirmation,
            requires_confirmation: !matches!(confirmation, ConfirmationRequirement::None),
            expires_at_unix_ms,
            prepared_revision,
            // UUID v4 is backed by the operating system CSPRNG.  This value
            // is intentionally omitted from the serialized prepare response.
            confirmation_nonce: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn confirmation_matches(&self, token: Option<&str>) -> bool {
        matches!(self.confirmation, ConfirmationRequirement::None)
            || token.is_some_and(|token| token == self.confirmation_nonce)
    }

    pub(crate) fn command(&self) -> &str {
        &self.command
    }

    /// Only desktop-owned UI code may reveal this value to a person.  It is
    /// deliberately crate-private so companion crates cannot turn preparing
    /// an action into automatic confirmation.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(crate) fn confirmation_nonce(&self) -> &str {
        &self.confirmation_nonce
    }
}

impl std::fmt::Debug for PreparedAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedAction")
            .field("action_id", &self.action_id)
            .field("workspace_id", &self.workspace_id)
            .field("tile_id", &self.tile_id)
            .field("command_digest", &self.command_digest)
            .field("redacted_preview", &self.redacted_preview)
            .field("risk", &self.risk)
            .field("confirmation", &self.confirmation)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("prepared_revision", &self.prepared_revision)
            .finish_non_exhaustive()
    }
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

fn default_confirmation_requirement() -> ConfirmationRequirement {
    ConfirmationRequirement::None
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_id: ActionId,
    /// Kept for compatibility with older in-process callers. It is ignored
    /// by provider-facing serialization; trusted UI approval is authoritative.
    #[serde(skip)]
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
    lifecycle: Arc<AtomicU8>,
}

const REQUEST_PENDING: u8 = 0;
const REQUEST_DISPATCHING: u8 = 1;
const REQUEST_CANCELLED: u8 = 2;

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
            // A caller that timed out cancels while the request is still
            // pending.  Never dispatch a cancelled operation later: this is
            // especially important for terminal execution requests.
            if request
                .lifecycle
                .compare_exchange(
                    REQUEST_PENDING,
                    REQUEST_DISPATCHING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                continue;
            }
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
        self.call_with_timeout(operation, Duration::from_secs(5))
    }

    fn call_with_timeout(
        &self,
        operation: RuntimeOperation,
        timeout: Duration,
    ) -> Result<Value, RuntimeControlError> {
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        let lifecycle = Arc::new(AtomicU8::new(REQUEST_PENDING));
        self.sender
            .send(QueuedRequest {
                operation,
                response: response_tx,
                lifecycle: lifecycle.clone(),
            })
            .map_err(|_| {
                RuntimeControlError::Internal("workspace control is unavailable".into())
            })?;
        match response_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if lifecycle
                    .compare_exchange(
                        REQUEST_PENDING,
                        REQUEST_CANCELLED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Err(RuntimeControlError::Internal(
                        "workspace control timed out".into(),
                    ))
                } else {
                    // The GTK thread acquired this request just before the
                    // deadline.  Wait for its authoritative result rather
                    // than telling the caller it failed and executing it
                    // later in the background.
                    response_rx.recv().map_err(|_| {
                        RuntimeControlError::Internal("workspace control is unavailable".into())
                    })?
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(RuntimeControlError::Internal(
                "workspace control is unavailable".into(),
            )),
        }
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
                if !self.authorizer.allows_mutation() {
                    return Err(RuntimeControlError::Unauthorized);
                }
                let request: FocusTileRequest = serde_json::from_value(arguments)
                    .map_err(|error| RuntimeControlError::InvalidRequest(error.to_string()))?;
                result_value(self.control.focus_tile(request))
            }
            "create_terminal_tile" => {
                if !self.authorizer.allows_mutation() {
                    return Err(RuntimeControlError::Unauthorized);
                }
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
        "rm ",
        "rmdir ",
        "git reset --hard",
        "git clean",
        "wipefs",
        "format ",
    ]
    .iter()
    .any(|pattern| normalized.starts_with(pattern))
    {
        return ActionRisk::Destructive;
    }
    if [
        "ls",
        "pwd",
        "whoami",
        "id",
        "git status",
        "git log",
        "git diff",
        "git show",
        "docker ps",
        "docker images",
        "cargo metadata",
        "cargo tree",
    ]
    .iter()
    .any(|command| normalized == *command || normalized.starts_with(&format!("{command} ")))
    {
        return ActionRisk::ReadOnly;
    }
    // The shell surface is open-ended.  Treat every command that is not on
    // the small, argument-safe read-only allowlist as mutating so it requires
    // user confirmation rather than silently expanding a dangerous allowlist.
    ActionRisk::Mutating
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
        text = truncate_utf8(&text, max_bytes);
    }
    OutputSnapshot {
        line_count: text.lines().count(),
        text,
        truncated,
        redacted,
    }
}

fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let suffix = "…";
    let mut end = max_bytes.saturating_sub(suffix.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &input[..end], suffix)
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    struct ReadOnlyAuthorizer;

    impl RuntimeCapabilityAuthorizer for ReadOnlyAuthorizer {
        fn allows_runtime_session(&self) -> bool {
            true
        }

        fn allows_mutation(&self) -> bool {
            false
        }
    }

    struct CountingControl {
        calls: AtomicUsize,
    }

    impl CountingControl {
        fn called(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }

        fn record(&self) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl WorkspaceControlPort for CountingControl {
        fn workspace_snapshot(
            &self,
            _request: SnapshotRequest,
        ) -> Result<WorkspaceSnapshot, RuntimeControlError> {
            self.record();
            Ok(WorkspaceSnapshot::new("workspace:test".into(), 0))
        }

        fn workspace_events(
            &self,
            _request: EventRequest,
        ) -> Result<EventResponse, RuntimeControlError> {
            self.record();
            Ok(EventResponse {
                workspace_id: "workspace:test".into(),
                oldest_cursor: 1,
                next_cursor: 0,
                latest_revision: 1,
                truncated: false,
                events: Vec::new(),
            })
        }

        fn focus_tile(
            &self,
            _request: FocusTileRequest,
        ) -> Result<ActionResult, RuntimeControlError> {
            self.record();
            Ok(ActionResult {
                workspace_revision: 0,
                message: String::new(),
            })
        }

        fn create_terminal_tile(
            &self,
            _request: CreateTerminalTileRequest,
        ) -> Result<ActionResult, RuntimeControlError> {
            self.record();
            Ok(ActionResult {
                workspace_revision: 0,
                message: String::new(),
            })
        }

        fn prepare_terminal_action(
            &self,
            _request: PrepareActionRequest,
        ) -> Result<PreparedAction, RuntimeControlError> {
            self.record();
            Err(RuntimeControlError::Internal(
                "not used by this test".into(),
            ))
        }

        fn execute_terminal_action(
            &self,
            _request: ExecuteActionRequest,
        ) -> Result<ActionResult, RuntimeControlError> {
            self.record();
            Ok(ActionResult {
                workspace_revision: 0,
                message: String::new(),
            })
        }

        fn interrupt_tile(
            &self,
            _request: InterruptTileRequest,
        ) -> Result<ActionResult, RuntimeControlError> {
            self.record();
            Ok(ActionResult {
                workspace_revision: 0,
                message: String::new(),
            })
        }
    }

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
        assert_eq!(
            classify_command("curl -o output https://x"),
            ActionRisk::Mutating
        );
        assert_eq!(
            classify_command("rm important-file"),
            ActionRisk::Destructive
        );
    }

    #[test]
    fn tile_focus_and_creation_require_mutation_capability() {
        let control = Arc::new(CountingControl {
            calls: AtomicUsize::new(0),
        });
        let service = RuntimeMcpService::new(control.clone(), Arc::new(ReadOnlyAuthorizer));

        let focus = service.call(
            "focus_tile",
            json!({ "workspace_id": "workspace:test", "tile_id": "tile-1" }),
        );
        let create = service.call(
            "create_terminal_tile",
            json!({
                "workspace_id": "workspace:test",
                "axis": "horizontal"
            }),
        );

        assert!(matches!(focus, Err(RuntimeControlError::Unauthorized)));
        assert!(matches!(create, Err(RuntimeControlError::Unauthorized)));
        assert_eq!(control.called(), 0);
    }

    #[test]
    fn confirmation_nonce_is_not_the_returned_command_hash() {
        let action = PreparedAction::new(
            "action-1".into(),
            "workspace:test".into(),
            "tile-1".into(),
            "known-command-hash".into(),
            "git pull".into(),
            ActionRisk::Mutating,
            ConfirmationRequirement::ExactAction,
            now_unix_ms() + 30_000,
            7,
        );

        let serialized = serde_json::to_value(&action).unwrap();
        assert!(serialized.get("confirmation_nonce").is_none());
        assert_eq!(
            serialized.get("command_digest").and_then(Value::as_str),
            Some("known-command-hash")
        );
        assert_eq!(
            serialized.get("prepared_revision").and_then(Value::as_u64),
            Some(7)
        );
        assert_eq!(
            serialized
                .get("requires_confirmation")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(serialized.get("command").is_none());
        assert!(!action.confirmation_matches(Some("known-command-hash")));
        assert!(!action.confirmation_matches(None));
    }

    #[test]
    fn timed_out_queue_requests_are_cancelled_before_dispatch() {
        let (queue, _) = WorkspaceControlQueue::new();
        let control = QueuedWorkspaceControl {
            sender: queue.sender.clone(),
        };
        let result = thread::spawn(move || {
            control.call_with_timeout(
                RuntimeOperation::Snapshot(SnapshotRequest::default()),
                Duration::from_millis(10),
            )
        })
        .join()
        .unwrap();

        assert!(
            matches!(result, Err(RuntimeControlError::Internal(message)) if message == "workspace control timed out")
        );
        let handled = Arc::new(AtomicUsize::new(0));
        let handled_for_drain = handled.clone();
        queue.drain(1, move |_| {
            handled_for_drain.fetch_add(1, Ordering::Relaxed);
            Ok(json!({}))
        });
        assert_eq!(handled.load(Ordering::Relaxed), 0);
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
    fn output_byte_bounds_do_not_split_unicode() {
        let output = sanitize_output("🙂🙂🙂", 3, 5);
        assert!(output.truncated);
        assert!(output.text.is_char_boundary(output.text.len()));
        assert!(output.text.len() <= 5);
    }

    #[test]
    fn snapshot_defaults_to_metadata_only() {
        assert_eq!(
            SnapshotRequest::default().output_policy,
            OutputPolicy::MetadataOnly
        );
        assert_eq!(RUNTIME_SCHEMA_VERSION, 2);
        assert_eq!(
            WorkspaceSnapshot::new("workspace:test".into(), 0).workspace_revision,
            1
        );
    }

    #[test]
    fn event_journal_cursors_and_truncation_are_deterministic() {
        let mut journal = WorkspaceEventJournal::new();
        let first = journal
            .append(
                1,
                WorkspaceEventType::FocusChanged,
                Some("tile-1".into()),
                "focused",
            )
            .unwrap();
        assert_eq!(first, 1);
        let response = journal.response("workspace:test".into(), 0, 100, 1);
        assert_eq!(response.oldest_cursor, 1);
        assert_eq!(response.next_cursor, 2);
        assert!(!response.truncated);
        assert_eq!(response.events.len(), 1);

        for index in 0..(EVENT_JOURNAL_CAPACITY + 2) {
            journal.append(
                index as u64 + 2,
                WorkspaceEventType::ActionCompleted,
                None,
                "done",
            );
        }
        assert!(journal.oldest_cursor() > 1);
        let stale = journal.response("workspace:test".into(), 1, 100, 2);
        assert!(stale.truncated);
        assert!(stale.events.is_empty());
    }

    #[test]
    fn output_events_are_coalesced_and_summaries_are_bounded() {
        let mut journal = WorkspaceEventJournal::new();
        let summary = "x".repeat(MAX_EVENT_SUMMARY_BYTES + 20);
        let first = journal.append(
            1,
            WorkspaceEventType::OutputActivity,
            Some("tile-1".into()),
            summary,
        );
        let second = journal.append(
            1,
            WorkspaceEventType::OutputActivity,
            Some("tile-1".into()),
            "more",
        );
        assert!(first.is_some());
        assert!(second.is_none());
        let response = journal.response("workspace:test".into(), 0, 100, 1);
        assert_eq!(response.events.len(), 1);
        assert!(response.events[0].safe_summary.len() <= MAX_EVENT_SUMMARY_BYTES);
    }
}
