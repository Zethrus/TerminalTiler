use std::path::Path;

use gtk::prelude::*;

use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::ui::layout_tree;

#[derive(Clone)]
pub struct WorkspaceRuntime {
    sessions: Vec<crate::terminal::session::TerminalSession>,
}

impl WorkspaceRuntime {
    pub fn apply_density(&self, density: ApplicationDensity) {
        for session in &self.sessions {
            session.apply_density(density);
        }
    }

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
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

    // Workspace summary header
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["workspace-summary"])
        .build();

    let name_label = gtk::Label::builder()
        .label(&preset.name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["workspace-summary-name"])
        .build();

    let path_label = gtk::Label::builder()
        .label(workspace_root.display().to_string())
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();

    summary.append(&name_label);
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
