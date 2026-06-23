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
use crate::ui::{
    agent_setup_dialog, board_chrome, board_drag, new_task_dialog, task_detail_dialog,
};

const AGENT_TERMINAL_PLACEHOLDER: &str = "__placeholder__";

type BannerAction = (&'static str, Box<dyn Fn() + 'static>);

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
    status_banner: gtk::Box,
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
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["kanban-board"])
            .build();
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(8);
        root.set_margin_end(8);
        make_shrinkable(&root);

        root.append(&build_header(project_name));
        let status_banner = build_status_banner();
        root.append(&status_banner);

        let columns_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .homogeneous(true)
            .css_classes(["kanban-columns"])
            .build();
        make_shrinkable(&columns_row);

        let columns_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(false)
            .min_content_width(0)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["kanban-columns-scroll"])
            .build();
        make_shrinkable(&columns_scroller);

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
        columns_scroller.set_child(Some(&columns_row));
        root.append(&columns_scroller);

        let (agents_section, agents_list, terminal_stack) = build_agents_section();
        root.append(&agents_section);

        let inner = Rc::new(Inner {
            window: window.clone(),
            project_root,
            use_dark_palette,
            density,
            root,
            status_banner,
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

fn make_shrinkable(widget: &impl IsA<gtk::Widget>) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}

fn build_header(project_name: &str) -> gtk::Box {
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
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
    let lifecycle_summary = gtk::Label::builder()
        .label("0 active · 0 stale · 0 blocked · 0 review")
        .halign(gtk::Align::End)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["status-chip", "kanban-lifecycle-summary"])
        .build();
    lifecycle_summary.set_widget_name("kanban-lifecycle-summary");
    header.append(&lifecycle_summary);
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
    let run_next = icons::labeled_button(
        "Run next",
        icon_name::RUN,
        &["pill-button", "surface-button"],
    );
    run_next.set_widget_name("kanban-run-next");
    let refresh = icons::icon_button(
        icon_name::REFRESH,
        "Reload board",
        &["flat", "surface-button"],
    );
    refresh.set_widget_name("kanban-refresh");
    header.append(&new_task);
    header.append(&connect);
    header.append(&run_next);
    header.append(&refresh);
    header
}

fn build_status_banner() -> gtk::Box {
    let banner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["kanban-status-banner"])
        .build();
    banner.set_visible(false);
    banner
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

fn header_label(inner: &Rc<Inner>, name: &str) -> Option<gtk::Label> {
    let header = inner.root.first_child()?;
    let mut child = header.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == name
            && let Ok(label) = widget.clone().downcast::<gtk::Label>()
        {
            return Some(label);
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
    if let Some(button) = header_button(inner, "kanban-run-next") {
        let inner = inner.clone();
        button.connect_clicked(move |_| run_next_available(&inner));
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
    make_shrinkable(&section);
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
    make_shrinkable(&agents_list);
    section.append(&agents_list);

    let terminal_stack = gtk::Stack::builder()
        .vhomogeneous(false)
        .css_classes(["kanban-agent-terminals"])
        .build();
    terminal_stack.set_size_request(0, 240);
    terminal_stack.set_overflow(gtk::Overflow::Hidden);
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
    update_lifecycle_summary(inner, board);
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

fn update_lifecycle_summary(inner: &Rc<Inner>, board: &crate::model::board::Board) {
    let now = crate::model::board::now_epoch_secs();
    let active = board
        .tasks
        .iter()
        .filter(|task| {
            task.assignee.is_some()
                && task.heartbeat_at.or(task.claimed_at).is_some()
                && !board_service::task_is_stale(task, now)
                && task.paused.is_none()
        })
        .count();
    let stale = board
        .tasks
        .iter()
        .filter(|task| task.assignee.is_some() && board_service::task_is_stale(task, now))
        .count();
    let blocked = board
        .tasks
        .iter()
        .filter(|task| task.blocked.is_some())
        .count();
    let review = board
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InReview)
        .count();
    if let Some(label) = header_label(inner, "kanban-lifecycle-summary") {
        label.set_text(&format!(
            "{active} active · {stale} stale · {blocked} blocked · {review} review"
        ));
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
            dispatch_agent(&inner, &task_id, default_agent, default_yolo, false, true);
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
                dispatch_agent(&inner, &task_id, agent, yolo, false, true);
            });
            run_box.append(&agent_button);
        }
    }
    run_popover.set_child(Some(&run_box));
    run_menu.set_popover(Some(&run_popover));
    card.append_action(&run_menu);

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
    card.append_action(&review_menu);

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
        card.append_action(&advance);
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
    card.append_action(&delete);

    install_card_detail_click(inner, &card.widget, &task_id, default_agent, default_yolo);
    install_card_drag_source(&card.widget, &task_id);

    card.widget
}

/// Open the task-detail modal when the card body is clicked. Clicks on the action buttons
/// claim the gesture sequence first, so they don't also trigger this.
fn install_card_detail_click(
    inner: &Rc<Inner>,
    card: &gtk::Box,
    task_id: &str,
    default_agent: AgentKind,
    default_yolo: bool,
) {
    let on_changed: Rc<dyn Fn()> = {
        let inner = inner.clone();
        Rc::new(move || render(&inner))
    };
    let on_run: Rc<dyn Fn(String)> = {
        let inner = inner.clone();
        Rc::new(move |id: String| {
            dispatch_agent(&inner, &id, default_agent, default_yolo, false, true)
        })
    };
    let on_delete: Rc<dyn Fn(String)> = {
        let inner = inner.clone();
        Rc::new(move |id: String| {
            match board_store::update(&inner.project_root, |board| {
                board_service::delete_task(board, &id)
                    .is_ok()
                    .then(|| board.clone())
            }) {
                Ok(Some(board)) => render_persisted_board(&inner, &board),
                Ok(None) => {}
                Err(error) => crate::logging::error(format!("failed to save board: {error}")),
            }
        })
    };

    let gesture = gtk::GestureClick::new();
    {
        let inner = inner.clone();
        let task_id = task_id.to_string();
        gesture.connect_released(move |_, _, _, _| {
            task_detail_dialog::present(
                &inner.window,
                inner.project_root.clone(),
                task_id.clone(),
                on_changed.clone(),
                on_run.clone(),
                on_delete.clone(),
            );
        });
    }
    card.add_controller(gesture);
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

        if run.state.is_active() {
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
        }

        inner.agents_list.append(&row.widget);
    }
}

fn run_next_available(inner: &Rc<Inner>) {
    let board = board_store::load(&inner.project_root);
    let now = crate::model::board::now_epoch_secs();
    let Some(task) = board_service::next_available_work(&board, now).cloned() else {
        show_status_banner(
            inner,
            "No available work",
            "There are no unblocked To Do tasks without a fresh active lease.",
            None,
        );
        return;
    };
    let agent = board_service::implementation_agent_for_task(&board, &task);
    dispatch_agent(
        inner,
        &task.id,
        agent,
        board.automation.yolo_default,
        false,
        true,
    );
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
            dispatch_agent(
                inner,
                &task_id,
                agent,
                board.automation.yolo_default,
                false,
                false,
            );
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

fn show_status_banner(inner: &Rc<Inner>, title: &str, message: &str, action: Option<BannerAction>) {
    clear_box(&inner.status_banner);
    inner.status_banner.set_visible(true);

    let text = gtk::Label::builder()
        .label(format!("{title}: {message}"))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .css_classes(["kanban-status-banner-label"])
        .build();
    inner.status_banner.append(&text);

    if let Some((label, callback)) = action {
        let button = icons::labeled_button(label, icon_name::RUN, &["pill-button", "warning"]);
        button.set_widget_name("kanban-status-banner-action");
        button.connect_clicked(move |_| callback());
        inner.status_banner.append(&button);
    }

    let dismiss = icons::icon_button(icon_name::CLOSE, "Dismiss board status", &["flat"]);
    dismiss.set_widget_name("kanban-status-banner-dismiss");
    {
        let banner = inner.status_banner.clone();
        dismiss.connect_clicked(move |_| banner.set_visible(false));
    }
    inner.status_banner.append(&dismiss);
}

fn dispatch_agent(
    inner: &Rc<Inner>,
    task_id: &str,
    agent: AgentKind,
    yolo: bool,
    force: bool,
    allow_takeover_prompt: bool,
) {
    if agent == AgentKind::Claude
        && let Err(error) = agent_config::connect_claude(&inner.project_root)
    {
        show_status_banner(
            inner,
            "MCP setup failed",
            &format!("Could not prepare {} MCP config: {error}", agent.label()),
            None,
        );
        return;
    }

    let task = match board_store::update(&inner.project_root, |board| {
        board_service::start_work(board, task_id, agent.assignee_id(), None, force)
            .map(|transition| transition.task.clone())
    }) {
        Ok(Ok(task)) => task,
        Ok(Err(board_service::BoardError::OwnershipConflict(conflict))) => {
            let message = format!(
                "Task is already active for '{}'. Use takeover only if that run is abandoned.",
                conflict.current_assignee
            );
            let action = allow_takeover_prompt.then(|| {
                let takeover_inner = inner.clone();
                let takeover_task_id = task_id.to_string();
                (
                    "Take over and run",
                    Box::new(move || {
                        dispatch_agent(&takeover_inner, &takeover_task_id, agent, yolo, true, true)
                    }) as Box<dyn Fn() + 'static>,
                )
            });
            show_status_banner(inner, "Claim conflict", &message, action);
            return;
        }
        Ok(Err(board_service::BoardError::TaskNotFound(_))) => {
            show_status_banner(
                inner,
                "No task",
                "That task is no longer on the board.",
                None,
            );
            return;
        }
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
    if reviewer == AgentKind::Claude {
        let _ = agent_config::connect_claude(&inner.project_root);
    }
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
