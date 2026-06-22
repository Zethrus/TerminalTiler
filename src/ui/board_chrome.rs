//! Pure widget builders for the Kanban board: columns, cards, and empty states.
//!
//! These follow TerminalTiler's existing chrome conventions (shared `config-panel`
//! framing, `card-title`/`field-hint`/`status-chip` text classes, the alert empty-state
//! shape) so the board reads as native rather than bolted on. Interaction wiring lives in
//! `board_view`.

use adw::prelude::*;

use crate::model::agent_run::AgentRun;
use crate::model::board::{Task, TaskStatus};

/// One Kanban column: header with a live count badge plus a scrollable card list.
pub(crate) struct BoardColumnChrome {
    pub(crate) widget: gtk::Box,
    pub(crate) count_badge: gtk::Label,
    pub(crate) card_list: gtk::Box,
}

/// A rendered card, exposing an `actions` row the view fills with buttons.
pub(crate) struct BoardCardChrome {
    pub(crate) widget: gtk::Box,
    pub(crate) actions: gtk::Box,
}

/// CSS modifier class for a column/card based on its status.
pub(crate) fn status_modifier_class(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "kanban-status-todo",
        TaskStatus::InProgress => "kanban-status-in-progress",
        TaskStatus::InReview => "kanban-status-in-review",
        TaskStatus::Complete => "kanban-status-complete",
        TaskStatus::Cancelled => "kanban-status-cancelled",
    }
}

/// Build an empty column shell for a status. The caller fills `card_list`.
pub(crate) fn build_board_column(status: TaskStatus) -> BoardColumnChrome {
    let modifier = status_modifier_class(status);

    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["config-panel", "kanban-column", modifier])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["kanban-column-header"])
        .build();
    let dot = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["kanban-status-dot", modifier])
        .build();
    let title = gtk::Label::builder()
        .label(status.column_title())
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["eyebrow", "kanban-column-title"])
        .build();
    let count_badge = gtk::Label::builder()
        .label("0")
        .css_classes(["kanban-count-badge"])
        .build();
    header.append(&dot);
    header.append(&title);
    header.append(&count_badge);
    column.append(&header);

    let card_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .valign(gtk::Align::Start)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();
    scroller.set_child(Some(&card_list));
    column.append(&scroller);

    BoardColumnChrome {
        widget: column,
        count_badge,
        card_list,
    }
}

/// Build a task card. The view appends action buttons to `actions` and wires a click.
pub(crate) fn build_board_card(task: &Task) -> BoardCardChrome {
    let modifier = status_modifier_class(task.status);
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["kanban-card", modifier])
        .build();

    let title = gtk::Label::builder()
        .label(&task.title)
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["card-title", "kanban-card-title"])
        .build();
    card.append(&title);

    let description = task.description.trim();
    if !description.is_empty() {
        let body = gtk::Label::builder()
            .label(description)
            .halign(gtk::Align::Start)
            .wrap(true)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["field-hint", "kanban-card-body"])
            .build();
        card.append(&body);
    }

    let meta = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    if let Some(assignee) = task.assignee.as_deref() {
        meta.append(
            &gtk::Label::builder()
                .label(format!("@{assignee}"))
                .css_classes(["status-chip", "kanban-assignee-chip"])
                .build(),
        );
    }
    if let Some(note) = task.latest_note() {
        meta.append(
            &gtk::Label::builder()
                .label(note)
                .halign(gtk::Align::Start)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["field-hint", "kanban-card-note"])
                .build(),
        );
    }
    card.append(&meta);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(2)
        .css_classes(["kanban-card-actions"])
        .build();
    card.append(&actions);

    BoardCardChrome {
        widget: card,
        actions,
    }
}

/// The "no tasks" placeholder shown in an empty column.
pub(crate) fn build_empty_state() -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .css_classes(["kanban-empty-state"])
        .build();
    container.append(
        &gtk::Label::builder()
            .label("No tasks")
            .css_classes(["kanban-empty-title"])
            .build(),
    );
    container
}

/// A row in the Agents panel describing one run, exposing an `actions` box.
pub(crate) struct AgentRunRowChrome {
    pub(crate) widget: gtk::Box,
    pub(crate) actions: gtk::Box,
}

/// Build an agent-run row (agent kind + task + state chip + actions slot).
pub(crate) fn build_agent_run_row(run: &AgentRun) -> AgentRunRowChrome {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["alert-row", "agent-run-row"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let mut agent_label = run.agent_kind.label().to_string();
    if run.yolo {
        agent_label.push_str(" YOLO");
    }
    header.append(
        &gtk::Label::builder()
            .label(agent_label)
            .css_classes(["status-chip", "agent-kind-chip"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(run.run_kind.label())
            .css_classes(["status-chip", "agent-kind-chip"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(&run.task_title)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["card-title"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(run.state.label())
            .css_classes(["status-chip", "agent-state-chip"])
            .build(),
    );
    row.append(&header);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    row.append(&actions);

    AgentRunRowChrome {
        widget: row,
        actions,
    }
}
