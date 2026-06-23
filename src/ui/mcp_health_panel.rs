//! Reusable TerminalTiler MCP health panel for board setup, Connect Agent flows, and
//! the main-window MCP Health modal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;

use crate::services::agent_config::{self, McpDiagnostics};
use crate::storage::board_workspace_store::BoardWorkspaceStore;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

// Status labels come from diagnostics as ready, wrong_project_root, missing_config,
// missing_binary, or needs_repair so agents and users see the same actionable state.
pub(crate) struct McpHealthPanel {
    pub(crate) widget: gtk::Box,
    overall_status: gtk::Label,
    project_root: gtk::Label,
    board_path: gtk::Label,
    process_cwd: gtk::Label,
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

        let overall_status = health_label("mcp-health-overall-status");
        let project_root_label = health_label("mcp-health-project-root");
        let board_path = health_label("mcp-health-board-path");
        let process_cwd = health_label("mcp-health-process-cwd");
        let mcp_binary = health_label("mcp-health-binary-path");
        let claude_state = health_label("mcp-health-claude-state");
        let codex_state = health_label("mcp-health-codex-state");

        for label in [
            &overall_status,
            &project_root_label,
            &board_path,
            &process_cwd,
            &mcp_binary,
            &claude_state,
            &codex_state,
        ] {
            widget.append(label);
        }

        let panel = Self {
            widget,
            overall_status,
            project_root: project_root_label,
            board_path,
            process_cwd,
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

/// Present a compact diagnostics modal for the active project and every project
/// TerminalTiler currently knows about from open tabs plus saved Kanban shortcuts.
pub(crate) fn present_modal(
    window: &adw::ApplicationWindow,
    active_project_root: PathBuf,
    open_project_roots: Vec<PathBuf>,
    board_workspace_store: BoardWorkspaceStore,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title("MCP Health");
    dialog.set_follows_content_size(false);
    dialog.set_content_width(620);
    dialog.set_content_height(560);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "mcp-health-dialog-window");

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .css_classes(["mcp-health-modal"])
        .build();

    root.append(
        &gtk::Label::builder()
            .label("TerminalTiler MCP status for the active project and saved Kanban projects.")
            .wrap(true)
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );

    let active_panel = Rc::new(McpHealthPanel::new(&active_project_root));
    root.append(&active_panel.widget);

    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(
        &gtk::Label::builder()
            .label("Known projects")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "mcp-health-projects-title"])
            .build(),
    );

    let project_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["mcp-health-project-list"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["mcp-health-project-scroller"])
        .build();
    scroller.set_child(Some(&project_list));
    root.append(&scroller);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .css_classes(["mcp-health-actions"])
        .build();
    let refresh_button = icons::labeled_button("Refresh", icon_name::REFRESH, &["pill-button"]);
    let close_button = icons::labeled_button("Close", icon_name::CLOSE, &["pill-button", "flat"]);
    actions.append(&refresh_button);
    actions.append(&close_button);
    root.append(&actions);

    dialog.set_child(Some(&root));
    dialog.set_default_widget(Some(&refresh_button));

    let render = Rc::new({
        let active_project_root = active_project_root.clone();
        let open_project_roots = open_project_roots.clone();
        let board_workspace_store = board_workspace_store.clone();
        let active_panel = active_panel.clone();
        let project_list = project_list.clone();
        move || {
            active_panel.refresh(&active_project_root);
            render_project_rows(
                &project_list,
                known_project_roots(
                    &active_project_root,
                    &open_project_roots,
                    &board_workspace_store,
                ),
                &active_project_root,
            );
        }
    });

    render();

    {
        let render = render.clone();
        refresh_button.connect_clicked(move |_| render());
    }
    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    dialog.present(Some(window));
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
    panel
        .overall_status
        .set_text(&format!("Overall status [{}]", diagnostics.status.as_str()));
    panel.project_root.set_text(&format!(
        "Active project root: {}",
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
    panel.process_cwd.set_text(&format!(
        "Process cwd: {}",
        diagnostics
            .process_cwd
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string())
    ));
    panel.mcp_binary.set_text(&format!(
        "MCP binary [{}]: {} ({})",
        if diagnostics.mcp_binary_exists {
            "ready"
        } else {
            "missing_binary"
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
        diagnostics.claude_status.as_str(),
        diagnostics.claude_config_path.display(),
        diagnostics.claude_detail
    ));
    panel.codex_state.set_text(&format!(
        "Codex config/root [{}]: {} / {} — {} (manual sessions; board-launched Codex uses project-bound overrides)",
        diagnostics.codex_status.as_str(),
        diagnostics
            .codex_config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics
            .codex_config_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.codex_detail
    ));
}

fn known_project_roots(
    active_project_root: &Path,
    open_project_roots: &[PathBuf],
    board_workspace_store: &BoardWorkspaceStore,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(active_project_root.to_path_buf());
    roots.extend(open_project_roots.iter().cloned());
    roots.extend(
        board_workspace_store
            .load()
            .into_iter()
            .map(|workspace| workspace.project_root),
    );
    dedupe_project_roots(roots)
}

fn dedupe_project_roots(project_roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = BTreeMap::<String, PathBuf>::new();
    for project_root in project_roots {
        let key_path = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.clone());
        let key = key_path.to_string_lossy().to_string();
        deduped.entry(key).or_insert(project_root);
    }
    deduped.into_values().collect()
}

fn render_project_rows(
    project_list: &gtk::Box,
    project_roots: Vec<PathBuf>,
    active_project_root: &Path,
) {
    clear_box(project_list);

    if project_roots.is_empty() {
        project_list.append(
            &gtk::Label::builder()
                .label("No saved or open projects found.")
                .halign(gtk::Align::Start)
                .css_classes(["field-hint", "mcp-health-empty-projects"])
                .build(),
        );
        return;
    }

    for project_root in project_roots {
        let diagnostics = agent_config::diagnose_mcp(&project_root);
        project_list.append(&project_row(&diagnostics, active_project_root));
    }
}

fn project_row(diagnostics: &McpDiagnostics, active_project_root: &Path) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["card", "mcp-health-project-row"])
        .build();

    let is_active = same_project_path(&diagnostics.project_root, active_project_root);
    let title = if is_active {
        format!("{} — active", diagnostics.project_root.display())
    } else {
        diagnostics.project_root.display().to_string()
    };
    row.append(
        &gtk::Label::builder()
            .label(&title)
            .halign(gtk::Align::Start)
            .wrap(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["mcp-health-project-title"])
            .build(),
    );
    row.append(&project_detail_label(&format!(
        "Board: {} ({})",
        diagnostics.board_path.display(),
        if diagnostics.board_exists {
            "present"
        } else {
            "missing"
        }
    )));
    row.append(&project_detail_label(&format!(
        "Claude [{}] · Codex [{}] · Overall [{}]",
        diagnostics.claude_status.as_str(),
        diagnostics.codex_status.as_str(),
        diagnostics.status.as_str()
    )));
    row.add_css_class(status_css_class(diagnostics.status.as_str()));
    row
}

fn project_detail_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .wrap(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["field-hint", "mcp-health-project-detail"])
        .build()
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn same_project_path(first: &Path, second: &Path) -> bool {
    let first = first.canonicalize().unwrap_or_else(|_| first.to_path_buf());
    let second = second
        .canonicalize()
        .unwrap_or_else(|_| second.to_path_buf());
    first == second
}

fn status_css_class(status: &str) -> &'static str {
    match status {
        "ready" => "mcp-health-status-ready",
        "wrong_project_root" => "mcp-health-status-wrong-project-root",
        "missing_config" => "mcp-health-status-missing-config",
        "missing_binary" => "mcp-health-status-missing-binary",
        _ => "mcp-health-status-needs-repair",
    }
}
