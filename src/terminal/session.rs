use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use vte4::prelude::*;

use crate::logging;
use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;

const DEFAULT_TERMINAL_COPY_SHORTCUT: &str = "<Ctrl><Shift>C";
const DEFAULT_TERMINAL_PASTE_SHORTCUT: &str = "<Ctrl><Shift>V";
const MIN_TERMINAL_FONT_POINTS: i32 = 7;
const MAX_TERMINAL_FONT_POINTS: i32 = 20;
const DARK_TERMINAL_PALETTE: [&str; 16] = [
    "#0f1724", "#c9575f", "#78a062", "#d6a04b", "#6b8cff", "#b28cf0", "#5eb8c8", "#d7dde8",
    "#334155", "#ef7c86", "#91be78", "#e6bb6a", "#8fa7ff", "#c8a6f6", "#7ccad7", "#f8fafc",
];
const LIGHT_TERMINAL_PALETTE: [&str; 16] = [
    "#24313f", "#b24f45", "#617d43", "#9b6d11", "#4168b5", "#8b61a8", "#2f7f8a", "#d6dde8",
    "#516172", "#cf685d", "#78975a", "#b38622", "#5e81ca", "#a47dc1", "#4f97a2", "#f7f2e8",
];

#[derive(Clone, Copy)]
struct TerminalPalette {
    foreground: &'static str,
    background: &'static str,
    cursor: &'static str,
    cursor_foreground: &'static str,
    highlight_background: &'static str,
    highlight_foreground: &'static str,
    palette: &'static [&'static str; 16],
}

#[derive(Clone)]
pub struct TerminalSession {
    terminal: vte4::Terminal,
    state: Rc<RefCell<TerminalSessionState>>,
    descriptor: Rc<str>,
}

#[derive(Default)]
struct TerminalSessionState {
    child_pid: Option<libc::pid_t>,
    exited: bool,
    termination_requested: bool,
    kill_timeout: Option<glib::SourceId>,
}

impl TerminalSessionState {
    fn clear_kill_timeout(&mut self) {
        if let Some(source_id) = self.kill_timeout.take() {
            source_id.remove();
        }
    }
}

impl TerminalSession {
    pub fn spawn(
        tile: &TileSpec,
        workspace_root: &Path,
        use_dark_palette: bool,
        density: ApplicationDensity,
        zoom_steps: i32,
    ) -> Self {
        let terminal = vte4::Terminal::new();
        terminal.set_hexpand(true);
        terminal.set_vexpand(true);
        terminal.set_scrollback_lines(20_000);
        terminal.set_mouse_autohide(true);
        terminal.set_clear_background(false);
        terminal.set_cursor_blink_mode(vte4::CursorBlinkMode::System);
        install_terminal_shortcuts(&terminal);
        apply_terminal_appearance(&terminal, use_dark_palette, density, zoom_steps);

        let working_dir = tile.working_directory.resolve(workspace_root);
        let state = Rc::new(RefCell::new(TerminalSessionState::default()));
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
                state.clear_kill_timeout();
                logging::info(format!(
                    "terminal child exited status={} {}",
                    status, descriptor
                ));
            });
        }

        let session = Self {
            terminal: terminal.clone(),
            state,
            descriptor,
        };

        if let Some(error) = validate_working_dir(&working_dir) {
            session.report_spawn_problem(&error);
            session.mark_exited();
            return session;
        }

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let argv = build_spawn_argv(&shell, tile.startup_command.as_deref());
        let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let envv = ["TERM=xterm-256color", "COLORTERM=truecolor"];
        let working_dir_label = working_dir.display().to_string();
        let state_for_spawn = session.state.clone();
        let descriptor_for_spawn = session.descriptor.clone();
        let terminal_for_error = session.terminal.clone();

        terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(working_dir_label.as_str()),
            &argv_refs,
            &envv,
            glib::SpawnFlags::SEARCH_PATH,
            || unsafe {
                libc::setsid();
            },
            -1,
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(pid) => {
                    let pid = pid.0 as libc::pid_t;
                    let terminate_immediately = {
                        let mut state = state_for_spawn.borrow_mut();
                        state.child_pid = Some(pid);
                        state.exited = false;
                        state.termination_requested
                    };

                    logging::info(format!(
                        "spawned terminal child pid={} {}",
                        pid, descriptor_for_spawn
                    ));

                    if terminate_immediately {
                        request_process_termination(
                            &state_for_spawn,
                            &descriptor_for_spawn,
                            "workspace closed before spawn completed",
                        );
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

        session
    }

    pub fn widget(&self) -> vte4::Terminal {
        self.terminal.clone()
    }

    pub fn terminate(&self, reason: &str) {
        request_process_termination(&self.state, &self.descriptor, reason);
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

    pub fn paste_dropped_paths(&self, paths: &[PathBuf]) -> bool {
        let Some(payload) = serialize_dropped_paths(paths) else {
            return false;
        };

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
    let palette = if use_dark_palette {
        TerminalPalette {
            foreground: "#d7dde8",
            background: "#0f1724",
            cursor: "#f2b35f",
            cursor_foreground: "#101923",
            highlight_background: "#27405f",
            highlight_foreground: "#f8fafc",
            palette: &DARK_TERMINAL_PALETTE,
        }
    } else {
        TerminalPalette {
            foreground: "#223041",
            background: "#f4efe4",
            cursor: "#cb7a2b",
            cursor_foreground: "#fffaf1",
            highlight_background: "#d7e2f2",
            highlight_foreground: "#16202b",
            palette: &LIGHT_TERMINAL_PALETTE,
        }
    };

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
    state.clear_kill_timeout();
}

fn request_process_termination(
    state: &Rc<RefCell<TerminalSessionState>>,
    descriptor: &str,
    reason: &str,
) {
    let pid = {
        let mut state = state.borrow_mut();
        if state.exited {
            logging::info(format!(
                "termination skipped for already-exited terminal {}",
                descriptor
            ));
            return;
        }

        state.termination_requested = true;
        state.clear_kill_timeout();
        state.child_pid
    };

    let Some(pid) = pid else {
        logging::info(format!(
            "queued terminal termination until spawn completes reason='{}' {}",
            reason, descriptor
        ));
        return;
    };

    logging::info(format!(
        "terminating terminal process group pid={} reason='{}' {}",
        pid, reason, descriptor
    ));
    send_signal_to_process_group(pid, libc::SIGHUP, descriptor);
    send_signal_to_process_group(pid, libc::SIGTERM, descriptor);

    let descriptor = descriptor.to_string();
    let state_weak = Rc::downgrade(state);
    let timeout = glib::timeout_add_seconds_local_once(2, move || {
        if let Some(state) = state_weak.upgrade() {
            escalate_termination(&state, &descriptor);
        }
    });
    state.borrow_mut().kill_timeout = Some(timeout);
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

    if process_group_exists(pid) {
        logging::info(format!(
            "escalating terminal termination with SIGKILL pid={} {}",
            pid, descriptor
        ));
        send_signal_to_process_group(pid, libc::SIGKILL, descriptor);
    }
}

fn process_group_exists(pid: libc::pid_t) -> bool {
    let Some(target) = process_group_target(pid) else {
        return false;
    };

    unsafe {
        if libc::kill(target, 0) == 0 {
            true
        } else {
            io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        }
    }
}

fn send_signal_to_process_group(pid: libc::pid_t, signal: libc::c_int, descriptor: &str) {
    let Some(target) = process_group_target(pid) else {
        logging::error(format!(
            "invalid terminal pid {} while sending {} {}",
            pid,
            signal_name(signal),
            descriptor
        ));
        return;
    };

    let result = unsafe { libc::kill(target, signal) };
    if result == 0 {
        return;
    }

    let errno = io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default();
    if errno != libc::ESRCH {
        logging::error(format!(
            "failed to send {} to terminal process group pid={} errno={} {}",
            signal_name(signal),
            pid,
            errno,
            descriptor
        ));
    }
}

fn process_group_target(pid: libc::pid_t) -> Option<libc::pid_t> {
    if pid > 0 { Some(-pid) } else { None }
}

fn signal_name(signal: libc::c_int) -> &'static str {
    match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGTERM => "SIGTERM",
        libc::SIGKILL => "SIGKILL",
        _ => "UNKNOWN",
    }
}

fn validate_working_dir(path: &Path) -> Option<String> {
    if !path.exists() {
        return Some(format!(
            "The working directory does not exist:\n{}",
            path.display()
        ));
    }

    if !path.is_dir() {
        return Some(format!(
            "The working directory is not a directory:\n{}",
            path.display()
        ));
    }

    None
}

fn build_spawn_argv(shell: &str, command: Option<&str>) -> Vec<String> {
    let mut argv = vec![shell.to_string()];
    if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
        argv.push("-lc".into());
        argv.push(command.to_string());
    }
    argv
}

fn report_spawn_problem(terminal: &vte4::Terminal, descriptor: &str, message: &str) {
    logging::error(format!(
        "terminal launch failure {}: {}",
        descriptor, message
    ));
    let rendered = format!("\r\n{}\r\n", message);
    terminal.feed(rendered.as_bytes());
}

fn serialize_dropped_paths(paths: &[PathBuf]) -> Option<String> {
    let serialized = paths
        .iter()
        .map(|path| path.as_os_str())
        .filter(|path| !path.is_empty())
        .map(|path| shell_quote_path(&path.to_string_lossy()))
        .collect::<Vec<_>>();

    if serialized.is_empty() {
        None
    } else {
        Some(format!("{} ", serialized.join(" ")))
    }
}

fn shell_quote_path(path: &str) -> String {
    let escaped = path.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::{
        build_spawn_argv, process_group_target, serialize_dropped_paths, validate_working_dir,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn serializes_single_path_for_shell_paste() {
        let payload = serialize_dropped_paths(&[PathBuf::from("/tmp/report.txt")]);

        assert_eq!(payload.as_deref(), Some("'/tmp/report.txt' "));
    }

    #[test]
    fn serializes_multiple_paths_with_spaces() {
        let payload = serialize_dropped_paths(&[
            PathBuf::from("/tmp/screenshot 1.png"),
            PathBuf::from("/workspace/notes.md"),
        ]);

        assert_eq!(
            payload.as_deref(),
            Some("'/tmp/screenshot 1.png' '/workspace/notes.md' ")
        );
    }

    #[test]
    fn escapes_single_quotes_in_paths() {
        let payload = serialize_dropped_paths(&[PathBuf::from("/tmp/it's-here.txt")]);

        assert_eq!(payload.as_deref(), Some("'/tmp/it'\"'\"'s-here.txt' "));
    }

    #[test]
    fn preserves_raw_directory_paths() {
        let payload = serialize_dropped_paths(&[PathBuf::from("/workspace/project")]);

        assert_eq!(payload.as_deref(), Some("'/workspace/project' "));
    }

    #[test]
    fn ignores_empty_drop_payloads() {
        let payload = serialize_dropped_paths(&[]);

        assert_eq!(payload, None);
    }

    #[test]
    fn builds_login_shell_argv_for_startup_commands() {
        let argv = build_spawn_argv("/bin/bash", Some("cargo test"));

        assert_eq!(argv, vec!["/bin/bash", "-lc", "cargo test"]);
    }

    #[test]
    fn omits_command_flags_when_startup_command_is_blank() {
        let argv = build_spawn_argv("/bin/bash", Some("   "));

        assert_eq!(argv, vec!["/bin/bash"]);
    }

    #[test]
    fn rejects_missing_working_directories() {
        let message = validate_working_dir(Path::new("/definitely/not/here"));

        assert!(
            message
                .as_deref()
                .is_some_and(|value| value.contains("does not exist"))
        );
    }

    #[test]
    fn derives_negative_process_group_signal_target() {
        assert_eq!(process_group_target(4242), Some(-4242));
        assert_eq!(process_group_target(0), None);
    }
}
