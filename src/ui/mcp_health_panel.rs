//! Reusable TerminalTiler MCP health panel for board setup, Connect Agent flows, and
//! the main-window MCP Health modal.
//!
//! The shared [`McpHealthPanel`] is display-only so the board and Connect-Agent embeds
//! stay in sync. The standalone modal ([`present_modal`]) layers one-click repair on top:
//! a project's **Fix** button writes its config through [`agent_config`] and then
//! re-diagnoses. Claude config is project-scoped, so every row can repair it safely; Codex
//! is a single global entry, so only the active project's Fix touches it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::services::agent_config::{self, McpConfigStatus, McpDiagnostics};
use crate::storage::board_workspace_store::BoardWorkspaceStore;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

/// A late-bound, shareable refresh callback. The render closure stores itself here so the
/// Fix buttons it builds can re-run diagnostics after writing config.
type SharedRefresh = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

// Status labels come from diagnostics as ready, wrong_project_root, missing_config,
// missing_binary, or needs_repair so agents and users see the same actionable state.
#[derive(Clone)]
pub(crate) struct McpHealthPanel {
    pub(crate) widget: gtk::Box,
    overall_badge: StatusBadge,
    project_root: gtk::Label,
    board_path: gtk::Label,
    process_cwd: gtk::Label,
    mcp_binary: gtk::Label,
    claude_state: gtk::Label,
    codex_state: gtk::Label,
    refresh_generation: Rc<Cell<u64>>,
}

impl McpHealthPanel {
    pub(crate) fn new(project_root: &Path) -> Self {
        let panel = Self::new_uninitialized();
        panel.refresh(project_root);
        panel
    }

    fn new_uninitialized() -> Self {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["config-panel", "mcp-health-panel"])
            .build();

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .css_classes(["mcp-health-panel-header"])
            .build();
        header.append(
            &gtk::Label::builder()
                .label("MCP health")
                .halign(gtk::Align::Start)
                .hexpand(true)
                .css_classes(["eyebrow", "mcp-health-title"])
                .build(),
        );
        let overall_badge = StatusBadge::new();
        header.append(&overall_badge.root);
        widget.append(&header);

        let project_root_label = health_label("mcp-health-project-root");
        let board_path = health_label("mcp-health-board-path");
        let process_cwd = health_label("mcp-health-process-cwd");
        let mcp_binary = health_label("mcp-health-binary-path");
        let claude_state = health_label("mcp-health-claude-state");
        let codex_state = health_label("mcp-health-codex-state");

        for (caption, value) in [
            ("Project", &project_root_label),
            ("Board", &board_path),
            ("Process", &process_cwd),
            ("Binary", &mcp_binary),
            ("Claude", &claude_state),
            ("Codex", &codex_state),
        ] {
            widget.append(&detail_row(caption, value));
        }

        Self {
            widget,
            overall_badge,
            project_root: project_root_label,
            board_path,
            process_cwd,
            mcp_binary,
            claude_state,
            codex_state,
            refresh_generation: Rc::new(Cell::new(0)),
        }
    }

    pub(crate) fn refresh(&self, project_root: &Path) {
        let generation = self.refresh_generation.get().saturating_add(1);
        self.refresh_generation.set(generation);
        let project_root = project_root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(agent_config::diagnose_mcp(&project_root));
        });
        let panel = self.clone();
        glib::timeout_add_local(Duration::from_millis(20), move || {
            match receiver.try_recv() {
                Ok(diagnostics) => {
                    if panel.refresh_generation.get() == generation {
                        update_labels(&panel, &diagnostics);
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }
}

/// Present a compact diagnostics modal for the active project and every project
/// TerminalTiler currently knows about from open tabs plus saved Kanban shortcuts.
///
/// Each project exposes a one-click **Fix** that registers the bundled MCP server and then
/// re-diagnoses, so users never have to leave the window to repair setup.
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

    let toast_overlay = adw::ToastOverlay::new();

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

    // Late-bound refresh: Fix buttons created inside the render closure re-run diagnostics
    // by calling back into the render that built them. The callback only keeps a weak
    // pointer to this cell, and the dialog clears the cell when it closes, so the refresh
    // wiring cannot keep the dialog tree alive.
    let refresh_cell: SharedRefresh = Rc::new(RefCell::new(None));
    let trigger_refresh: Rc<dyn Fn()> = {
        let refresh_cell = Rc::downgrade(&refresh_cell);
        Rc::new(move || {
            let Some(refresh_cell) = refresh_cell.upgrade() else {
                return;
            };
            let refresh = refresh_cell.borrow().clone();
            if let Some(refresh) = refresh {
                refresh();
            }
        })
    };

    let active_panel = Rc::new(McpHealthPanel::new_uninitialized());
    let active_fix = icons::labeled_button(
        "Fix",
        icon_name::APPLY,
        &["pill-button", "suggested-action", "mcp-health-fix-button"],
    );
    {
        let trigger_refresh = trigger_refresh.clone();
        let toast_overlay = toast_overlay.clone();
        let project_root = active_project_root.clone();
        active_fix.connect_clicked(move |_| {
            repair_active_project(&toast_overlay, &project_root);
            trigger_refresh();
        });
    }
    root.append(&active_project_card(&active_panel.widget, &active_fix));

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

    toast_overlay.set_child(Some(&root));
    dialog.set_child(Some(&toast_overlay));
    dialog.set_default_widget(Some(&refresh_button));

    let render = Rc::new({
        let active_project_root = active_project_root.clone();
        let open_project_roots = open_project_roots.clone();
        let board_workspace_store = board_workspace_store.clone();
        let active_panel = active_panel.clone();
        let active_fix = active_fix.clone();
        let project_list = project_list.clone();
        let toast_overlay = toast_overlay.clone();
        let trigger_refresh = trigger_refresh.clone();
        let refresh_generation = Rc::new(Cell::new(0_u64));
        move || {
            let roots = other_known_project_roots(
                &active_project_root,
                &open_project_roots,
                &board_workspace_store,
            );
            let generation = refresh_generation.get().saturating_add(1);
            refresh_generation.set(generation);
            let (sender, receiver) = std::sync::mpsc::channel();
            let active_root = active_project_root.clone();
            std::thread::spawn(move || {
                let global = agent_config::diagnose_mcp_global();
                let active = agent_config::diagnose_mcp_with_global(&active_root, &global);
                let projects = roots
                    .iter()
                    .map(|root| agent_config::diagnose_mcp_with_global(root, &global))
                    .collect::<Vec<_>>();
                let _ = sender.send((active, projects));
            });
            let refresh_generation = refresh_generation.clone();
            let active_panel = active_panel.clone();
            let active_fix = active_fix.clone();
            let project_list = project_list.clone();
            let active_project_root = active_project_root.clone();
            let toast_overlay = toast_overlay.clone();
            let trigger_refresh = trigger_refresh.clone();
            glib::timeout_add_local(Duration::from_millis(20), move || {
                match receiver.try_recv() {
                    Ok((active_diagnostics, projects)) => {
                        if refresh_generation.get() == generation {
                            update_labels(&active_panel, &active_diagnostics);
                            apply_fix_affordance(
                                &active_fix,
                                active_fix_affordance(&active_diagnostics),
                            );
                            render_project_rows(
                                &project_list,
                                projects,
                                &active_project_root,
                                &toast_overlay,
                                &trigger_refresh,
                            );
                        }
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                }
            });
        }
    });
    *refresh_cell.borrow_mut() = Some(render.clone());

    render();

    {
        let refresh_cell = refresh_cell.clone();
        dialog.connect_closed(move |_| {
            refresh_cell.borrow_mut().take();
        });
    }

    {
        let render = render.clone();
        refresh_button.connect_clicked(move |_| render());
    }
    {
        let dialog = dialog.downgrade();
        close_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    dialog.present(Some(window));
}

/// Wrap the active project's panel and its Fix action in an elevated hero card.
fn active_project_card(panel: &gtk::Box, fix_button: &gtk::Button) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["card", "mcp-health-active-card"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["mcp-health-active-header"])
        .build();
    header.append(
        &gtk::Label::builder()
            .label("Active project")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["eyebrow", "mcp-health-active-eyebrow"])
            .build(),
    );
    header.append(fix_button);
    card.append(&header);
    card.append(panel);
    card
}

/// What the Fix affordance should look like for a repair target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixAffordance {
    /// Already configured — nothing to repair.
    Hidden,
    /// A config write cannot help (the bundled binary is missing).
    Disabled(&'static str),
    /// Repairable from this window.
    Enabled,
}

const MISSING_BUNDLED_BINARY_FIX_REASON: &str = "The bundled terminaltiler-mcp binary was not found, so editing config can't help. \
Reinstall TerminalTiler to restore it.";

fn active_fix_affordance(diagnostics: &McpDiagnostics) -> FixAffordance {
    if !diagnostics.mcp_binary_exists {
        return FixAffordance::Disabled(MISSING_BUNDLED_BINARY_FIX_REASON);
    }
    if diagnostics.claude_status == McpConfigStatus::Ready
        && diagnostics.codex_status == McpConfigStatus::Ready
    {
        return FixAffordance::Hidden;
    }
    FixAffordance::Enabled
}

fn repairable_agent_fix_affordance(
    status: McpConfigStatus,
    mcp_binary_exists: bool,
) -> FixAffordance {
    if !mcp_binary_exists {
        return FixAffordance::Disabled(MISSING_BUNDLED_BINARY_FIX_REASON);
    }
    if status == McpConfigStatus::Ready {
        return FixAffordance::Hidden;
    }
    FixAffordance::Enabled
}

/// Apply the [`FixAffordance`] to an already-built button (used for the persistent active
/// project Fix, whose handler is wired once).
fn apply_fix_affordance(button: &gtk::Button, affordance: FixAffordance) {
    match affordance {
        FixAffordance::Hidden => button.set_visible(false),
        FixAffordance::Disabled(reason) => {
            button.set_visible(true);
            button.set_sensitive(false);
            button.set_tooltip_text(Some(reason));
        }
        FixAffordance::Enabled => {
            button.set_visible(true);
            button.set_sensitive(true);
            button.set_tooltip_text(None);
        }
    }
}

/// Register the bundled MCP server for the active project with both Claude (project-scoped)
/// and Codex (global), reporting the outcome through the modal's toast overlay.
fn repair_active_project(overlay: &adw::ToastOverlay, project_root: &Path) {
    let claude = agent_config::connect_claude(project_root);
    let codex = agent_config::connect_codex(project_root);
    let message = match (&claude, &codex) {
        (Ok(_), Ok(_)) => "Repaired Claude and Codex for the active project.".to_string(),
        (Err(error), _) => format!("Claude repair failed: {error}"),
        (Ok(_), Err(error)) => format!("Claude repaired; Codex repair failed: {error}"),
    };
    show_modal_toast(overlay, &message);
}

/// Register the bundled MCP server with Claude only (project-scoped `.mcp.json`).
fn repair_claude_only(overlay: &adw::ToastOverlay, project_root: &Path) {
    let message = match agent_config::connect_claude(project_root) {
        Ok(path) => format!("Repaired Claude config: {}", path.display()),
        Err(error) => format!("Repair failed: {error}"),
    };
    show_modal_toast(overlay, &message);
}

fn show_modal_toast(overlay: &adw::ToastOverlay, message: &str) {
    overlay.add_toast(adw::Toast::new(message));
}

fn health_label(css_class: &str) -> gtk::Label {
    gtk::Label::builder()
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["field-hint", "mcp-health-row", css_class])
        .build()
}

/// A caption + value row used for the panel's key/value details.
fn detail_row(caption: &str, value: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["mcp-health-detail"])
        .build();
    row.append(
        &gtk::Label::builder()
            .label(caption)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .width_chars(7)
            .xalign(0.0)
            .css_classes(["mcp-health-detail-caption"])
            .build(),
    );
    row.append(value);
    row
}

fn update_labels(panel: &McpHealthPanel, diagnostics: &McpDiagnostics) {
    panel.overall_badge.set(diagnostics.status);
    panel
        .project_root
        .set_text(&diagnostics.project_root.display().to_string());
    panel.board_path.set_text(&format!(
        "{} · {}",
        diagnostics.board_path.display(),
        if diagnostics.board_exists {
            "present"
        } else {
            "not created yet"
        }
    ));
    panel.process_cwd.set_text(
        &diagnostics
            .process_cwd
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
    );
    panel.mcp_binary.set_text(&format!(
        "{} · {}",
        diagnostics.mcp_binary_path.display(),
        if diagnostics.mcp_binary_exists {
            "present"
        } else {
            "not found"
        }
    ));
    panel.claude_state.set_text(&format!(
        "{} · {} — {}",
        humanize_status(diagnostics.claude_status),
        diagnostics.claude_config_path.display(),
        diagnostics.claude_detail
    ));
    panel.codex_state.set_text(&format!(
        "{} · {} — {} (manual sessions; board-launched Codex uses project-bound overrides)",
        humanize_status(diagnostics.codex_status),
        diagnostics
            .codex_config_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string()),
        diagnostics.codex_detail
    ));
}

/// Known project roots minus the active one (which has its own hero card above).
fn other_known_project_roots(
    active_project_root: &Path,
    open_project_roots: &[PathBuf],
    board_workspace_store: &BoardWorkspaceStore,
) -> Vec<PathBuf> {
    known_project_roots(
        active_project_root,
        open_project_roots,
        board_workspace_store,
    )
    .into_iter()
    .filter(|root| !same_project_path(root, active_project_root))
    .collect()
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
    diagnostics: Vec<McpDiagnostics>,
    active_project_root: &Path,
    toast_overlay: &adw::ToastOverlay,
    trigger_refresh: &Rc<dyn Fn()>,
) {
    clear_box(project_list);

    if diagnostics.is_empty() {
        project_list.append(
            &gtk::Label::builder()
                .label("No other saved or open projects found.")
                .halign(gtk::Align::Start)
                .css_classes(["field-hint", "mcp-health-empty-projects"])
                .build(),
        );
        return;
    }

    for diagnostics in diagnostics {
        project_list.append(&project_row(
            &diagnostics,
            active_project_root,
            toast_overlay,
            trigger_refresh,
        ));
    }
}

fn project_row(
    diagnostics: &McpDiagnostics,
    active_project_root: &Path,
    toast_overlay: &adw::ToastOverlay,
    trigger_refresh: &Rc<dyn Fn()>,
) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["card", "mcp-health-project-row"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["mcp-health-project-header"])
        .build();
    let is_active = same_project_path(&diagnostics.project_root, active_project_root);
    let title = if is_active {
        format!("{} — active", diagnostics.project_root.display())
    } else {
        diagnostics.project_root.display().to_string()
    };
    header.append(
        &gtk::Label::builder()
            .label(&title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["mcp-health-project-title"])
            .build(),
    );
    let badge = StatusBadge::new();
    badge.set(diagnostics.status);
    header.append(&badge.root);
    if let Some(fix_button) = build_row_fix_button(
        diagnostics.claude_status,
        diagnostics.mcp_binary_exists,
        &diagnostics.project_root,
        toast_overlay,
        trigger_refresh,
    ) {
        header.append(&fix_button);
    }
    row.append(&header);

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
        "Claude: {} · Codex: {}",
        humanize_status(diagnostics.claude_status),
        humanize_status(diagnostics.codex_status)
    )));
    if diagnostics.codex_status != McpConfigStatus::Ready {
        row.append(&project_detail_label(
            "Codex config is global — repair it from the active project.",
        ));
    }
    row.add_css_class(status_css_class(diagnostics.status.as_str()));
    row
}

/// Build a Claude-only Fix button for a known-project row, or `None` when Claude is
/// already configured. A missing bundled binary yields a disabled, explanatory button.
fn build_row_fix_button(
    claude_status: McpConfigStatus,
    mcp_binary_exists: bool,
    project_root: &Path,
    toast_overlay: &adw::ToastOverlay,
    trigger_refresh: &Rc<dyn Fn()>,
) -> Option<gtk::Button> {
    let button = icons::labeled_button(
        "Fix",
        icon_name::APPLY,
        &["pill-button", "mcp-health-fix-button"],
    );
    match repairable_agent_fix_affordance(claude_status, mcp_binary_exists) {
        FixAffordance::Hidden => return None,
        FixAffordance::Disabled(reason) => {
            button.set_sensitive(false);
            button.set_tooltip_text(Some(reason));
        }
        FixAffordance::Enabled => {
            let toast_overlay = toast_overlay.clone();
            let trigger_refresh = trigger_refresh.clone();
            let project_root = project_root.to_path_buf();
            button.connect_clicked(move |_| {
                repair_claude_only(&toast_overlay, &project_root);
                trigger_refresh();
            });
        }
    }
    Some(button)
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

/// A status pill (colored dot + humanized label) shared by the panel header and the
/// known-project rows.
#[derive(Clone)]
struct StatusBadge {
    root: gtk::Box,
    label: gtk::Label,
}

const BADGE_MODIFIERS: [&str; 3] = ["is-ready", "is-warn", "is-error"];

impl StatusBadge {
    fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .css_classes(["mcp-health-badge"])
            .build();
        root.append(
            &gtk::Box::builder()
                .css_classes(["mcp-health-badge-dot"])
                .build(),
        );
        let label = gtk::Label::builder()
            .css_classes(["mcp-health-badge-label"])
            .build();
        root.append(&label);
        Self { root, label }
    }

    fn set(&self, status: McpConfigStatus) {
        self.label.set_text(humanize_status(status));
        for modifier in BADGE_MODIFIERS {
            self.root.remove_css_class(modifier);
        }
        self.root.add_css_class(badge_modifier(status));
    }
}

/// Short, human-readable status used by badges and per-config rows.
fn humanize_status(status: McpConfigStatus) -> &'static str {
    match status {
        McpConfigStatus::Ready => "Ready",
        McpConfigStatus::WrongProjectRoot => "Wrong project",
        McpConfigStatus::MissingConfig => "Needs setup",
        McpConfigStatus::MissingBinary => "Binary missing",
        McpConfigStatus::NeedsRepair => "Needs repair",
    }
}

/// Badge tint modifier mirroring [`status_css_class`]: ready is the calm accent, an
/// unconfigured-but-expected state is a warning, everything else is an error.
fn badge_modifier(status: McpConfigStatus) -> &'static str {
    match status {
        McpConfigStatus::Ready => "is-ready",
        McpConfigStatus::MissingConfig => "is-warn",
        McpConfigStatus::WrongProjectRoot
        | McpConfigStatus::MissingBinary
        | McpConfigStatus::NeedsRepair => "is-error",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics(
        status: McpConfigStatus,
        claude_status: McpConfigStatus,
        codex_status: McpConfigStatus,
        mcp_binary_exists: bool,
    ) -> McpDiagnostics {
        let project_root = PathBuf::from("/tmp/terminaltiler-project");
        McpDiagnostics {
            status,
            project_root: project_root.clone(),
            board_path: project_root.join(".terminaltiler/board.json"),
            board_exists: false,
            process_cwd: Some(project_root.clone()),
            mcp_binary_path: PathBuf::from("/opt/TerminalTiler/terminaltiler-mcp"),
            mcp_binary_exists,
            claude_config_path: project_root.join(".mcp.json"),
            claude_configured: claude_status == McpConfigStatus::Ready,
            claude_detail: String::new(),
            claude_status,
            claude_bound_project_root: None,
            claude_command: None,
            claude_args: Vec::new(),
            codex_config_path: Some(PathBuf::from("/tmp/codex/config.toml")),
            codex_config_root: Some(PathBuf::from("/tmp/codex")),
            codex_configured: codex_status == McpConfigStatus::Ready,
            codex_detail: String::new(),
            codex_status,
            codex_bound_project_root: None,
            codex_command: None,
            codex_args: Vec::new(),
        }
    }

    #[test]
    fn active_fix_stays_enabled_when_aggregate_status_is_ready_but_one_agent_needs_repair() {
        let claude_ready_codex_missing = diagnostics(
            McpConfigStatus::Ready,
            McpConfigStatus::Ready,
            McpConfigStatus::MissingConfig,
            true,
        );
        assert_eq!(
            active_fix_affordance(&claude_ready_codex_missing),
            FixAffordance::Enabled
        );

        let claude_missing_codex_ready = diagnostics(
            McpConfigStatus::Ready,
            McpConfigStatus::MissingConfig,
            McpConfigStatus::Ready,
            true,
        );
        assert_eq!(
            active_fix_affordance(&claude_missing_codex_ready),
            FixAffordance::Enabled
        );
    }

    #[test]
    fn active_fix_hides_only_after_both_agent_configs_are_ready() {
        let all_ready = diagnostics(
            McpConfigStatus::Ready,
            McpConfigStatus::Ready,
            McpConfigStatus::Ready,
            true,
        );
        assert_eq!(active_fix_affordance(&all_ready), FixAffordance::Hidden);
    }

    #[test]
    fn known_project_fix_uses_claude_status_not_aggregate_readiness() {
        assert_eq!(
            repairable_agent_fix_affordance(McpConfigStatus::MissingConfig, true),
            FixAffordance::Enabled
        );
        assert_eq!(
            repairable_agent_fix_affordance(McpConfigStatus::Ready, true),
            FixAffordance::Hidden
        );
    }

    #[test]
    fn fix_is_disabled_when_bundled_mcp_binary_is_missing() {
        let missing_binary = diagnostics(
            McpConfigStatus::MissingBinary,
            McpConfigStatus::MissingConfig,
            McpConfigStatus::MissingConfig,
            false,
        );
        assert_eq!(
            active_fix_affordance(&missing_binary),
            FixAffordance::Disabled(MISSING_BUNDLED_BINARY_FIX_REASON)
        );
        assert_eq!(
            repairable_agent_fix_affordance(McpConfigStatus::MissingConfig, false),
            FixAffordance::Disabled(MISSING_BUNDLED_BINARY_FIX_REASON)
        );
    }
}
