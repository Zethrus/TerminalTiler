//! The Kanban board tab: a header, five status columns, and an Agents section that hosts
//! the live terminals of dispatched agents.
//!
//! Disk is the source of truth: every mutation loads the board, applies one
//! `services::board` operation, saves atomically, and re-renders. A lightweight poller
//! reloads when the file changes underneath us (e.g. an agent updating it over MCP).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use adw::prelude::*;
use gdk::prelude::StaticType;
use gtk::glib;

use crate::model::agent_run::{AgentKind, AgentRunOptions};
use crate::model::board::{Task, TaskStatus};
use crate::model::preset::ApplicationDensity;
use crate::services::agent_orchestrator::AgentOrchestrator;
use crate::services::{agent_config, board as board_service, review_dispatch};
use crate::storage::board_store;
use crate::ui::icons::{self, name as icon_name};
use crate::ui::{agent_setup_dialog, board_chrome, board_drag, new_task_dialog};

const AGENT_TERMINAL_PLACEHOLDER: &str = "__placeholder__";

struct ColumnHandles {
    status: TaskStatus,
    widget: gtk::Box,
    count_badge: gtk::Label,
    card_list: gtk::Box,
}

struct Inner {
    window: adw::ApplicationWindow,
    project_root: PathBuf,
    use_dark_palette: bool,
    density: ApplicationDensity,
    root: gtk::Box,
    columns: Vec<ColumnHandles>,
    agents_section: gtk::Box,
    agents_list: gtk::Box,
    terminal_stack: gtk::Stack,
    orchestrator: AgentOrchestrator,
    last_mtime: Cell<Option<SystemTime>>,
}

/// Handle to a board tab. Place [`BoardView::widget`] inside a tab page shell.
#[derive(Clone)]
pub struct BoardView {
    inner: Rc<Inner>,
}

impl BoardView {
    pub fn new(
        window: &adw::ApplicationWindow,
        project_root: PathBuf,
        project_name: &str,
        use_dark_palette: bool,
        density: ApplicationDensity,
    ) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["kanban-board"])
            .build();
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(8);
        root.set_margin_end(8);

        root.append(&build_header(project_name));

        let columns_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .hexpand(true)
            .vexpand(true)
            .homogeneous(true)
            .css_classes(["kanban-columns"])
            .build();
        let mut columns = Vec::new();
        for status in TaskStatus::ALL {
            let column = board_chrome::build_board_column(status);
            columns_row.append(&column.widget);
            columns.push(ColumnHandles {
                status,
                widget: column.widget,
                count_badge: column.count_badge,
                card_list: column.card_list,
            });
        }
        root.append(&columns_row);

        let (agents_section, agents_list, terminal_stack) = build_agents_section();
        root.append(&agents_section);

        let inner = Rc::new(Inner {
            window: window.clone(),
            project_root,
            use_dark_palette,
            density,
            root,
            columns,
            agents_section,
            agents_list,
            terminal_stack,
            orchestrator: AgentOrchestrator::new(),
            last_mtime: Cell::new(None),
        });

        wire_header_buttons(&inner);
        install_column_drop_targets(&inner);
        inner
            .last_mtime
            .set(board_store::mtime(&inner.project_root));
        render(&inner);
        start_poller(&inner);

        Self { inner }
    }

    pub fn widget(&self) -> gtk::Widget {
        self.inner.root.clone().upcast()
    }

    pub fn has_active_agent_processes(&self) -> bool {
        self.inner.orchestrator.has_active_processes()
    }

    pub fn terminate_agents(&self, reason: &str) {
        self.inner.orchestrator.terminate_all(reason);
    }
}

fn build_header(project_name: &str) -> gtk::Box {
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["kanban-header"])
        .build();
    header.append(
        &gtk::Label::builder()
            .label(project_name)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["kanban-board-title"])
            .build(),
    );
    // Buttons are appended/wired by name in wire_header_buttons via well-known order.
    let new_task = icons::labeled_button(
        "New Task",
        icon_name::ADD,
        &["pill-button", "suggested-action"],
    );
    new_task.set_widget_name("kanban-new-task");
    let connect = icons::labeled_button(
        "Connect Agent",
        icon_name::TERMINAL,
        &["pill-button", "surface-button"],
    );
    connect.set_widget_name("kanban-connect-agent");
    let refresh = icons::icon_button(
        icon_name::REFRESH,
        "Reload board",
        &["flat", "surface-button"],
    );
    refresh.set_widget_name("kanban-refresh");
    header.append(&new_task);
    header.append(&connect);
    header.append(&refresh);
    header
}

fn header_button(inner: &Rc<Inner>, name: &str) -> Option<gtk::Button> {
    let header = inner.root.first_child()?;
    let mut child = header.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == name
            && let Ok(button) = widget.clone().downcast::<gtk::Button>()
        {
            return Some(button);
        }
        child = widget.next_sibling();
    }
    None
}

fn wire_header_buttons(inner: &Rc<Inner>) {
    if let Some(button) = header_button(inner, "kanban-new-task") {
        let inner = inner.clone();
        button.connect_clicked(move |_| {
            let inner_for_submit = inner.clone();
            new_task_dialog::present(&inner.window, move |title, description, status| {
                match board_store::update(&inner_for_submit.project_root, |board| {
                    board_service::create_task(board, title, description, status);
                    board.clone()
                }) {
                    Ok(board) => render_persisted_board(&inner_for_submit, &board),
                    Err(error) => crate::logging::error(format!("failed to save board: {error}")),
                }
            });
        });
    }
    if let Some(button) = header_button(inner, "kanban-connect-agent") {
        let inner = inner.clone();
        button.connect_clicked(move |_| {
            agent_setup_dialog::present(&inner.window, inner.project_root.clone());
        });
    }
    if let Some(button) = header_button(inner, "kanban-refresh") {
        let inner = inner.clone();
        button.connect_clicked(move |_| render(&inner));
    }
}

fn install_column_drop_targets(inner: &Rc<Inner>) {
    for column in &inner.columns {
        let drop_target = gtk::DropTarget::new(
            board_drag::KanbanTaskDragPayload::static_type(),
            gtk::gdk::DragAction::MOVE,
        );
        {
            let column_widget = column.widget.clone();
            drop_target.connect_enter(move |_, _, _| {
                column_widget.add_css_class("is-drop-target");
                gtk::gdk::DragAction::MOVE
            });
        }
        {
            let column_widget = column.widget.clone();
            drop_target.connect_leave(move |_| {
                column_widget.remove_css_class("is-drop-target");
            });
        }
        {
            let inner = inner.clone();
            let column_widget = column.widget.clone();
            let target_status = column.status;
            drop_target.connect_drop(move |_, value, _, _| {
                column_widget.remove_css_class("is-drop-target");

                let Ok(payload) = value.get::<board_drag::KanbanTaskDragPayload>() else {
                    return false;
                };
                handle_task_drop(&inner, payload.into_task_id(), target_status)
            });
        }
        column.widget.add_controller(drop_target);
    }
}

fn build_agents_section() -> (gtk::Box, gtk::Box, gtk::Stack) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "kanban-agents-panel"])
        .build();
    section.set_visible(false);

    section.append(
        &gtk::Label::builder()
            .label("Agents")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "kanban-agents-title"])
            .build(),
    );

    let agents_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    section.append(&agents_list);

    let terminal_stack = gtk::Stack::builder()
        .vhomogeneous(false)
        .css_classes(["kanban-agent-terminals"])
        .build();
    terminal_stack.set_size_request(-1, 240);
    let placeholder = gtk::Label::builder()
        .label("Dispatch a task to an agent to see its live terminal here.")
        .css_classes(["field-hint"])
        .build();
    terminal_stack.add_named(&placeholder, Some(AGENT_TERMINAL_PLACEHOLDER));
    section.append(&terminal_stack);

    (section, agents_list, terminal_stack)
}

fn render(inner: &Rc<Inner>) {
    let mut board = board_store::load(&inner.project_root);
    dispatch_pending_auto_reviews(inner, &board);
    board = board_store::load(&inner.project_root);
    render_board(inner, &board);
    render_agents(inner);
}

fn render_board(inner: &Rc<Inner>, board: &crate::model::board::Board) {
    for column in &inner.columns {
        clear_box(&column.card_list);
        let tasks = board_service::tasks_by_status(board, column.status);
        column.count_badge.set_text(&tasks.len().to_string());
        if tasks.is_empty() {
            column.card_list.append(&board_chrome::build_empty_state());
            continue;
        }
        for task in tasks {
            column.card_list.append(&build_card(inner, task));
        }
    }
}

fn build_card(inner: &Rc<Inner>, task: &Task) -> gtk::Box {
    let card = board_chrome::build_board_card(task);
    let task_id = task.id.clone();

    // Run-with-agent menu (default/safe/YOLO implementation runs).
    let run_menu = gtk::MenuButton::builder()
        .label("Run agent")
        .css_classes(["flat", "surface-button"])
        .build();
    let run_popover = gtk::Popover::new();
    let run_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let board = board_store::load(&inner.project_root);
    let default_yolo = board.automation.yolo_default;
    let default_agent = board_service::implementation_agent_for_task(&board, task);
    let default_label = if default_yolo {
        format!("Default: {} YOLO", default_agent.label())
    } else {
        format!("Default: {}", default_agent.label())
    };
    let default_button =
        icons::labeled_button(&default_label, icon_name::RUN, &["flat", "surface-button"]);
    {
        let inner = inner.clone();
        let task_id = task_id.clone();
        let run_popover = run_popover.clone();
        default_button.connect_clicked(move |_| {
            run_popover.popdown();
            dispatch_agent(&inner, &task_id, default_agent, default_yolo);
        });
    }
    run_box.append(&default_button);

    for agent in AgentKind::ALL {
        for (label, yolo) in [
            (agent.label().to_string(), false),
            (format!("{} YOLO", agent.label()), true),
        ] {
            let agent_button =
                icons::labeled_button(&label, icon_name::RUN, &["flat", "surface-button"]);
            let inner = inner.clone();
            let task_id = task_id.clone();
            let run_popover = run_popover.clone();
            agent_button.connect_clicked(move |_| {
                run_popover.popdown();
                dispatch_agent(&inner, &task_id, agent, yolo);
            });
            run_box.append(&agent_button);
        }
    }
    run_popover.set_child(Some(&run_box));
    run_menu.set_popover(Some(&run_popover));
    card.actions.append(&run_menu);

    // Manual review/re-review menu (normal and YOLO review runs).
    let review_label = if task.review.last_started_at.is_some() {
        "Re-run review"
    } else {
        "Run review"
    };
    let review_menu = gtk::MenuButton::builder()
        .label(review_label)
        .css_classes(["flat", "surface-button"])
        .build();
    let review_popover = gtk::Popover::new();
    let review_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let default_reviewer = board_service::reviewer_for_task(&board, task);
    let default_review_label = if default_yolo {
        format!("Default review: {} YOLO", default_reviewer.label())
    } else {
        format!("Default review: {}", default_reviewer.label())
    };
    let default_review = icons::labeled_button(
        &default_review_label,
        icon_name::SEARCH,
        &["flat", "surface-button"],
    );
    {
        let inner = inner.clone();
        let task_id = task_id.clone();
        let review_popover = review_popover.clone();
        default_review.connect_clicked(move |_| {
            review_popover.popdown();
            dispatch_review(
                &inner,
                &task_id,
                Some(default_reviewer),
                Some(default_yolo),
                true,
            );
        });
    }
    review_box.append(&default_review);
    for agent in AgentKind::ALL {
        for (label, yolo) in [
            (format!("{} review", agent.label()), false),
            (format!("{} YOLO review", agent.label()), true),
        ] {
            let review_button =
                icons::labeled_button(&label, icon_name::SEARCH, &["flat", "surface-button"]);
            let inner = inner.clone();
            let task_id = task_id.clone();
            let review_popover = review_popover.clone();
            review_button.connect_clicked(move |_| {
                review_popover.popdown();
                dispatch_review(&inner, &task_id, Some(agent), Some(yolo), true);
            });
            review_box.append(&review_button);
        }
    }
    review_popover.set_child(Some(&review_box));
    review_menu.set_popover(Some(&review_popover));
    card.actions.append(&review_menu);

    // Advance to the next column.
    if let Some(next) = next_status(task.status) {
        let advance = icons::labeled_button(
            next.column_title(),
            icon_name::NEXT,
            &["flat", "surface-button"],
        );
        let inner = inner.clone();
        let task_id = task_id.clone();
        advance.connect_clicked(move |_| {
            match board_store::update(&inner.project_root, |board| {
                board_service::set_status(board, &task_id, next)
                    .is_ok()
                    .then(|| board.clone())
            }) {
                Ok(Some(board)) => render_persisted_board(&inner, &board),
                Ok(None) => {}
                Err(error) => crate::logging::error(format!("failed to save board: {error}")),
            }
        });
        card.actions.append(&advance);
    }

    // Delete.
    let delete = icons::icon_button(
        icon_name::DELETE,
        "Delete task",
        &["flat", "surface-button"],
    );
    {
        let inner = inner.clone();
        let task_id = task_id.clone();
        delete.connect_clicked(move |_| {
            match board_store::update(&inner.project_root, |board| {
                board_service::delete_task(board, &task_id)
                    .is_ok()
                    .then(|| board.clone())
            }) {
                Ok(Some(board)) => render_persisted_board(&inner, &board),
                Ok(None) => {}
                Err(error) => crate::logging::error(format!("failed to save board: {error}")),
            }
        });
    }
    card.actions.append(&delete);

    install_card_drag_source(&card.widget, &task_id);

    card.widget
}

fn install_card_drag_source(card: &gtk::Box, task_id: &str) {
    let drag_source = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::MOVE)
        .build();
    {
        let task_id = task_id.to_string();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(
                &board_drag::KanbanTaskDragPayload::new(task_id.clone()).to_value(),
            ))
        });
    }
    {
        let card = card.clone();
        drag_source.connect_drag_begin(move |_, _| {
            card.add_css_class("is-dragging");
        });
    }
    {
        let card = card.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            card.remove_css_class("is-dragging");
        });
    }
    card.add_controller(drag_source);
}

fn render_agents(inner: &Rc<Inner>) {
    let runs = inner.orchestrator.runs();
    inner.agents_section.set_visible(!runs.is_empty());
    clear_box(&inner.agents_list);

    for run in runs {
        let row = board_chrome::build_agent_run_row(&run);

        let view = icons::labeled_button(
            "View terminal",
            icon_name::TERMINAL,
            &["flat", "surface-button"],
        );
        {
            let inner = inner.clone();
            let run_id = run.id.clone();
            view.connect_clicked(move |_| {
                if inner.terminal_stack.child_by_name(&run_id).is_some() {
                    inner.terminal_stack.set_visible_child_name(&run_id);
                }
            });
        }
        row.actions.append(&view);

        let stop = icons::labeled_button("Stop", icon_name::CLOSE, &["flat", "surface-button"]);
        {
            let inner = inner.clone();
            let run_id = run.id.clone();
            stop.connect_clicked(move |_| {
                inner.orchestrator.stop(&run_id);
                render_agents(&inner);
            });
        }
        row.actions.append(&stop);

        inner.agents_list.append(&row.widget);
    }
}

fn handle_task_drop(inner: &Rc<Inner>, task_id: String, target_status: TaskStatus) -> bool {
    let board = board_store::load(&inner.project_root);
    let Some(task) = board_service::get_task(&board, &task_id).cloned() else {
        return false;
    };
    if task.status == target_status {
        return true;
    }

    match target_status {
        TaskStatus::InProgress => {
            let agent = board_service::implementation_agent_for_task(&board, &task);
            dispatch_agent(inner, &task_id, agent, board.automation.yolo_default);
            true
        }
        TaskStatus::InReview => persist_dropped_status(inner, &task_id, target_status),
        TaskStatus::Cancelled => {
            if persist_dropped_status(inner, &task_id, target_status) {
                inner
                    .orchestrator
                    .stop_task(&task_id, "kanban task moved to Cancelled");
                render_agents(inner);
                true
            } else {
                false
            }
        }
        TaskStatus::Todo | TaskStatus::Complete => {
            persist_dropped_status(inner, &task_id, target_status)
        }
    }
}

fn persist_dropped_status(inner: &Rc<Inner>, task_id: &str, status: TaskStatus) -> bool {
    match board_store::update(&inner.project_root, |board| {
        board_service::set_status(board, task_id, status)
            .is_ok()
            .then(|| board.clone())
    }) {
        Ok(Some(board)) => {
            render_persisted_board(inner, &board);
            true
        }
        Ok(None) => false,
        Err(error) => {
            crate::logging::error(format!("failed to save board: {error}"));
            false
        }
    }
}

fn dispatch_agent(inner: &Rc<Inner>, task_id: &str, agent: AgentKind, yolo: bool) {
    // Make sure the agent can reach the MCP board before it starts.
    let _ = match agent {
        AgentKind::Claude => agent_config::connect_claude(&inner.project_root),
        AgentKind::Codex => agent_config::connect_codex(&inner.project_root),
    };

    let task = match board_store::update(&inner.project_root, |board| {
        let task = board_service::get_task(board, task_id).cloned();
        // Reflect the claim immediately; the agent will also do this via MCP.
        if task.is_some() {
            let _ = board_service::claim_task(board, task_id, agent.assignee_id());
        }
        task
    }) {
        Ok(Some(task)) => task,
        Ok(None) => return,
        Err(error) => {
            crate::logging::error(format!("failed to save board: {error}"));
            return;
        }
    };
    inner
        .last_mtime
        .set(board_store::mtime(&inner.project_root));

    let dispatched = inner.orchestrator.dispatch(
        &inner.project_root,
        agent,
        &task,
        AgentRunOptions::implementation(yolo),
        inner.use_dark_palette,
        inner.density,
    );
    dispatched.terminal.set_vexpand(true);
    inner
        .terminal_stack
        .add_named(&dispatched.terminal, Some(&dispatched.run.id));
    inner
        .terminal_stack
        .set_visible_child_name(&dispatched.run.id);

    render(inner);
}

fn dispatch_review(
    inner: &Rc<Inner>,
    task_id: &str,
    requested_agent: Option<AgentKind>,
    requested_yolo: Option<bool>,
    force: bool,
) {
    let selection = match review_dispatch::claim_pending_review(
        &inner.project_root,
        task_id,
        requested_agent,
        requested_yolo,
        force,
    ) {
        Ok(Some(selection)) => selection,
        Ok(None) => return,
        Err(error) => {
            crate::logging::error(format!("failed to save board: {error}"));
            return;
        }
    };

    let task = selection.task;
    let reviewer = selection.reviewer;
    let yolo = selection.yolo;
    let _ = match reviewer {
        AgentKind::Claude => agent_config::connect_claude(&inner.project_root),
        AgentKind::Codex => agent_config::connect_codex(&inner.project_root),
    };
    inner
        .last_mtime
        .set(board_store::mtime(&inner.project_root));

    let dispatched = inner.orchestrator.dispatch(
        &inner.project_root,
        reviewer,
        &task,
        AgentRunOptions::review(yolo),
        inner.use_dark_palette,
        inner.density,
    );
    dispatched.terminal.set_vexpand(true);
    inner
        .terminal_stack
        .add_named(&dispatched.terminal, Some(&dispatched.run.id));
    inner
        .terminal_stack
        .set_visible_child_name(&dispatched.run.id);

    render(inner);
}

fn dispatch_pending_auto_reviews(inner: &Rc<Inner>, board: &crate::model::board::Board) {
    let pending_ids: Vec<String> = board
        .tasks
        .iter()
        .filter(|task| task.needs_auto_review())
        .map(|task| task.id.clone())
        .collect();

    for task_id in pending_ids {
        dispatch_review(inner, &task_id, None, None, false);
    }
}

fn render_persisted_board(inner: &Rc<Inner>, board: &crate::model::board::Board) {
    dispatch_pending_auto_reviews(inner, board);
    let board = board_store::load(&inner.project_root);
    inner
        .last_mtime
        .set(board_store::mtime(&inner.project_root));
    render_board(inner, &board);
    render_agents(inner);
}

fn start_poller(inner: &Rc<Inner>) {
    let inner = inner.clone();
    glib::timeout_add_local(Duration::from_millis(750), move || {
        if inner.root.root().is_none() {
            // Tab was closed; stop polling.
            return glib::ControlFlow::Break;
        }
        let current = board_store::mtime(&inner.project_root);
        if current != inner.last_mtime.get() {
            inner.last_mtime.set(current);
            let mut board = board_store::load(&inner.project_root);
            dispatch_pending_auto_reviews(&inner, &board);
            board = board_store::load(&inner.project_root);
            render_board(&inner, &board);
        }
        render_agents(&inner);
        glib::ControlFlow::Continue
    });
}

fn next_status(status: TaskStatus) -> Option<TaskStatus> {
    match status {
        TaskStatus::Todo => Some(TaskStatus::InProgress),
        TaskStatus::InProgress => Some(TaskStatus::InReview),
        TaskStatus::InReview => Some(TaskStatus::Complete),
        TaskStatus::Complete | TaskStatus::Cancelled => None,
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
