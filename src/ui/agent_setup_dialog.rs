//! One-click "Connect Agent" modal. Registers the bundled `terminaltiler-mcp` server with
//! Claude Code (project `.mcp.json`) or Codex (`~/.codex/config.toml`).

use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;

use crate::services::agent_config;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

/// Present the connect dialog for a project root.
pub(crate) fn present(window: &adw::ApplicationWindow, project_root: PathBuf) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Connect Agent");
    dialog.set_content_width(460);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "agent-setup-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    content.append(
        &gtk::Label::builder()
            .label("Connect this project's Kanban board to an AI agent. This registers the bundled MCP server so the agent can read and update tasks.")
            .wrap(true)
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );

    content.append(
        &gtk::Label::builder()
            .label(format!(
                "Server: {}",
                agent_config::mcp_binary_path().display()
            ))
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["status-chip", "settings-meta-chip"])
            .build(),
    );

    let status_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["field-hint"])
        .build();

    let button_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let claude_button = icons::labeled_button(
        "Install to Claude",
        icon_name::TERMINAL,
        &["pill-button", "suggested-action"],
    );
    let codex_button = icons::labeled_button(
        "Install to Codex",
        icon_name::TERMINAL,
        &["pill-button", "surface-button"],
    );
    button_row.append(&claude_button);
    button_row.append(&codex_button);
    content.append(&button_row);
    content.append(&status_label);

    let report = Rc::new({
        let status_label = status_label.clone();
        move |result: Result<PathBuf, String>| {
            status_label.set_visible(true);
            match result {
                Ok(path) => {
                    status_label.remove_css_class("error-text");
                    status_label.set_text(&format!("Connected. Wrote {}", path.display()));
                }
                Err(message) => {
                    status_label.add_css_class("error-text");
                    status_label.set_text(&format!("Could not connect: {message}"));
                }
            }
        }
    });

    {
        let report = report.clone();
        let project_root = project_root.clone();
        claude_button.connect_clicked(move |_| {
            report(agent_config::connect_claude(&project_root));
        });
    }
    {
        let report = report.clone();
        codex_button.connect_clicked(move |_| {
            report(agent_config::connect_codex());
        });
    }

    dialog.set_child(Some(&content));
    dialog.present(Some(window));
}
