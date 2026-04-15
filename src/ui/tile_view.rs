use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gdk::prelude::StaticType;
use gtk::prelude::*;

use vte4::prelude::*;

use crate::model::assets::{CliSnippet, OutputSeverity, PaneStatusSnapshot, WorkspaceAssets};
use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;
use crate::services::launch_resolution::resolve_tile_launch;
use crate::services::output_helpers::{helper_summary_text, scan_output};
use crate::services::snippets::resolve_snippet;
use crate::terminal::session::TerminalSession;

pub struct TileView {
    pub widget: gtk::Widget,
    pub session: TerminalSession,
    pub tile: TileSpec,
    pub close_button: gtk::Button,
}

pub fn build(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    use_dark_palette: bool,
    density: ApplicationDensity,
    zoom_steps: i32,
    snippets_provider: Rc<dyn Fn() -> Vec<CliSnippet>>,
    on_swap: Rc<dyn Fn(String, String)>,
    on_close: Rc<dyn Fn(String)>,
    can_close: bool,
) -> TileView {
    let session = TerminalSession::spawn(
        tile,
        workspace_root,
        assets,
        use_dark_palette,
        density,
        zoom_steps,
    );

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .css_classes(["terminal-card", tile.accent_class.as_str()])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["terminal-header"])
        .build();

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    left.set_tooltip_text(Some("Drag this header to swap terminal positions"));

    let badge = gtk::Label::builder()
        .label(&tile.agent_label)
        .halign(gtk::Align::Start)
        .css_classes(["agent-badge"])
        .build();
    let title = gtk::Label::builder()
        .label(&tile.title)
        .halign(gtk::Align::Start)
        .css_classes(["tile-title"])
        .build();

    left.append(&badge);
    left.append(&title);
    if !tile.pane_groups.is_empty() {
        left.append(
            &gtk::Label::builder()
                .label(tile.pane_groups.join(", "))
                .halign(gtk::Align::Start)
                .tooltip_text(format!("Pane groups: {}", tile.pane_groups.join(", ")))
                .css_classes(["status-chip", "muted-chip"])
                .build(),
        );
    }

    let status = gtk::Label::builder()
        .label(
            initial_status_snapshot(tile, workspace_root, assets)
                .to_line()
                .trim(),
        )
        .css_classes(["status-chip"])
        .build();

    let recovery_button = build_header_icon_button("system-run-symbolic", "Recover pane");
    recovery_button.add_css_class("tile-recovery-action");
    recovery_button.set_visible(false);
    recovery_button.set_sensitive(false);

    let snippet_button = build_header_icon_button("insert-text-symbolic", "Run CLI snippet");
    snippet_button.add_css_class("tile-snippet-action");

    let close_button = build_header_icon_button(
        "window-close-symbolic",
        if can_close {
            "Close tile"
        } else {
            "Cannot close the last tile"
        },
    );
    close_button.set_sensitive(can_close);
    {
        let tile_id = tile.id.clone();
        let on_close = on_close.clone();
        close_button.connect_clicked(move |_| {
            on_close(tile_id.clone());
        });
    }

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&status);
    actions.append(&recovery_button);
    actions.append(&snippet_button);
    actions.append(&close_button);

    header.append(&left);
    header.append(&actions);
    shell.append(&header);

    let terminal_frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["terminal-frame"])
        .build();

    let terminal = session.widget();
    terminal.add_css_class("terminal-surface");

    let recovery_popover = build_terminal_recovery_popover(&terminal, &session);
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

    let snippet_popover = build_snippet_popover(
        &snippet_button,
        snippets_provider.clone(),
        &session,
        show_recovery_prompt.clone(),
    );
    {
        let snippet_popover = snippet_popover.clone();
        let snippets_provider = snippets_provider.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        snippet_button.connect_clicked(move |_| {
            if session.needs_recovery_prompt() {
                snippet_popover.popdown();
                show_recovery_prompt();
                return;
            }
            refresh_snippet_list(
                &snippet_popover,
                Rc::new(snippets_provider()),
                session.clone(),
                show_recovery_prompt.clone(),
            );
            if snippet_popover.is_visible() {
                snippet_popover.popdown();
            } else {
                snippet_popover.popup();
            }
        });
    }

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
        let workspace_root = workspace_root.to_path_buf();
        let assets = assets.clone();
        let update = move || {
            let snapshot = status_snapshot_for_terminal(
                &tile,
                &workspace_root,
                &assets,
                &terminal_for_update,
                &session_for_update,
            );
            let disconnected = session_for_update.needs_recovery_prompt();
            if disconnected {
                status.set_text(&disconnected_status_line(&snapshot));
            } else {
                status.set_text(&snapshot.to_line());
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

    let file_drop_target =
        gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    {
        let shell = shell.clone();
        file_drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::COPY
        });
    }
    {
        let shell = shell.clone();
        file_drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        file_drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");

            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };

            let paths = files
                .files()
                .into_iter()
                .filter_map(|file| file.path())
                .collect::<Vec<PathBuf>>();

            if session.needs_recovery_prompt() {
                show_recovery_prompt();
                true
            } else {
                session.paste_dropped_paths(&paths)
            }
        });
    }
    shell.add_controller(file_drop_target);

    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    {
        let tile_id = tile.id.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&tile_id.to_value()))
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

    let drop_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
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

            let Ok(dragged_id) = value.get::<String>() else {
                return false;
            };
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

fn build_header_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .focus_on_click(false)
        .css_classes(["flat", "tile-header-action", "tile-header-close"])
        .build();
    button.set_tooltip_text(Some(tooltip));
    if let Some(img) = button.first_child() {
        let _ = img.pango_context();
    }
    button
}

fn build_snippet_popover(
    button: &gtk::Button,
    snippets_provider: Rc<dyn Fn() -> Vec<CliSnippet>>,
    session: &TerminalSession,
    show_recovery_prompt: Rc<dyn Fn()>,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("snippet-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(button);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    popover.set_child(Some(&content));

    refresh_snippet_list(
        &popover,
        Rc::new(snippets_provider()),
        session.clone(),
        show_recovery_prompt,
    );
    popover
}

fn refresh_snippet_list(
    popover: &gtk::Popover,
    snippets: Rc<Vec<CliSnippet>>,
    session: TerminalSession,
    show_recovery_prompt: Rc<dyn Fn()>,
) {
    let Some(content) = popover
        .child()
        .and_then(|child| child.downcast::<gtk::Box>().ok())
    else {
        return;
    };
    clear_box(&content);
    content.append(
        &gtk::Label::builder()
            .label("CLI Snippets")
            .halign(gtk::Align::Start)
            .css_classes(["tile-header-popover-label"])
            .build(),
    );

    if snippets.is_empty() {
        content.append(
            &gtk::Label::builder()
                .label("No snippets configured yet. Add them in Assets.")
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["snippet-empty-state"])
                .build(),
        );
        return;
    }

    for snippet in snippets.iter().cloned() {
        let button = build_snippet_button(&snippet);
        let popover = popover.clone();
        let snippets = snippets.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        let form_content = content.clone();
        button.connect_clicked(move |_| {
            if snippet.variables.is_empty() {
                let variables = HashMap::new();
                let _ = execute_snippet(
                    &snippet,
                    &variables,
                    &session,
                    &popover,
                    show_recovery_prompt.clone(),
                );
            } else {
                show_snippet_variable_form(
                    &form_content,
                    snippet.clone(),
                    snippets.clone(),
                    session.clone(),
                    popover.clone(),
                    show_recovery_prompt.clone(),
                );
            }
        });
        content.append(&button);
    }
}

fn show_snippet_variable_form(
    content: &gtk::Box,
    snippet: CliSnippet,
    snippets: Rc<Vec<CliSnippet>>,
    session: TerminalSession,
    popover: gtk::Popover,
    show_recovery_prompt: Rc<dyn Fn()>,
) {
    clear_box(content);

    content.append(
        &gtk::Label::builder()
            .label(&snippet.name)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    if !snippet.description.trim().is_empty() {
        content.append(
            &gtk::Label::builder()
                .label(&snippet.description)
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["field-hint"])
                .build(),
        );
    }

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["snippet-variable-form"])
        .build();
    let mut fields = Vec::new();
    for variable in &snippet.variables {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        row.append(
            &gtk::Label::builder()
                .label(&variable.label)
                .halign(gtk::Align::Start)
                .css_classes(["tile-header-popover-label"])
                .build(),
        );
        if !variable.description.trim().is_empty() {
            row.append(
                &gtk::Label::builder()
                    .label(&variable.description)
                    .halign(gtk::Align::Start)
                    .wrap(true)
                    .css_classes(["field-hint"])
                    .build(),
            );
        }
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .text(&variable.default_value)
            .placeholder_text(&variable.id)
            .build();
        row.append(&entry);
        form.append(&row);
        fields.push((variable.id.clone(), entry));
    }
    content.append(&form);

    let feedback = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["snippet-error"])
        .build();
    content.append(&feedback);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let back_button = gtk::Button::builder()
        .label("Back")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    {
        let popover = popover.clone();
        let snippets = snippets.clone();
        let session = session.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        back_button.connect_clicked(move |_| {
            refresh_snippet_list(
                &popover,
                snippets.clone(),
                session.clone(),
                show_recovery_prompt.clone(),
            );
        });
    }
    actions.append(&back_button);

    let run_button = gtk::Button::builder()
        .label("Run")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    {
        let snippet = snippet.clone();
        let popover = popover.clone();
        let session = session.clone();
        let feedback = feedback.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        let fields = fields.clone();
        run_button.connect_clicked(move |_| {
            let variables = fields
                .iter()
                .map(|(id, entry)| (id.clone(), entry.text().to_string()))
                .collect::<HashMap<_, _>>();
            if let Err(error) = execute_snippet(
                &snippet,
                &variables,
                &session,
                &popover,
                show_recovery_prompt.clone(),
            ) {
                feedback.set_text(&error);
                feedback.set_visible(true);
            }
        });
    }
    actions.append(&run_button);
    content.append(&actions);
}

fn build_snippet_button(snippet: &CliSnippet) -> gtk::Button {
    let button = gtk::Button::builder()
        .focus_on_click(false)
        .css_classes(["flat", "snippet-list-item"])
        .build();
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .build();
    shell.append(
        &gtk::Label::builder()
            .label(&snippet.name)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["snippet-name"])
            .build(),
    );

    let mut summary_parts = Vec::new();
    if !snippet.description.trim().is_empty() {
        summary_parts.push(snippet.description.trim().to_string());
    }
    if !snippet.tags.is_empty() {
        summary_parts.push(format!("#{}", snippet.tags.join(" #")));
    }
    if !snippet.variables.is_empty() {
        summary_parts.push(format!("{} args", snippet.variables.len()));
    }
    if !summary_parts.is_empty() {
        shell.append(
            &gtk::Label::builder()
                .label(summary_parts.join("  •  "))
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["snippet-description"])
                .build(),
        );
    }

    button.set_child(Some(&shell));
    button
}

fn execute_snippet(
    snippet: &CliSnippet,
    variables: &HashMap<String, String>,
    session: &TerminalSession,
    popover: &gtk::Popover,
    show_recovery_prompt: Rc<dyn Fn()>,
) -> Result<(), String> {
    if session.needs_recovery_prompt() {
        popover.popdown();
        show_recovery_prompt();
        return Ok(());
    }

    let command = resolve_snippet(snippet, variables)?;
    if session.send_text(&command) {
        popover.popdown();
        Ok(())
    } else {
        Err("This pane is not ready to receive input.".into())
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn initial_status_snapshot(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
) -> PaneStatusSnapshot {
    let connection_label = resolve_tile_launch(tile, workspace_root, assets)
        .map(|resolved| resolved.connection_label)
        .unwrap_or_else(|_| "launch-error".into());
    PaneStatusSnapshot {
        connection_label,
        location_label: tile.working_directory.short_label(),
        shell_label: tile.agent_label.clone(),
        helper_label: String::new(),
        helper_severity: None,
    }
}

fn status_snapshot_for_terminal(
    tile: &TileSpec,
    workspace_root: &Path,
    assets: &WorkspaceAssets,
    terminal: &vte4::Terminal,
    session: &TerminalSession,
) -> PaneStatusSnapshot {
    let mut snapshot = initial_status_snapshot(tile, workspace_root, assets);
    if let Some(uri) = terminal.current_directory_uri() {
        snapshot.location_label = short_location_from_uri(uri.as_str());
    } else if let Some(title) = terminal.window_title() {
        snapshot.location_label = title.to_string();
    }
    let (matches, shell_label) = if let Some(title) = terminal.window_title() {
        (
            scan_output(&tile.output_helpers, title.as_str()),
            title.to_string(),
        )
    } else {
        let recent = session.recent_output(32);
        let matches = scan_output(&tile.output_helpers, &recent);
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
    let popover = gtk::Popover::new();
    popover.add_css_class("terminal-context-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(terminal);

    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .css_classes(["terminal-context-menu"])
        .build();

    let copy_button = build_terminal_context_button("Copy", Some("Ctrl+Shift+C"));
    copy_button.set_sensitive(session.has_selection());
    {
        let session = session.clone();
        let popover = popover.clone();
        copy_button.connect_clicked(move |_| {
            session.copy_selection_to_clipboard();
            popover.popdown();
        });
    }
    {
        let copy_button = copy_button.clone();
        terminal.connect_selection_changed(move |term| {
            copy_button.set_sensitive(term.has_selection());
        });
    }
    menu.append(&copy_button);

    let paste_button = build_terminal_context_button("Paste", Some("Ctrl+Shift+V"));
    {
        let session = session.clone();
        let popover = popover.clone();
        let show_recovery_prompt = show_recovery_prompt.clone();
        paste_button.connect_clicked(move |_| {
            if session.needs_recovery_prompt() {
                show_recovery_prompt();
            } else {
                session.paste_clipboard();
            }
            popover.popdown();
        });
    }
    menu.append(&paste_button);

    let reconnect_button = build_terminal_context_button("Reconnect", None);
    {
        let session = session.clone();
        let popover = popover.clone();
        reconnect_button.connect_clicked(move |_| {
            session.reset_auto_reconnect_attempts();
            let _ = session.reconnect();
            popover.popdown();
        });
    }
    menu.append(&reconnect_button);

    let local_shell_button = build_terminal_context_button("Open Local Shell", None);
    {
        let session = session.clone();
        let popover = popover.clone();
        local_shell_button.connect_clicked(move |_| {
            session.reset_auto_reconnect_attempts();
            let _ = session.open_local_shell();
            popover.popdown();
        });
    }
    menu.append(&local_shell_button);

    let transcript_button = build_terminal_context_button("Show Transcript", None);
    {
        let session = session.clone();
        let popover = popover.clone();
        let terminal = terminal.clone();
        transcript_button.connect_clicked(move |_| {
            popover.popdown();
            present_transcript_dialog(&terminal, &session.recent_transcript(240));
        });
    }
    menu.append(&transcript_button);

    popover.set_child(Some(&menu));

    let right_click = gtk::GestureClick::builder()
        .button(3)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    {
        let terminal = terminal.clone();
        let popover = popover.clone();
        let session = session.clone();
        let paste_button = paste_button.clone();
        let local_shell_button = local_shell_button.clone();
        right_click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            terminal.grab_focus();
            paste_button
                .set_sensitive(session.has_active_process() || session.needs_recovery_prompt());
            local_shell_button.set_sensitive(session.needs_recovery_prompt());
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            popover.popup();
        });
    }
    terminal.add_controller(right_click);
}

fn build_terminal_recovery_popover(
    terminal: &vte4::Terminal,
    session: &TerminalSession,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("terminal-recovery-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(terminal);

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    shell.append(
        &gtk::Label::builder()
            .label("Session ended")
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    shell.append(
        &gtk::Label::builder()
            .label("Reconnect the configured session or open a local shell in this pane.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let reconnect_button = gtk::Button::builder()
        .label("Reconnect Session")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    {
        let session = session.clone();
        let popover = popover.clone();
        reconnect_button.connect_clicked(move |_| {
            session.reset_auto_reconnect_attempts();
            let _ = session.reconnect();
            popover.popdown();
        });
    }
    shell.append(&reconnect_button);

    let local_shell_button = gtk::Button::builder()
        .label("Open Local Shell")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    {
        let session = session.clone();
        let popover = popover.clone();
        local_shell_button.connect_clicked(move |_| {
            session.reset_auto_reconnect_attempts();
            let _ = session.open_local_shell();
            popover.popdown();
        });
    }
    shell.append(&local_shell_button);

    popover.set_child(Some(&shell));
    popover
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

fn build_terminal_context_button(label: &str, shortcut: Option<&str>) -> gtk::Button {
    let button = gtk::Button::builder()
        .focus_on_click(false)
        .css_classes(["flat", "terminal-context-action"])
        .build();

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["terminal-context-label"])
            .build(),
    );

    if let Some(shortcut) = shortcut {
        row.append(
            &gtk::Label::builder()
                .label(shortcut)
                .halign(gtk::Align::End)
                .css_classes(["terminal-context-shortcut"])
                .build(),
        );
    }

    button.set_child(Some(&row));
    button
}

#[allow(deprecated)]
fn present_transcript_dialog(terminal: &vte4::Terminal, transcript: &str) {
    let Some(window) = terminal
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(&window)
        .title("Recent Transcript")
        .default_width(820)
        .default_height(480)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    let area = dialog.content_area();
    area.set_spacing(12);
    area.set_margin_top(16);
    area.set_margin_bottom(16);
    area.set_margin_start(16);
    area.set_margin_end(16);
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let text = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    text.buffer().set_text(transcript);
    scroller.set_child(Some(&text));
    area.append(&scroller);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}
