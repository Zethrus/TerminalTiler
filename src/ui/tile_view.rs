use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gdk::prelude::StaticType;
use gtk::glib;

use vte4::prelude::*;

use crate::model::assets::{CliSnippet, OutputSeverity, PaneStatusSnapshot, WorkspaceAssets};
use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;
use crate::services::output_helpers::{CompiledOutputHelpers, helper_summary_text};
use crate::services::snippets::resolve_snippet;
use crate::services::stats::StatsRecorder;
use crate::terminal::session::TerminalSession;
use crate::ui::pane_status::initial_status_snapshot;
use crate::ui::snippet_popover::{self, SnippetPopoverInput};
use crate::ui::terminal_context_menu::{self, TerminalContextMenuInput};
use crate::ui::terminal_recovery_popover;
use crate::ui::tile_chrome::{
    TERMINAL_HEADER_BADGE_MAX_CHARS, TileHeaderInput, append_terminal_tile_action_chrome,
    build_terminal_tile_action_chrome, build_tile_frame, build_tile_header_chrome,
    build_tile_shell, make_shrinkable,
};
use crate::ui::tile_drag::TileDragPayload;
use crate::ui::transcript_dialog;

pub struct TileView {
    pub widget: gtk::Widget,
    pub session: TerminalSession,
    pub tile: TileSpec,
    pub close_button: gtk::Button,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    use_dark_palette: bool,
    density: ApplicationDensity,
    zoom_steps: i32,
    restored_history_lines: &[String],
    snippets_provider: Rc<dyn Fn() -> Vec<CliSnippet>>,
    on_swap: Rc<dyn Fn(String, String)>,
    on_close: Rc<dyn Fn(String)>,
    can_close: bool,
    stats: StatsRecorder,
) -> TileView {
    let session = TerminalSession::spawn(
        tile,
        workspace_root,
        assets,
        use_dark_palette,
        density,
        zoom_steps,
        restored_history_lines,
        stats,
    );

    let output_helpers = CompiledOutputHelpers::new(&tile.output_helpers);
    let initial_status_line = initial_status_snapshot(tile, workspace_root, assets)
        .to_line()
        .trim()
        .to_string();
    let shell = build_tile_shell(tile);
    let header = build_tile_header_chrome(TileHeaderInput {
        tile,
        badge_text: &tile.agent_label,
        badge_tooltip: &tile.agent_label,
        badge_max_chars: TERMINAL_HEADER_BADGE_MAX_CHARS,
        status_text: &initial_status_line,
        status_tooltip: &initial_status_line,
        status_ellipsize: gtk::pango::EllipsizeMode::Start,
        drag_tooltip: "Drag this header to swap terminal positions",
    });
    let left = header.drag_handle.clone();
    let title = header.title_label.clone();
    let status = header.status_label.clone();

    let tile_actions = build_terminal_tile_action_chrome(can_close);
    let recovery_button = tile_actions.recovery_button.clone();
    let snippet_button = tile_actions.snippet_button.clone();
    let close_button = tile_actions.close_button.clone();
    {
        let tile_id = tile.id.clone();
        let on_close = on_close.clone();
        close_button.connect_clicked(move |_| {
            on_close(tile_id.clone());
        });
    }

    let actions = header.actions.clone();
    append_terminal_tile_action_chrome(&actions, &tile_actions);

    shell.append(&header.widget);

    let terminal_frame = build_tile_frame("terminal-frame");

    let terminal = session.widget();
    terminal.add_css_class("terminal-surface");
    make_shrinkable(&terminal);

    let recovery_popover = terminal_recovery_popover::build(
        &terminal,
        Rc::new({
            let session = session.clone();
            move || {
                session.reset_auto_reconnect_attempts();
                let _ = session.reconnect();
            }
        }),
        Rc::new({
            let session = session.clone();
            move || {
                session.reset_auto_reconnect_attempts();
                let _ = session.open_local_shell();
            }
        }),
    );
    let show_recovery_prompt: Rc<dyn Fn()> = {
        let terminal = terminal.clone();
        let recovery_popover = recovery_popover.clone();
        Rc::new(move || {
            recovery_popover.set_pointing_to(Some(&default_recovery_prompt_rect(&terminal)));
            recovery_popover.popup();
            terminal.grab_focus();
        })
    };
    {
        let show_recovery_prompt = show_recovery_prompt.clone();
        recovery_button.connect_clicked(move |_| {
            show_recovery_prompt();
        });
    }

    snippet_popover::install(
        &snippet_button,
        SnippetPopoverInput {
            snippets_provider: snippets_provider.clone(),
            before_popup: Some(Rc::new({
                let session = session.clone();
                let show_recovery_prompt = show_recovery_prompt.clone();
                move |popover| {
                    if session.needs_recovery_prompt() {
                        popover.popdown();
                        show_recovery_prompt();
                        false
                    } else {
                        true
                    }
                }
            })),
            execute: Rc::new({
                let session = session.clone();
                let show_recovery_prompt = show_recovery_prompt.clone();
                move |snippet, variables, popover| {
                    if session.needs_recovery_prompt() {
                        popover.popdown();
                        show_recovery_prompt();
                        return Ok(());
                    }

                    let command =
                        resolve_snippet(snippet, &variables).map_err(|error| error.to_string())?;
                    if session.send_text(&command) {
                        Ok(())
                    } else {
                        Err("This pane is not ready to receive input.".into())
                    }
                }
            }),
        },
    );

    install_terminal_context_menu(&terminal, &session, show_recovery_prompt.clone());
    install_terminal_recovery_key_controller(&terminal, &session, show_recovery_prompt.clone());
    terminal_frame.append(&terminal);
    shell.append(&terminal_frame);

    {
        let title_label = title.clone();
        terminal.connect_window_title_changed(move |term| {
            if let Some(new_title) = term.window_title()
                && !new_title.is_empty()
            {
                title_label.set_text(&new_title);
                title_label.set_tooltip_text(Some(&new_title));
            }
        });
    }
    {
        let terminal_for_update = terminal.clone();
        let session_for_update = session.clone();
        let status = status.clone();
        let recovery_button = recovery_button.clone();
        let shell = shell.clone();
        let tile = tile.clone();
        let output_helpers = output_helpers.clone();
        let workspace_root = workspace_root.to_path_buf();
        let assets = assets.clone();
        let update = move || {
            let snapshot = status_snapshot_for_terminal(
                &tile,
                &workspace_root,
                &assets,
                &terminal_for_update,
                &session_for_update,
                &output_helpers,
            );
            let disconnected = session_for_update.needs_recovery_prompt();
            if disconnected {
                let status_line = disconnected_status_line(&snapshot);
                status.set_text(&status_line);
                status.set_tooltip_text(Some(&status_line));
            } else {
                let status_line = snapshot.to_line();
                status.set_text(&status_line);
                status.set_tooltip_text(Some(&status_line));
            }
            sync_terminal_recovery_state(&shell, &status, &recovery_button, disconnected);
            sync_status_severity(
                &status,
                if disconnected {
                    None
                } else {
                    snapshot.helper_severity
                },
            );
        };
        update();
        let update = Rc::new(update);

        {
            let update = update.clone();
            terminal.connect_window_title_changed(move |_| update());
        }
        {
            let update = update.clone();
            terminal.connect_current_directory_uri_changed(move |_| update());
        }
        {
            let update = update.clone();
            terminal.connect_contents_changed(move |_| update());
        }
        terminal.connect_child_exited(move |_, _| {
            update();
        });
    }

    install_dropped_file_target(&shell, &session, show_recovery_prompt.clone());

    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    {
        let tile_id = tile.id.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(
                &TileDragPayload::new(tile_id.clone()).to_value(),
            ))
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_begin(move |_, _| {
            shell.add_css_class("is-dragging");
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            shell.remove_css_class("is-dragging");
        });
    }
    left.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(TileDragPayload::static_type(), gdk::DragAction::MOVE);
    {
        let shell = shell.clone();
        drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::MOVE
        });
    }
    {
        let shell = shell.clone();
        drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let target_id = tile.id.clone();
        let on_swap = on_swap.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");

            let Ok(payload) = value.get::<TileDragPayload>() else {
                return false;
            };
            let dragged_id = payload.into_tile_id();
            on_swap(dragged_id, target_id.clone());
            true
        });
    }
    shell.add_controller(drop_target);

    TileView {
        widget: shell.upcast(),
        session,
        tile: tile.clone(),
        close_button,
    }
}

fn install_dropped_file_target(
    shell: &gtk::Box,
    session: &TerminalSession,
    show_recovery_prompt: Rc<dyn Fn()>,
) {
    let file_list_drop_target =
        gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    {
        let shell = shell.clone();
        file_list_drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        file_list_drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        file_list_drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");

            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };

            let paths = local_paths_from_gio_files(files.files());

            paste_dropped_file_paths(&session, &paths, show_recovery_prompt.as_ref())
        });
    }
    shell.add_controller(file_list_drop_target);

    let single_file_drop_target =
        gtk::DropTarget::new(gtk::gio::File::static_type(), gdk::DragAction::COPY);
    {
        let shell = shell.clone();
        single_file_drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        single_file_drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        single_file_drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");

            let Ok(file) = value.get::<gtk::gio::File>() else {
                return false;
            };
            let paths = local_paths_from_gio_files([file]);

            paste_dropped_file_paths(&session, &paths, show_recovery_prompt.as_ref())
        });
    }
    shell.add_controller(single_file_drop_target);

    let uri_list_formats =
        gdk::ContentFormats::new(&["text/uri-list", "x-special/gnome-copied-files"]);
    let uri_list_drop_target =
        gtk::DropTargetAsync::new(Some(uri_list_formats), gdk::DragAction::COPY);
    uri_list_drop_target
        .connect_accept(|_, drop| drop_formats_can_contain_uri_list(&drop.formats()));
    {
        let shell = shell.clone();
        uri_list_drop_target.connect_drag_enter(move |_, _, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        uri_list_drop_target.connect_drag_leave(move |_, _| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        uri_list_drop_target.connect_drop(move |_, drop, _, _| {
            let shell = shell.clone();
            let session = session.clone();
            let show_recovery_prompt = show_recovery_prompt.clone();
            let drop = drop.clone();
            let drop_for_finish = drop.clone();
            drop.read_async(
                &["text/uri-list", "x-special/gnome-copied-files"],
                glib::Priority::DEFAULT,
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    shell.remove_css_class("is-drop-target");
                    let Ok((stream, _mime_type)) = result else {
                        drop_for_finish.finish(gdk::DragAction::empty());
                        return;
                    };
                    glib::MainContext::default().spawn_local(async move {
                        let Ok(text) = read_drop_stream_text(stream).await else {
                            drop_for_finish.finish(gdk::DragAction::empty());
                            return;
                        };
                        let paths = local_paths_from_uri_list_text(&text);
                        let accepted = paste_dropped_file_paths(
                            &session,
                            &paths,
                            show_recovery_prompt.as_ref(),
                        );
                        drop_for_finish.finish(if accepted {
                            gdk::DragAction::COPY
                        } else {
                            gdk::DragAction::empty()
                        });
                    });
                },
            );
            true
        });
    }
    shell.add_controller(uri_list_drop_target);
}

fn drop_formats_can_contain_uri_list(formats: &gdk::ContentFormats) -> bool {
    formats.contain_mime_type("text/uri-list")
        || formats.contain_mime_type("x-special/gnome-copied-files")
}

async fn read_drop_stream_text(stream: gtk::gio::InputStream) -> Result<String, gtk::glib::Error> {
    let mut bytes = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(16 * 1024, glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(chunk.as_ref());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn local_paths_from_gio_files<I>(files: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = gtk::gio::File>,
{
    files.into_iter().filter_map(|file| file.path()).collect()
}

fn local_paths_from_uri_list_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .filter(|line| !line.eq_ignore_ascii_case("copy") && !line.eq_ignore_ascii_case("cut"))
        .filter_map(local_path_from_drop_text_line)
        .collect()
}

fn local_path_from_drop_text_line(line: &str) -> Option<PathBuf> {
    if line.starts_with("file://") {
        gtk::gio::File::for_uri(line).path()
    } else if line.starts_with('/') {
        Some(PathBuf::from(line))
    } else {
        None
    }
}

fn paste_dropped_file_paths(
    session: &TerminalSession,
    paths: &[PathBuf],
    show_recovery_prompt: &dyn Fn(),
) -> bool {
    if session.needs_recovery_prompt() {
        show_recovery_prompt();
        true
    } else {
        session.paste_dropped_paths(paths)
    }
}

fn status_snapshot_for_terminal(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    terminal: &vte4::Terminal,
    session: &TerminalSession,
    output_helpers: &CompiledOutputHelpers,
) -> PaneStatusSnapshot {
    let mut snapshot = initial_status_snapshot(tile, workspace_root, assets);
    if let Some(uri) = terminal.current_directory_uri() {
        snapshot.location_label = short_location_from_uri(uri.as_str());
    } else if let Some(title) = terminal.window_title() {
        snapshot.location_label = title.to_string();
    }
    let (matches, shell_label) = if let Some(title) = terminal.window_title() {
        (output_helpers.scan(title.as_str()), title.to_string())
    } else {
        let recent = session.recent_output(32);
        let matches = output_helpers.scan(&recent);
        let shell_label = if recent.trim().is_empty() {
            tile.agent_label.clone()
        } else {
            recent
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .unwrap_or(&tile.agent_label)
                .to_string()
        };
        (matches, shell_label)
    };
    snapshot.shell_label = shell_label;
    let (helper_label, helper_severity) = helper_summary_text(&matches);
    snapshot.helper_label = helper_label;
    snapshot.helper_severity = helper_severity;
    snapshot
}

fn short_location_from_uri(uri: &str) -> String {
    let trimmed = uri.trim_start_matches("file://");
    PathBuf::from(trimmed)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| trimmed.to_string())
}

fn sync_status_severity(status: &gtk::Label, severity: Option<OutputSeverity>) {
    status.remove_css_class("helper-info");
    status.remove_css_class("helper-warning");
    status.remove_css_class("helper-error");
    match severity {
        Some(OutputSeverity::Info) => status.add_css_class("helper-info"),
        Some(OutputSeverity::Warning) => status.add_css_class("helper-warning"),
        Some(OutputSeverity::Error) => status.add_css_class("helper-error"),
        None => {}
    }
}

fn sync_terminal_recovery_state(
    shell: &gtk::Box,
    status: &gtk::Label,
    recovery_button: &gtk::Button,
    disconnected: bool,
) {
    if disconnected {
        shell.add_css_class("is-disconnected");
        status.add_css_class("recovery-chip");
        status.set_tooltip_text(Some(
            "This pane exited. Press Enter or type to choose Reconnect Session or Open Local Shell.",
        ));
        recovery_button.set_visible(true);
        recovery_button.set_sensitive(true);
    } else {
        shell.remove_css_class("is-disconnected");
        status.remove_css_class("recovery-chip");
        status.set_tooltip_text(None);
        recovery_button.set_visible(false);
        recovery_button.set_sensitive(false);
    }
}

fn disconnected_status_line(snapshot: &PaneStatusSnapshot) -> String {
    let mut parts = vec!["Disconnected".to_string()];
    if !snapshot.connection_label.trim().is_empty() {
        parts.push(snapshot.connection_label.trim().to_string());
    }
    parts.push("Reconnect or open local shell".into());
    parts.join("  •  ")
}

fn install_terminal_context_menu(
    terminal: &vte4::Terminal,
    session: &TerminalSession,
    show_recovery_prompt: Rc<dyn Fn()>,
) {
    let context_menu = terminal_context_menu::install(
        terminal,
        TerminalContextMenuInput {
            grab_focus: Rc::new({
                let terminal = terminal.clone();
                move || {
                    terminal.grab_focus();
                }
            }),
            has_selection: Rc::new({
                let session = session.clone();
                move || session.has_selection()
            }),
            can_paste: Rc::new({
                let session = session.clone();
                move || session.has_active_process() || session.needs_recovery_prompt()
            }),
            can_reconnect: Rc::new(|| true),
            can_open_local_shell: Rc::new({
                let session = session.clone();
                move || session.needs_recovery_prompt()
            }),
            copy: Rc::new({
                let session = session.clone();
                move || {
                    session.copy_selection_to_clipboard();
                }
            }),
            paste: Rc::new({
                let session = session.clone();
                let show_recovery_prompt = show_recovery_prompt.clone();
                move || {
                    if session.needs_recovery_prompt() {
                        show_recovery_prompt();
                    } else {
                        session.paste_clipboard();
                    }
                }
            }),
            reconnect: Rc::new({
                let session = session.clone();
                move || {
                    session.reset_auto_reconnect_attempts();
                    let _ = session.reconnect();
                }
            }),
            open_local_shell: Rc::new({
                let session = session.clone();
                move || {
                    session.reset_auto_reconnect_attempts();
                    let _ = session.open_local_shell();
                }
            }),
            show_transcript: Rc::new({
                let session = session.clone();
                let terminal = terminal.clone();
                move || {
                    transcript_dialog::present(&terminal, &session.recent_transcript(240));
                }
            }),
            focus_command_input: None,
        },
    );
    {
        let copy_button = context_menu.copy_button.clone();
        terminal.connect_selection_changed(move |term| {
            copy_button.set_sensitive(term.has_selection());
        });
    }
}

fn install_terminal_recovery_key_controller(
    terminal: &vte4::Terminal,
    session: &TerminalSession,
    show_recovery_prompt: Rc<dyn Fn()>,
) {
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let session = session.clone();
    key_controller.connect_key_pressed(move |_, key, _, state| {
        if !session.needs_recovery_prompt() || !should_open_recovery_prompt_for_key(key, state) {
            return gtk::glib::Propagation::Proceed;
        }

        show_recovery_prompt();
        gtk::glib::Propagation::Stop
    });
    terminal.add_controller(key_controller);
}

fn should_open_recovery_prompt_for_key(key: gdk::Key, state: gdk::ModifierType) -> bool {
    let modifiers = state & gtk::accelerator_get_default_mod_mask();
    if matches!(
        key,
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::BackSpace | gdk::Key::Delete
    ) && (modifiers.is_empty() || modifiers == gdk::ModifierType::SHIFT_MASK)
    {
        return true;
    }

    if modifiers == (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK)
        && key
            .to_unicode()
            .is_some_and(|value| value.eq_ignore_ascii_case(&'v'))
    {
        return true;
    }

    (modifiers.is_empty() || modifiers == gdk::ModifierType::SHIFT_MASK)
        && key.to_unicode().is_some_and(|value| !value.is_control())
}

fn default_recovery_prompt_rect(terminal: &vte4::Terminal) -> gdk::Rectangle {
    gdk::Rectangle::new((terminal.allocated_width() / 2).max(1), 8, 1, 1)
}

#[cfg(test)]
mod tests {
    use super::{
        local_paths_from_gio_files, local_paths_from_uri_list_text, read_drop_stream_text,
    };
    use gtk::prelude::*;
    use std::path::PathBuf;

    #[test]
    fn extracts_local_paths_from_single_gio_file_drop_payloads() {
        let file = gtk::gio::File::for_path("/tmp/terminaltiler one.png");

        assert_eq!(
            local_paths_from_gio_files([file]),
            vec![PathBuf::from("/tmp/terminaltiler one.png")]
        );
    }

    #[test]
    fn extracts_local_paths_from_multiple_gio_file_drop_payloads() {
        let files = vec![
            gtk::gio::File::for_path("/tmp/one.png"),
            gtk::gio::File::for_path("/tmp/two words.txt"),
        ];

        assert_eq!(
            local_paths_from_gio_files(files),
            vec![
                PathBuf::from("/tmp/one.png"),
                PathBuf::from("/tmp/two words.txt"),
            ]
        );
    }

    #[test]
    fn skips_non_local_gio_file_drop_payloads() {
        let file = gtk::gio::File::for_uri("sftp://example.com/tmp/remote.txt");

        assert!(local_paths_from_gio_files([file]).is_empty());
    }

    #[test]
    fn reads_uri_list_drop_stream_asynchronously() {
        let context = gtk::glib::MainContext::default();
        let bytes = gtk::glib::Bytes::from_static(b"file:///tmp/photo%201.jpg\n");
        let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes).upcast();

        let text = context
            .block_on(read_drop_stream_text(stream))
            .expect("drop stream should be readable");

        assert_eq!(text, "file:///tmp/photo%201.jpg\n");
        assert_eq!(
            local_paths_from_uri_list_text(&text),
            vec![PathBuf::from("/tmp/photo 1.jpg")]
        );
    }

    #[test]
    fn extracts_local_paths_from_uri_list_drop_payloads() {
        let payload =
            "# source:file-manager\nfile:///tmp/photo%201.jpg\r\nfile:///home/me/second.png\r\n";

        assert_eq!(
            local_paths_from_uri_list_text(payload),
            vec![
                PathBuf::from("/tmp/photo 1.jpg"),
                PathBuf::from("/home/me/second.png"),
            ]
        );
    }

    #[test]
    fn extracts_local_paths_from_gnome_copied_files_payloads() {
        let payload = "copy\nfile:///tmp/photo.jpg\nfile:///tmp/second%20file.jpg\n";

        assert_eq!(
            local_paths_from_uri_list_text(payload),
            vec![
                PathBuf::from("/tmp/photo.jpg"),
                PathBuf::from("/tmp/second file.jpg"),
            ]
        );
    }

    #[test]
    fn ignores_remote_and_non_file_uri_list_entries() {
        let payload = "sftp://example.com/tmp/remote.jpg\nhttps://example.com/image.jpg\nfile:///tmp/local.jpg\n";

        assert_eq!(
            local_paths_from_uri_list_text(payload),
            vec![PathBuf::from("/tmp/local.jpg")]
        );
    }
}
