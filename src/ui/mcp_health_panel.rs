//! Reusable TerminalTiler MCP health panel for board setup and Connect Agent flows.

use std::path::Path;

use adw::prelude::*;

use crate::services::agent_config::{self, McpDiagnostics};

pub(crate) struct McpHealthPanel {
    pub(crate) widget: gtk::Box,
    project_root: gtk::Label,
    board_path: gtk::Label,
    mcp_binary: gtk::Label,
    claude_state: gtk::Label,
    codex_state: gtk::Label,
}

impl McpHealthPanel {
    pub(crate) fn new(project_root: &Path) -> Self {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["config-panel", "mcp-health-panel"])
            .build();
        widget.append(
            &gtk::Label::builder()
                .label("MCP health")
                .halign(gtk::Align::Start)
                .css_classes(["eyebrow", "mcp-health-title"])
                .build(),
        );

        let project_root_label = health_label("mcp-health-project-root");
        let board_path = health_label("mcp-health-board-path");
        let mcp_binary = health_label("mcp-health-binary-path");
        let claude_state = health_label("mcp-health-claude-state");
        let codex_state = health_label("mcp-health-codex-state");

        for label in [
            &project_root_label,
            &board_path,
            &mcp_binary,
            &claude_state,
            &codex_state,
        ] {
            widget.append(label);
        }

        let panel = Self {
            widget,
            project_root: project_root_label,
            board_path,
            mcp_binary,
            claude_state,
            codex_state,
        };
        panel.refresh(project_root);
        panel
    }

    pub(crate) fn refresh(&self, project_root: &Path) {
        let diagnostics = agent_config::diagnose_mcp(project_root);
        update_labels(self, &diagnostics);
    }
}

fn health_label(css_class: &str) -> gtk::Label {
    gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["field-hint", "mcp-health-row", css_class])
        .build()
}

fn update_labels(panel: &McpHealthPanel, diagnostics: &McpDiagnostics) {
    panel.project_root.set_text(&format!(
        "Project root: {}",
        diagnostics.project_root.display()
    ));
    panel.board_path.set_text(&format!(
        "Board: {} ({})",
        diagnostics.board_path.display(),
        if diagnostics.board_exists {
            "present"
        } else {
            "not created yet"
        }
    ));
    panel.mcp_binary.set_text(&format!(
        "MCP binary [{}]: {} ({})",
        if diagnostics.mcp_binary_exists {
            "ready"
        } else {
            "missing binary"
        },
        diagnostics.mcp_binary_path.display(),
        if diagnostics.mcp_binary_exists {
            "present"
        } else {
            "PATH lookup or missing"
        }
    ));
    panel.claude_state.set_text(&format!(
        "Claude config [{}]: {} — {}",
        config_state_label(diagnostics.claude_configured, &diagnostics.claude_detail),
        diagnostics.claude_config_path.display(),
        diagnostics.claude_detail
    ));
    panel.codex_state.set_text(&format!(
        "Codex config [{}]: {} — {}",
        config_state_label(diagnostics.codex_configured, &diagnostics.codex_detail),
        diagnostics
            .codex_config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.codex_detail
    ));
}

fn config_state_label(configured: bool, detail: &str) -> &'static str {
    if configured {
        "ready"
    } else if detail.contains("does not target this project root") {
        "wrong project root"
    } else if detail.contains("not installed") || detail.contains("entry missing") {
        "missing config"
    } else {
        "needs repair"
    }
}
