use std::path::{Path, PathBuf};

use gdk::prelude::StaticType;
use gtk::prelude::*;

use vte4::prelude::*;

use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;
use crate::terminal::session::TerminalSession;

pub struct TileView {
    pub widget: gtk::Widget,
    pub session: TerminalSession,
}

pub fn build(
    tile: &TileSpec,
    workspace_root: &Path,
    density: ApplicationDensity,
    zoom_steps: i32,
) -> TileView {
    let session = TerminalSession::spawn(tile, workspace_root, density, zoom_steps);

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

    let status = gtk::Label::builder()
        .label(tile.working_directory.short_label())
        .css_classes(["status-chip"])
        .build();

    header.append(&left);
    header.append(&status);
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
    install_terminal_context_menu(&terminal, &session);
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

            session.paste_dropped_paths(&paths)
        });
    }
    shell.add_controller(file_drop_target);

    TileView {
        widget: shell.upcast(),
        session,
    }
}

fn install_terminal_context_menu(terminal: &vte4::Terminal, session: &TerminalSession) {
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
        paste_button.connect_clicked(move |_| {
            session.paste_clipboard();
            popover.popdown();
        });
    }
    menu.append(&paste_button);

    popover.set_child(Some(&menu));

    let right_click = gtk::GestureClick::builder()
        .button(3)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    {
        let terminal = terminal.clone();
        let popover = popover.clone();
        right_click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            terminal.grab_focus();
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
