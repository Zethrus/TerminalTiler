//! One-click "Connect Agent" modal. Registers the bundled `terminaltiler-mcp` server with
//! Claude Code (project `.mcp.json`) or Codex (`~/.codex/config.toml`).

use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;

use crate::model::agent_run::AgentKind;
use crate::services::agent_config;
use crate::storage::board_store;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};
use crate::ui::mcp_health_panel::McpHealthPanel;

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

    let mcp_health = Rc::new(McpHealthPanel::new(&project_root));
    content.append(&mcp_health.widget);

    let status_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["field-hint"])
        .build();

    let board = board_store::load(&project_root);
    let default_agent = board
        .automation
        .default_agent
        .or(board.automation.default_reviewer)
        .unwrap_or(AgentKind::Claude);
    let default_reviewer = board
        .automation
        .default_reviewer
        .or(board.automation.default_agent)
        .unwrap_or(AgentKind::Claude);

    let automation_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel"])
        .build();
    automation_panel.append(
        &gtk::Label::builder()
            .label("Board automation")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow"])
            .build(),
    );
    automation_panel.append(
        &gtk::Label::builder()
            .label("Choose default CLIs for one-click task runs and automatic reviews. YOLO adds each CLI's unsafe/no-approval flag and is off by default.")
            .wrap(true)
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );

    let default_agent_claude = gtk::CheckButton::builder().label("Claude").build();
    let default_agent_codex = gtk::CheckButton::builder().label("Codex").build();
    default_agent_codex.set_group(Some(&default_agent_claude));
    default_agent_claude.set_active(default_agent == AgentKind::Claude);
    default_agent_codex.set_active(default_agent == AgentKind::Codex);
    automation_panel.append(&choice_row(
        "Default task agent",
        &default_agent_claude,
        &default_agent_codex,
    ));

    let default_reviewer_claude = gtk::CheckButton::builder().label("Claude").build();
    let default_reviewer_codex = gtk::CheckButton::builder().label("Codex").build();
    default_reviewer_codex.set_group(Some(&default_reviewer_claude));
    default_reviewer_claude.set_active(default_reviewer == AgentKind::Claude);
    default_reviewer_codex.set_active(default_reviewer == AgentKind::Codex);
    automation_panel.append(&choice_row(
        "Default reviewer",
        &default_reviewer_claude,
        &default_reviewer_codex,
    ));

    let yolo_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(board.automation.yolo_default)
        .build();
    let yolo_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    yolo_row.append(
        &gtk::Label::builder()
            .label("Use YOLO by default")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    yolo_row.append(&yolo_switch);
    automation_panel.append(&yolo_row);

    let save_button = icons::labeled_button(
        "Save defaults",
        icon_name::SAVE,
        &["pill-button", "surface-button"],
    );
    let save_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::End)
        .build();
    save_row.append(&save_button);
    automation_panel.append(&save_row);
    content.append(&automation_panel);

    let save_automation: Rc<dyn Fn()> = Rc::new({
        let project_root = project_root.clone();
        let status_label = status_label.clone();
        let default_agent_codex = default_agent_codex.clone();
        let default_reviewer_codex = default_reviewer_codex.clone();
        let yolo_switch = yolo_switch.clone();
        move || {
            let default_agent = if default_agent_codex.is_active() {
                AgentKind::Codex
            } else {
                AgentKind::Claude
            };
            let default_reviewer = if default_reviewer_codex.is_active() {
                AgentKind::Codex
            } else {
                AgentKind::Claude
            };
            let yolo_default = yolo_switch.is_active();
            status_label.set_visible(true);
            match board_store::update(&project_root, |board| {
                board.automation.default_agent = Some(default_agent);
                board.automation.default_reviewer = Some(default_reviewer);
                board.automation.yolo_default = yolo_default;
            }) {
                Ok(()) => {
                    status_label.remove_css_class("error-text");
                    status_label.set_text("Saved board automation defaults.");
                }
                Err(error) => {
                    status_label.add_css_class("error-text");
                    status_label.set_text(&format!("Could not save board automation: {error}"));
                }
            }
        }
    });

    {
        let save = save_automation.clone();
        save_button.connect_clicked(move |_| save());
    }

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
        let mcp_health = mcp_health.clone();
        let project_root = project_root.clone();
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
            mcp_health.refresh(&project_root);
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
        let project_root = project_root.clone();
        codex_button.connect_clicked(move |_| {
            report(agent_config::connect_codex(&project_root));
        });
    }

    dialog.set_child(Some(&content));
    dialog.present(Some(window));
}

fn choice_row(title: &str, first: &gtk::CheckButton, second: &gtk::CheckButton) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["settings-shortcut-title"])
            .build(),
    );
    row.append(first);
    row.append(second);
    row
}
