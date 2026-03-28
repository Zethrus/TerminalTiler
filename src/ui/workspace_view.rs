use std::path::Path;

use gtk::prelude::*;

use crate::model::preset::WorkspacePreset;
use crate::ui::layout_tree;

#[derive(Clone)]
pub struct WorkspaceRuntime {
    sessions: Vec<crate::terminal::session::TerminalSession>,
}

impl WorkspaceRuntime {
    pub fn terminate_all(&self, reason: &str) {
        for session in &self.sessions {
            session.terminate(reason);
        }
    }
}

pub struct WorkspaceView {
    pub widget: gtk::Widget,
    pub runtime: WorkspaceRuntime,
}

pub fn build(preset: &WorkspacePreset, workspace_root: &Path) -> WorkspaceView {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    // Workspace summary header
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["workspace-summary"])
        .build();

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let name_label = gtk::Label::builder()
        .label(&preset.name)
        .halign(gtk::Align::Start)
        .css_classes(["workspace-summary-name"])
        .build();
    left.append(&name_label);

    if !preset.root_label.is_empty() {
        let root_label = gtk::Label::builder()
            .label(&preset.root_label)
            .halign(gtk::Align::Start)
            .css_classes(["workspace-summary-subtitle"])
            .build();
        left.append(&root_label);
    }

    let path_label = gtk::Label::builder()
        .label(workspace_root.display().to_string())
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();

    summary.append(&left);
    summary.append(&path_label);
    shell.append(&summary);

    let layout = layout_tree::build(&preset.layout, workspace_root, preset.density);
    layout.widget.set_hexpand(true);
    layout.widget.set_vexpand(true);
    shell.append(&layout.widget);

    WorkspaceView {
        widget: shell.upcast(),
        runtime: WorkspaceRuntime {
            sessions: layout.sessions,
        },
    }
}
