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

pub fn build(tile: &TileSpec, workspace_root: &Path, density: ApplicationDensity) -> TileView {
    let session = TerminalSession::spawn(tile, workspace_root, density);
    let resolved_dir = tile.working_directory.resolve(workspace_root);

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
    let meta = gtk::Label::builder()
        .label(resolved_dir.display().to_string())
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["tile-meta", "tile-directory"])
        .build();

    left.append(&badge);
    left.append(&title);
    left.append(&meta);

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
