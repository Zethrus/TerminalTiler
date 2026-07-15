use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use vte4::prelude::*;

use crate::dropped_paths;
use crate::logging;
use crate::model::assets::WorkspaceAssets;
use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;
use crate::services::launch_resolution::{ResolvedLaunchTransport, resolve_tile_launch};
use crate::services::stats::StatsRecorder;
use crate::services::terminal_history::{
    normalize_terminal_history_lines, restored_terminal_history_text,
};
use crate::terminal_palette::terminal_palette;
use crate::transcript::TranscriptBuffer;

const DEFAULT_TERMINAL_COPY_SHORTCUT: &str = "<Ctrl><Shift>C";
const DEFAULT_TERMINAL_PASTE_SHORTCUT: &str = "<Ctrl><Shift>V";
const LIVE_TERMINAL_SCROLLBACK_LINES: i64 = 20_000;
const MIN_TERMINAL_FONT_POINTS: i32 = 7;
const MAX_TERMINAL_FONT_POINTS: i32 = 20;
#[derive(Clone)]
pub struct TerminalSession {
    terminal: vte4::Terminal,
    state: Rc<RefCell<TerminalSessionState>>,
    descriptor: Rc<str>,
    launch_spec: Rc<TerminalLaunchSpec>,
    transcript: Rc<RefCell<TranscriptBuffer>>,
}

#[derive(Default)]
struct TerminalSessionState {
    child_pid: Option<libc::pid_t>,
    exited: bool,
    termination_requested: bool,
    last_exit_status: Option<i32>,
    auto_reconnect_attempts: u8,
    auto_reconnect_pending: bool,
    kill_timeout: Option<glib::SourceId>,
    immediate_termination_requested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalLaunchMode {
    ConfiguredSession,
    LocalShell,
}

impl TerminalSessionState {
    fn clear_kill_timeout(&mut self) {
        if let Some(source_id) = self.kill_timeout.take() {
            source_id.remove();
        }
    }
}

struct TerminalLaunchSpec {
    working_directory: String,
    configured_argv: Vec<String>,
    local_shell_argv: Vec<String>,
    envv: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum WorkingDirectoryValidationError {
    Missing(PathBuf),
    NotDirectory(PathBuf),
}

impl fmt::Display for WorkingDirectoryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(
                formatter,
                "The working directory does not exist:\n{}",
                path.display()
            ),
            Self::NotDirectory(path) => write!(
                formatter,
                "The working directory is not a directory:\n{}",
                path.display()
            ),
        }
    }
}

impl TerminalLaunchSpec {
    fn argv_for_mode(&self, mode: TerminalLaunchMode) -> &[String] {
        match mode {
            TerminalLaunchMode::ConfiguredSession => &self.configured_argv,
            TerminalLaunchMode::LocalShell => &self.local_shell_argv,
        }
    }

    fn supports_recovery_options(&self) -> bool {
        supports_recovery_options(&self.configured_argv, &self.local_shell_argv)
    }
}

impl TerminalSession {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        tile: &TileSpec,
        workspace_root: &Path,
        assets: &WorkspaceAssets,
        use_dark_palette: bool,
        density: ApplicationDensity,
        zoom_steps: i32,
        restored_history_lines: &[String],
        restore_startup_command: Option<&str>,
        stats: StatsRecorder,
        configured_argv_override: Option<Vec<String>>,
    ) -> Self {
        let terminal = vte4::Terminal::new();
        terminal.set_hexpand(true);
        terminal.set_vexpand(true);
        terminal.set_size_request(0, 0);
        terminal.set_overflow(gtk::Overflow::Hidden);
        terminal.set_scrollback_lines(LIVE_TERMINAL_SCROLLBACK_LINES);
        terminal.set_mouse_autohide(true);
        terminal.set_clear_background(false);
        terminal.set_cursor_blink_mode(vte4::CursorBlinkMode::System);
        install_terminal_shortcuts(&terminal);
        apply_terminal_appearance(&terminal, use_dark_palette, density, zoom_steps);

        let working_dir = tile.working_directory.resolve(workspace_root);
        let state = Rc::new(RefCell::new(TerminalSessionState::default()));
        let transcript = Rc::new(RefCell::new(TranscriptBuffer::default()));
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let descriptor: Rc<str> = format!(
            "tile='{}' agent='{}' dir='{}'",
            tile.title,
            tile.agent_label,
            working_dir.display()
        )
        .into();

        {
            let state = state.clone();
            let descriptor = descriptor.clone();
            terminal.connect_child_exited(move |_, status| {
                let mut state = state.borrow_mut();
                state.exited = true;
                state.child_pid = None;
                state.last_exit_status = Some(status);
                state.immediate_termination_requested = false;
                state.clear_kill_timeout();
                logging::info(format!(
                    "terminal child exited status={} {}",
                    status, descriptor
                ));
            });
        }
        install_terminal_input_stats_hook(&terminal, state.clone(), stats.clone());

        let (launch_spec, initial_configured_argv) = if let Err(error) =
            validate_working_dir(&working_dir)
        {
            let error = error.to_string();
            report_spawn_problem(&terminal, &descriptor, &error);
            mark_state_exited(&state);
            (
                Rc::new(TerminalLaunchSpec {
                    working_directory: working_dir.display().to_string(),
                    configured_argv: Vec::new(),
                    local_shell_argv: build_local_shell_argv(&shell),
                    envv: vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into()],
                }),
                None,
            )
        } else if let Some(configured_argv) = configured_argv_override {
            (
                Rc::new(TerminalLaunchSpec {
                    working_directory: working_dir.display().to_string(),
                    configured_argv,
                    local_shell_argv: build_local_shell_argv(&shell),
                    envv: vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into()],
                }),
                None,
            )
        } else {
            match resolve_tile_launch(tile, workspace_root, assets) {
                Ok(resolved_launch) => {
                    let launch_shell = shell_for_launch(&shell, &resolved_launch.transport);
                    let initial_configured_argv =
                        restore_startup_command.and_then(|restore_startup_command| {
                            let mut launch_tile = tile.clone();
                            launch_tile.startup_command = Some(restore_startup_command.to_string());
                            match resolve_tile_launch(&launch_tile, workspace_root, assets) {
                                Ok(restore_launch) => Some(build_spawn_argv(
                                    &shell_for_launch(&shell, &restore_launch.transport),
                                    restore_launch.command.as_deref(),
                                )),
                                Err(error) => {
                                    logging::error(format!(
                                        "could not resolve restore launch override for {}: {}",
                                        descriptor, error
                                    ));
                                    None
                                }
                            }
                        });
                    (
                        Rc::new(TerminalLaunchSpec {
                            working_directory: working_dir.display().to_string(),
                            configured_argv: build_spawn_argv(
                                &launch_shell,
                                resolved_launch.command.as_deref(),
                            ),
                            local_shell_argv: build_local_shell_argv(&launch_shell),
                            envv: vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into()],
                        }),
                        initial_configured_argv,
                    )
                }
                Err(error) => {
                    report_spawn_problem(&terminal, &descriptor, &error);
                    mark_state_exited(&state);
                    (
                        Rc::new(TerminalLaunchSpec {
                            working_directory: working_dir.display().to_string(),
                            configured_argv: Vec::new(),
                            local_shell_argv: build_local_shell_argv(&shell),
                            envv: vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into()],
                        }),
                        None,
                    )
                }
            }
        };

        let session = Self {
            terminal: terminal.clone(),
            state,
            descriptor,
            launch_spec,
            transcript,
        };

        let restored_history = restored_terminal_history_text(restored_history_lines);
        if !restored_history.is_empty() {
            terminal.feed(restored_history.as_bytes());
        }

        if let Some(initial_configured_argv) = initial_configured_argv
            .as_ref()
            .filter(|argv| !argv.is_empty())
        {
            session.spawn_argv(initial_configured_argv);
        } else if !session.launch_spec.configured_argv.is_empty() {
            session.spawn_from_mode(TerminalLaunchMode::ConfiguredSession);
        }

        session
    }

    pub fn widget(&self) -> vte4::Terminal {
        self.terminal.clone()
    }

    pub fn terminate(&self, reason: &str) {
        request_process_termination(&self.state, &self.descriptor, reason);
    }

    pub fn terminate_immediately(&self, reason: &str) {
        request_process_termination_immediately(&self.state, &self.descriptor, reason);
    }

    pub fn apply_appearance(
        &self,
        use_dark_palette: bool,
        density: ApplicationDensity,
        zoom_steps: i32,
    ) {
        apply_terminal_appearance(&self.terminal, use_dark_palette, density, zoom_steps);
    }

    pub fn has_selection(&self) -> bool {
        self.terminal.has_selection()
    }

    pub fn copy_selection_to_clipboard(&self) -> bool {
        copy_terminal_selection(&self.terminal)
    }

    pub fn paste_clipboard(&self) {
        paste_terminal_clipboard(&self.terminal);
    }

    pub fn send_text(&self, text: &str) -> bool {
        if !self.has_active_process() {
            logging::info(format!(
                "skipped terminal input for inactive session {}",
                self.descriptor
            ));
            return false;
        }

        self.transcript.borrow_mut().push_input(text);
        self.terminal.grab_focus();
        self.terminal.feed_child(text.as_bytes());
        true
    }

    pub fn recent_output(&self, row_count: i64) -> String {
        self.refresh_output_snapshot();
        self.transcript
            .borrow()
            .recent_output(row_count.max(0) as usize)
    }

    pub fn recent_transcript(&self, line_count: usize) -> String {
        self.refresh_output_snapshot();
        self.transcript.borrow().recent_transcript(line_count)
    }

    pub fn capture_terminal_history(&self, max_lines: usize) -> Vec<String> {
        let Some(snapshot) = self.output_snapshot() else {
            return Vec::new();
        };
        normalize_terminal_history_lines(&snapshot, max_lines)
    }

    pub fn termination_requested(&self) -> bool {
        self.state.borrow().termination_requested
    }

    pub fn child_pid(&self) -> Option<u32> {
        self.state.borrow().child_pid.map(|pid| pid as u32)
    }

    pub fn last_exit_status(&self) -> Option<i32> {
        self.state.borrow().last_exit_status
    }

    pub fn has_active_process(&self) -> bool {
        let state = self.state.borrow();
        !state.exited && !state.termination_requested
    }

    pub fn supports_recovery_options(&self) -> bool {
        self.launch_spec.supports_recovery_options()
    }

    pub fn needs_recovery_prompt(&self) -> bool {
        let state = self.state.borrow();
        state.exited
            && !state.termination_requested
            && !state.auto_reconnect_pending
            && self.supports_recovery_options()
    }

    pub fn auto_reconnect_pending(&self) -> bool {
        self.state.borrow().auto_reconnect_pending
    }

    pub fn set_auto_reconnect_pending(&self, pending: bool) {
        self.state.borrow_mut().auto_reconnect_pending = pending;
    }

    pub fn auto_reconnect_attempts(&self) -> u8 {
        self.state.borrow().auto_reconnect_attempts
    }

    pub fn register_auto_reconnect_attempt(&self) -> u8 {
        let mut state = self.state.borrow_mut();
        state.auto_reconnect_attempts = state.auto_reconnect_attempts.saturating_add(1);
        state.auto_reconnect_attempts
    }

    pub fn reset_auto_reconnect_attempts(&self) {
        self.state.borrow_mut().auto_reconnect_attempts = 0;
    }

    pub fn reconnect(&self) -> Result<(), String> {
        self.spawn_launch_mode(
            TerminalLaunchMode::ConfiguredSession,
            b"\r\n[terminaltiler] reconnecting terminal session\r\n",
        )
    }

    pub fn open_local_shell(&self) -> Result<(), String> {
        self.spawn_launch_mode(
            TerminalLaunchMode::LocalShell,
            b"\r\n[terminaltiler] opening local shell\r\n",
        )
    }

    fn spawn_launch_mode(&self, mode: TerminalLaunchMode, notice: &[u8]) -> Result<(), String> {
        if let Err(error) = validate_working_dir(Path::new(&self.launch_spec.working_directory)) {
            let error = error.to_string();
            self.report_spawn_problem(&error);
            self.mark_exited();
            return Err(error);
        }

        self.terminal.feed(notice);
        self.spawn_from_mode(mode);
        Ok(())
    }

    pub fn paste_dropped_paths(&self, paths: &[PathBuf]) -> bool {
        let Some(payload) = dropped_paths::serialize_posix_paths(
            paths.iter().map(|path| path.as_os_str().to_string_lossy()),
        ) else {
            return false;
        };

        if !self.has_active_process() {
            logging::info(format!(
                "skipped dropped-path paste for inactive session {}",
                self.descriptor
            ));
            return false;
        }

        self.transcript.borrow_mut().push_input(&payload);
        self.terminal.grab_focus();
        self.terminal.paste_text(&payload);
        true
    }

    fn mark_exited(&self) {
        mark_state_exited(&self.state);
    }

    fn report_spawn_problem(&self, message: &str) {
        report_spawn_problem(&self.terminal, &self.descriptor, message);
    }

    fn spawn_from_mode(&self, mode: TerminalLaunchMode) {
        let argv = self.launch_spec.argv_for_mode(mode);
        self.spawn_argv(argv);
    }

    fn spawn_argv(&self, argv: &[String]) {
        if argv.is_empty() {
            return;
        }
        let argv_refs = argv.iter().map(String::as_str).collect::<Vec<_>>();
        let env_refs = self
            .launch_spec
            .envv
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        {
            let mut state = self.state.borrow_mut();
            state.exited = false;
            state.child_pid = None;
            state.last_exit_status = None;
            state.termination_requested = false;
            state.immediate_termination_requested = false;
            state.auto_reconnect_pending = false;
            state.clear_kill_timeout();
        }
        let state_for_spawn = self.state.clone();
        let descriptor_for_spawn = self.descriptor.clone();
        let terminal_for_error = self.terminal.clone();

        self.terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(self.launch_spec.working_directory.as_str()),
            &argv_refs,
            &env_refs,
            glib::SpawnFlags::SEARCH_PATH,
            || unsafe {
                libc::setsid();
            },
            -1,
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(pid) => {
                    let pid = pid.0 as libc::pid_t;
                    let (termination_requested, immediate_termination_requested) = {
                        let mut state = state_for_spawn.borrow_mut();
                        state.child_pid = Some(pid);
                        state.exited = false;
                        (
                            state.termination_requested,
                            state.immediate_termination_requested,
                        )
                    };

                    logging::info(format!(
                        "spawned terminal child pid={} {}",
                        pid, descriptor_for_spawn
                    ));

                    if termination_requested {
                        if immediate_termination_requested {
                            request_process_termination_immediately(
                                &state_for_spawn,
                                &descriptor_for_spawn,
                                "workspace closed before spawn completed",
                            );
                        } else {
                            request_process_termination(
                                &state_for_spawn,
                                &descriptor_for_spawn,
                                "workspace closed before spawn completed",
                            );
                        }
                    }
                }
                Err(error) => {
                    report_spawn_problem(
                        &terminal_for_error,
                        &descriptor_for_spawn,
                        &format!("TerminalTiler could not spawn this workspace shell.\n\n{error}"),
                    );
                    mark_state_exited(&state_for_spawn);
                }
            },
        );
    }

    fn refresh_output_snapshot(&self) {
        let Some(snapshot) = self.output_snapshot() else {
            return;
        };
        self.transcript.borrow_mut().replace_output(&snapshot);
    }

    fn output_snapshot(&self) -> Option<String> {
        let stream = gio::MemoryOutputStream::new_resizable();
        if self
            .terminal
            .write_contents_sync(
                &stream,
                vte4::WriteFlags::Default,
                None::<&gio::Cancellable>,
            )
            .is_err()
        {
            return None;
        }
        if stream.close(None::<&gio::Cancellable>).is_err() {
            return None;
        }
        let bytes = stream.steal_as_bytes();
        Some(String::from_utf8_lossy(bytes.as_ref()).into_owned())
    }
}

pub fn clamp_terminal_zoom_steps(density: ApplicationDensity, zoom_steps: i32) -> i32 {
    let base_points = density.terminal_font_points();
    (base_points + zoom_steps).clamp(MIN_TERMINAL_FONT_POINTS, MAX_TERMINAL_FONT_POINTS)
        - base_points
}

fn effective_terminal_font_points(density: ApplicationDensity, zoom_steps: i32) -> i32 {
    density.terminal_font_points() + clamp_terminal_zoom_steps(density, zoom_steps)
}

fn apply_terminal_appearance(
    terminal: &vte4::Terminal,
    use_dark_palette: bool,
    density: ApplicationDensity,
    zoom_steps: i32,
) {
    terminal.set_font(Some(&pango::FontDescription::from_string(&format!(
        "JetBrains Mono {}",
        effective_terminal_font_points(density, zoom_steps)
    ))));
    terminal.set_cell_height_scale(density.terminal_line_height_scale());
    apply_terminal_palette(terminal, use_dark_palette);
}

fn apply_terminal_palette(terminal: &vte4::Terminal, use_dark_palette: bool) {
    let palette = terminal_palette(use_dark_palette);

    let foreground = rgba(palette.foreground);
    let background = rgba(palette.background);
    let cursor = rgba(palette.cursor);
    let cursor_foreground = rgba(palette.cursor_foreground);
    let highlight_background = rgba(palette.highlight_background);
    let highlight_foreground = rgba(palette.highlight_foreground);
    let palette_colors = palette
        .palette
        .iter()
        .map(|value| rgba(value))
        .collect::<Vec<_>>();
    let palette_refs = palette_colors.iter().collect::<Vec<_>>();

    terminal.set_colors(Some(&foreground), Some(&background), &palette_refs);
    terminal.set_color_cursor(Some(&cursor));
    terminal.set_color_cursor_foreground(Some(&cursor_foreground));
    terminal.set_color_highlight(Some(&highlight_background));
    terminal.set_color_highlight_foreground(Some(&highlight_foreground));
}

fn rgba(value: &str) -> gdk::RGBA {
    gdk::RGBA::parse(value).expect("terminal palette color should parse")
}

fn copy_terminal_selection(terminal: &vte4::Terminal) -> bool {
    if !terminal.has_selection() {
        return false;
    }

    terminal.grab_focus();
    terminal.copy_clipboard_format(vte4::Format::Text);
    true
}

fn paste_terminal_clipboard(terminal: &vte4::Terminal) {
    terminal.grab_focus();
    terminal.paste_clipboard();
}

fn install_terminal_input_stats_hook(
    terminal: &vte4::Terminal,
    state: Rc<RefCell<TerminalSessionState>>,
    stats: StatsRecorder,
) {
    let manual_typing_armed = Rc::new(RefCell::new(None::<Instant>));
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let manual_typing_armed = manual_typing_armed.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifier_state| {
            if is_manual_printable_key(key, modifier_state) {
                *manual_typing_armed.borrow_mut() = Some(Instant::now());
            }
            gtk::glib::Propagation::Proceed
        });
    }
    terminal.add_controller(key_controller);

    terminal.connect_commit(move |_, text, _| {
        let state = state.borrow();
        if state.exited || state.termination_requested {
            *manual_typing_armed.borrow_mut() = None;
            return;
        }

        let armed = manual_typing_armed.borrow_mut().take();
        if armed.is_some_and(|armed| armed.elapsed() <= Duration::from_millis(750)) {
            stats.record_manual_typing(text);
        }
    });
}

fn is_manual_printable_key(key: gdk::Key, state: gdk::ModifierType) -> bool {
    let modifiers = state & gtk::accelerator_get_default_mod_mask();
    (modifiers.is_empty() || modifiers == gdk::ModifierType::SHIFT_MASK)
        && key.to_unicode().is_some_and(|value| !value.is_control())
}

fn install_terminal_shortcuts(terminal: &vte4::Terminal) {
    let shortcut_controller = gtk::ShortcutController::new();
    shortcut_controller.set_scope(gtk::ShortcutScope::Local);

    let terminal_for_copy = terminal.clone();
    let copy_action = gtk::CallbackAction::new(move |_, _| {
        if copy_terminal_selection(&terminal_for_copy) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    add_terminal_shortcut(
        &shortcut_controller,
        DEFAULT_TERMINAL_COPY_SHORTCUT,
        "copy",
        &copy_action,
    );

    let terminal_for_paste = terminal.clone();
    let paste_action = gtk::CallbackAction::new(move |_, _| {
        paste_terminal_clipboard(&terminal_for_paste);
        glib::Propagation::Stop
    });
    add_terminal_shortcut(
        &shortcut_controller,
        DEFAULT_TERMINAL_PASTE_SHORTCUT,
        "paste",
        &paste_action,
    );

    terminal.add_controller(shortcut_controller);
}

fn add_terminal_shortcut(
    shortcut_controller: &gtk::ShortcutController,
    accelerator: &str,
    shortcut_name: &str,
    action: &gtk::CallbackAction,
) {
    let Some(trigger) = gtk::ShortcutTrigger::parse_string(accelerator) else {
        logging::error(format!(
            "failed to parse terminal {} shortcut '{}'",
            shortcut_name, accelerator
        ));
        return;
    };

    shortcut_controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action.clone())));
}

fn mark_state_exited(state: &Rc<RefCell<TerminalSessionState>>) {
    let mut state = state.borrow_mut();
    state.exited = true;
    state.child_pid = None;
    state.last_exit_status = None;
    state.auto_reconnect_pending = false;
    state.immediate_termination_requested = false;
    state.clear_kill_timeout();
}

fn request_process_termination(
    state: &Rc<RefCell<TerminalSessionState>>,
    descriptor: &str,
    reason: &str,
) {
    let pid = prepare_process_termination(state, descriptor, reason, false);
    let Some(pid) = pid else {
        return;
    };

    logging::info(format!(
        "terminating terminal process tree pid={} reason='{}' {}",
        pid, reason, descriptor
    ));
    let targets = process_signal_targets(pid);
    send_signal_to_targets(&targets, libc::SIGHUP, descriptor);
    send_signal_to_targets(&targets, libc::SIGTERM, descriptor);

    let descriptor = descriptor.to_string();
    let state_weak = Rc::downgrade(state);
    let timeout = glib::timeout_add_seconds_local_once(2, move || {
        if let Some(state) = state_weak.upgrade() {
            escalate_termination(&state, &descriptor);
        }
    });
    state.borrow_mut().kill_timeout = Some(timeout);
}

fn request_process_termination_immediately(
    state: &Rc<RefCell<TerminalSessionState>>,
    descriptor: &str,
    reason: &str,
) {
    let pid = prepare_process_termination(state, descriptor, reason, true);
    let Some(pid) = pid else {
        return;
    };

    logging::info(format!(
        "immediately terminating terminal process tree pid={} reason='{}' {}",
        pid, reason, descriptor
    ));
    let targets = process_signal_targets(pid);
    send_signal_to_targets(&targets, libc::SIGHUP, descriptor);
    send_signal_to_targets(&targets, libc::SIGTERM, descriptor);
    send_signal_to_targets(&targets, libc::SIGKILL, descriptor);
}

fn prepare_process_termination(
    state: &Rc<RefCell<TerminalSessionState>>,
    descriptor: &str,
    reason: &str,
    immediate: bool,
) -> Option<libc::pid_t> {
    let pid = {
        let mut state = state.borrow_mut();
        if state.exited {
            logging::info(format!(
                "termination skipped for already-exited terminal {}",
                descriptor
            ));
            return None;
        }

        state.termination_requested = true;
        state.immediate_termination_requested |= immediate;
        state.auto_reconnect_pending = false;
        state.clear_kill_timeout();
        state.child_pid
    };

    if pid.is_none() {
        logging::info(format!(
            "queued terminal termination until spawn completes reason='{}' {}",
            reason, descriptor
        ));
    }
    pid
}

fn escalate_termination(state: &Rc<RefCell<TerminalSessionState>>, descriptor: &str) {
    let pid = {
        let mut state = state.borrow_mut();
        state.kill_timeout = None;
        if state.exited {
            return;
        }
        state.child_pid
    };

    let Some(pid) = pid else {
        return;
    };

    let targets = process_signal_targets(pid);
    if targets.iter().any(|target| signal_target_exists(*target)) {
        logging::info(format!(
            "escalating terminal termination with SIGKILL pid={} {}",
            pid, descriptor
        ));
        send_signal_to_targets(&targets, libc::SIGKILL, descriptor);
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum ProcessSignalTarget {
    Process(libc::pid_t),
    ProcessGroup(libc::pid_t),
}

impl ProcessSignalTarget {
    fn kill_target(self) -> libc::pid_t {
        match self {
            Self::Process(pid) => pid,
            Self::ProcessGroup(pgid) => -pgid,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Process(_) => "process",
            Self::ProcessGroup(_) => "process group",
        }
    }

    fn id(self) -> libc::pid_t {
        match self {
            Self::Process(pid) | Self::ProcessGroup(pid) => pid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProcessRecord {
    pid: libc::pid_t,
    ppid: libc::pid_t,
    pgid: libc::pid_t,
}

fn process_signal_targets(root_pid: libc::pid_t) -> Vec<ProcessSignalTarget> {
    let process_table = read_linux_process_table();
    collect_process_signal_targets(root_pid, &process_table)
}

fn collect_process_signal_targets(
    root_pid: libc::pid_t,
    process_table: &[ProcessRecord],
) -> Vec<ProcessSignalTarget> {
    if root_pid <= 0 {
        return Vec::new();
    }

    let by_pid = process_table
        .iter()
        .map(|record| (record.pid, *record))
        .collect::<HashMap<_, _>>();
    let current_pgrp = current_process_group();
    let mut children_by_parent: HashMap<libc::pid_t, Vec<libc::pid_t>> = HashMap::new();
    for record in process_table {
        children_by_parent
            .entry(record.ppid)
            .or_default()
            .push(record.pid);
    }

    let mut targets = Vec::new();
    let mut seen_targets = HashSet::new();
    let mut seen_processes = HashSet::new();
    let mut queue = VecDeque::from([root_pid]);

    while let Some(pid) = queue.pop_front() {
        if !seen_processes.insert(pid) {
            continue;
        }

        if let Some(record) = by_pid.get(&pid) {
            add_signal_target(
                signal_target_for_record(*record, current_pgrp),
                &mut targets,
                &mut seen_targets,
            );
        } else if pid == root_pid {
            let fallback = if root_pid == current_pgrp {
                ProcessSignalTarget::Process(root_pid)
            } else {
                ProcessSignalTarget::ProcessGroup(root_pid)
            };
            add_signal_target(fallback, &mut targets, &mut seen_targets);
        } else {
            add_signal_target(
                ProcessSignalTarget::Process(pid),
                &mut targets,
                &mut seen_targets,
            );
        }

        if let Some(children) = children_by_parent.get(&pid) {
            queue.extend(children);
        }
    }

    targets
}

fn signal_target_for_record(
    record: ProcessRecord,
    current_pgrp: libc::pid_t,
) -> ProcessSignalTarget {
    if record.pgid > 0 && record.pgid != current_pgrp {
        ProcessSignalTarget::ProcessGroup(record.pgid)
    } else {
        ProcessSignalTarget::Process(record.pid)
    }
}

fn current_process_group() -> libc::pid_t {
    unsafe { libc::getpgrp() }
}

fn add_signal_target(
    target: ProcessSignalTarget,
    targets: &mut Vec<ProcessSignalTarget>,
    seen: &mut HashSet<ProcessSignalTarget>,
) {
    match target {
        ProcessSignalTarget::Process(pid) | ProcessSignalTarget::ProcessGroup(pid) if pid <= 0 => {
            return;
        }
        _ => {}
    }

    if seen.insert(target) {
        targets.push(target);
    }
}

fn read_linux_process_table() -> Vec<ProcessRecord> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse().ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            parse_linux_stat(pid, &stat)
        })
        .collect()
}

fn parse_linux_stat(pid: libc::pid_t, stat: &str) -> Option<ProcessRecord> {
    let after_comm = stat.rsplit_once(") ")?.1;
    let fields = after_comm.split_whitespace().collect::<Vec<_>>();
    Some(ProcessRecord {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        pgid: fields.get(2)?.parse().ok()?,
    })
}

fn send_signal_to_targets(targets: &[ProcessSignalTarget], signal: libc::c_int, descriptor: &str) {
    if targets.is_empty() {
        logging::error(format!(
            "no terminal process targets found while sending {} {}",
            signal_name(signal),
            descriptor
        ));
        return;
    }

    for target in targets {
        send_signal_to_target(*target, signal, descriptor);
    }
}

fn signal_target_exists(target: ProcessSignalTarget) -> bool {
    unsafe {
        if libc::kill(target.kill_target(), 0) == 0 {
            true
        } else {
            io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        }
    }
}

fn send_signal_to_target(target: ProcessSignalTarget, signal: libc::c_int, descriptor: &str) {
    let result = unsafe { libc::kill(target.kill_target(), signal) };
    if result == 0 {
        return;
    }

    let errno = io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default();
    if errno != libc::ESRCH {
        logging::error(format!(
            "failed to send {} to terminal {} id={} errno={} {}",
            signal_name(signal),
            target.label(),
            target.id(),
            errno,
            descriptor
        ));
    }
}

fn signal_name(signal: libc::c_int) -> &'static str {
    match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGTERM => "SIGTERM",
        libc::SIGKILL => "SIGKILL",
        _ => "UNKNOWN",
    }
}

fn validate_working_dir(path: &Path) -> Result<(), WorkingDirectoryValidationError> {
    if !path.exists() {
        return Err(WorkingDirectoryValidationError::Missing(path.to_path_buf()));
    }

    if !path.is_dir() {
        return Err(WorkingDirectoryValidationError::NotDirectory(
            path.to_path_buf(),
        ));
    }

    Ok(())
}

fn build_spawn_argv(shell: &str, command: Option<&str>) -> Vec<String> {
    match command.filter(|value| !value.trim().is_empty()) {
        Some(command) => vec![
            shell.to_string(),
            "-i".into(),
            "-c".into(),
            command.to_string(),
        ],
        None => build_local_shell_argv(shell),
    }
}

fn shell_for_launch(default_shell: &str, transport: &ResolvedLaunchTransport) -> String {
    match transport {
        ResolvedLaunchTransport::LocalProfile {
            shell_program: Some(shell_program),
            ..
        } if !shell_program.trim().is_empty() => shell_program.trim().to_string(),
        _ => default_shell.to_string(),
    }
}

fn build_local_shell_argv(shell: &str) -> Vec<String> {
    vec![shell.to_string()]
}

fn supports_recovery_options(configured_argv: &[String], local_shell_argv: &[String]) -> bool {
    !configured_argv.is_empty() && configured_argv != local_shell_argv
}

fn report_spawn_problem(terminal: &vte4::Terminal, descriptor: &str, message: &str) {
    logging::error(format!(
        "terminal launch failure {}: {}",
        descriptor, message
    ));
    let rendered = format!("\r\n{}\r\n", message);
    terminal.feed(rendered.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::{
        ProcessRecord, ProcessSignalTarget, WorkingDirectoryValidationError,
        build_local_shell_argv, build_spawn_argv, collect_process_signal_targets, parse_linux_stat,
        shell_for_launch, supports_recovery_options, validate_working_dir,
    };
    use crate::services::launch_resolution::ResolvedLaunchTransport;
    use std::path::Path;

    #[test]
    fn builds_interactive_shell_argv_for_startup_commands() {
        let argv = build_spawn_argv("/bin/bash", Some("cargo test"));

        assert_eq!(argv, vec!["/bin/bash", "-i", "-c", "cargo test"]);
    }

    #[test]
    fn local_profile_shell_overrides_default_shell() {
        let transport = ResolvedLaunchTransport::LocalProfile {
            profile_id: "bash-env".into(),
            profile_name: "Bash Env".into(),
            shell_program: Some(" /bin/bash ".into()),
            startup_prefix: None,
        };

        assert_eq!(shell_for_launch("/usr/bin/fish", &transport), "/bin/bash");
    }

    #[test]
    fn local_profile_without_shell_keeps_default_shell() {
        let transport = ResolvedLaunchTransport::LocalProfile {
            profile_id: "default".into(),
            profile_name: "Default".into(),
            shell_program: Some("   ".into()),
            startup_prefix: None,
        };

        assert_eq!(
            shell_for_launch("/usr/bin/fish", &transport),
            "/usr/bin/fish"
        );
    }

    #[test]
    fn non_local_profile_launch_keeps_default_shell() {
        let transport = ResolvedLaunchTransport::DefaultLocal;

        assert_eq!(
            shell_for_launch("/usr/bin/fish", &transport),
            "/usr/bin/fish"
        );
    }

    #[test]
    fn omits_command_flags_when_startup_command_is_blank() {
        let argv = build_spawn_argv("/bin/bash", Some("   "));

        assert_eq!(argv, vec!["/bin/bash"]);
    }

    #[test]
    fn startup_command_launches_offer_recovery_options() {
        let configured = build_spawn_argv("/bin/bash", Some("ssh host.example.com"));
        let local_shell = build_local_shell_argv("/bin/bash");

        assert!(supports_recovery_options(&configured, &local_shell));
    }

    #[test]
    fn plain_shell_launches_do_not_offer_recovery_options() {
        let configured = build_spawn_argv("/bin/bash", None);
        let local_shell = build_local_shell_argv("/bin/bash");

        assert!(!supports_recovery_options(&configured, &local_shell));
    }

    #[test]
    fn rejects_missing_working_directories() {
        let error = validate_working_dir(Path::new("/definitely/not/here"))
            .expect_err("missing working directory should fail");

        assert!(matches!(
            error,
            WorkingDirectoryValidationError::Missing(path)
                if path == Path::new("/definitely/not/here")
        ));
    }

    #[test]
    fn process_group_signal_target_uses_negative_kill_target() {
        assert_eq!(ProcessSignalTarget::ProcessGroup(4242).kill_target(), -4242);
        assert_eq!(ProcessSignalTarget::Process(4242).kill_target(), 4242);
    }

    #[test]
    fn process_signal_targets_include_descendant_process_groups_once() {
        let process_table = vec![
            ProcessRecord {
                pid: 99000,
                ppid: 1,
                pgid: 99000,
            },
            ProcessRecord {
                pid: 99001,
                ppid: 99000,
                pgid: 99000,
            },
            ProcessRecord {
                pid: 99002,
                ppid: 99001,
                pgid: 99002,
            },
            ProcessRecord {
                pid: 99003,
                ppid: 99002,
                pgid: 99002,
            },
            ProcessRecord {
                pid: 99004,
                ppid: 99000,
                pgid: 99004,
            },
            ProcessRecord {
                pid: 99005,
                ppid: 99004,
                pgid: 99002,
            },
        ];

        let targets = collect_process_signal_targets(99000, &process_table);

        assert_eq!(targets.len(), 3);
        assert!(targets.contains(&ProcessSignalTarget::ProcessGroup(99000)));
        assert!(targets.contains(&ProcessSignalTarget::ProcessGroup(99002)));
        assert!(targets.contains(&ProcessSignalTarget::ProcessGroup(99004)));
    }

    #[test]
    fn process_signal_targets_fall_back_to_root_process_group_without_proc_record() {
        assert_eq!(
            collect_process_signal_targets(99999, &[]),
            vec![ProcessSignalTarget::ProcessGroup(99999)]
        );
        assert!(collect_process_signal_targets(0, &[]).is_empty());
    }

    #[test]
    fn parses_linux_stat_with_spaces_and_parentheses_in_command_name() {
        let record = parse_linux_stat(
            123,
            "123 (agent (worker) shell) S 45 67 67 34816 123 4194304",
        )
        .expect("stat line should parse");

        assert_eq!(
            record,
            ProcessRecord {
                pid: 123,
                ppid: 45,
                pgid: 67,
            }
        );
    }
}
