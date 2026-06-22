//! Task-detail modal for the Kanban board: a card opens this dialog with three tabs —
//! Instructions, Knowledge, and Attachments — plus a footer (timestamps, copyable id, and
//! Run / Delete / Close actions). Mirrors `new_task_dialog` for dialog chrome and reuses the
//! shared `dialog_form` inputs.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;

use crate::model::board::{KnowledgeEntry, Task, TaskAttachment, now_epoch_secs};
use crate::services::board as board_service;
use crate::storage::board_store;
use crate::ui::icons::{self, name as icon_name};
use crate::ui::{dialog_chrome, dialog_form};

/// Re-populates the attachments list from disk.
type RefreshFn = Rc<dyn Fn()>;
/// Imports a batch of dropped/picked files into the task.
type ImportFn = Rc<dyn Fn(&[PathBuf])>;

/// Maximum number of attachments a single task may hold.
const MAX_ATTACHMENTS: usize = 10;
/// Maximum size of a single attachment, in bytes (25 MB).
const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
/// File extensions accepted as attachments (lower-case, without the dot).
const ALLOWED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "pdf", "doc", "docx", "xls", "xlsx", "csv",
    "txt", "md", "json", "zip",
];

/// Present the task-detail dialog for `task_id`.
///
/// - `on_changed` re-renders the board after instruction/attachment edits made here.
/// - `on_run` dispatches the board's default agent for the task.
/// - `on_delete` deletes the task from the board.
pub(crate) fn present(
    window: &adw::ApplicationWindow,
    project_root: PathBuf,
    task_id: String,
    on_changed: Rc<dyn Fn()>,
    on_run: Rc<dyn Fn(String)>,
    on_delete: Rc<dyn Fn(String)>,
) {
    let board = board_store::load(&project_root);
    let Some(task) = board_service::get_task(&board, &task_id).cloned() else {
        return;
    };

    let dialog = adw::Dialog::new();
    dialog.set_title("Task");
    dialog.set_content_width(520);
    dialog.set_content_height(640);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "task-detail-dialog-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    content.append(&build_header(&task));
    content.append(&build_status_chip(&task));

    // Tab strip (segmented control) + stack.
    let stack = gtk::Stack::builder()
        .vhomogeneous(false)
        .css_classes(["task-detail-stack"])
        .build();
    stack.add_named(
        &build_instructions_tab(&dialog, &project_root, &task, &on_changed),
        Some("instructions"),
    );
    stack.add_named(&build_knowledge_tab(&task), Some("knowledge"));
    stack.add_named(
        &build_attachments_tab(window, &project_root, &task_id, &on_changed),
        Some("attachments"),
    );
    content.append(&build_tab_strip(&stack));
    let stack_scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&stack)
        .build();
    content.append(&stack_scroller);

    content.append(&build_footer(
        window, &dialog, &task, &task_id, &on_run, &on_delete,
    ));

    dialog.set_child(Some(&content));
    dialog.present(Some(window));
}

fn build_header(task: &Task) -> gtk::Label {
    gtk::Label::builder()
        .label(&task.title)
        .halign(gtk::Align::Start)
        .wrap(true)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["card-title", "task-detail-title"])
        .build()
}

fn build_status_chip(task: &Task) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Start)
        .spacing(6)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(task.status.column_title())
            .css_classes(["status-chip", "task-detail-status-chip"])
            .build(),
    );
    if let Some(assignee) = task.assignee.as_deref() {
        row.append(
            &gtk::Label::builder()
                .label(format!("@{assignee}"))
                .css_classes(["status-chip", "task-detail-lifecycle-chip"])
                .build(),
        );
    }
    for indicator in board_service::lifecycle_indicators(task, now_epoch_secs()) {
        row.append(
            &gtk::Label::builder()
                .label(indicator)
                .css_classes([
                    "status-chip",
                    "task-detail-lifecycle-chip",
                    lifecycle_chip_class(indicator),
                ])
                .build(),
        );
    }
    row
}

fn lifecycle_chip_class(indicator: &str) -> &'static str {
    match indicator {
        "blocked" => "kanban-lifecycle-blocked",
        "paused" => "kanban-lifecycle-paused",
        "stale" => "kanban-lifecycle-stale",
        "active" => "kanban-lifecycle-active",
        _ => "kanban-lifecycle-neutral",
    }
}

fn build_tab_strip(stack: &gtk::Stack) -> gtk::Box {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .homogeneous(true)
        .css_classes(["task-detail-tabs"])
        .build();

    let instructions = tab_button("Instructions", icon_name::SNIPPET, None);
    let knowledge = tab_button("Knowledge", icon_name::SEARCH, Some(&instructions));
    let attachments = tab_button("Attachments", icon_name::FOLDER, Some(&instructions));
    instructions.set_active(true);

    for (button, name) in [
        (&instructions, "instructions"),
        (&knowledge, "knowledge"),
        (&attachments, "attachments"),
    ] {
        let stack = stack.clone();
        let name = name.to_string();
        button.connect_toggled(move |btn| {
            if btn.is_active() {
                stack.set_visible_child_name(&name);
            }
        });
        strip.append(button);
    }
    strip
}

fn tab_button(label: &str, icon: &str, group: Option<&gtk::ToggleButton>) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .css_classes(["task-detail-tab"])
        .build();
    icons::set_button_icon_label(button.upcast_ref::<gtk::Button>(), label, icon);
    if let Some(group) = group {
        button.set_group(Some(group));
    }
    button
}

fn build_instructions_tab(
    dialog: &adw::Dialog,
    project_root: &Path,
    task: &Task,
    on_changed: &Rc<dyn Fn()>,
) -> gtk::Box {
    let tab = tab_page();

    tab.append(&dialog_form::field_label("Additional instructions"));
    let (scroller, view) = dialog_form::multiline_input(96);
    if let Some(instructions) = task.additional_instructions.as_deref() {
        view.buffer().set_text(instructions);
    }
    tab.append(&scroller);

    let save = icons::labeled_button(
        "Save",
        icon_name::SAVE,
        &["pill-button", "suggested-action"],
    );
    let save_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::End)
        .build();
    save_row.append(&save);
    tab.append(&save_row);

    let status = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .visible(false)
        .css_classes(["field-hint"])
        .build();
    tab.append(&status);

    {
        let project_root = project_root.to_path_buf();
        let task_id = task.id.clone();
        let view = view.clone();
        let on_changed = on_changed.clone();
        let status = status.clone();
        let dialog = dialog.clone();
        save.connect_clicked(move |_| {
            let buffer = view.buffer();
            let (start, end) = buffer.bounds();
            let text = buffer.text(&start, &end, false).to_string();
            match board_store::update(&project_root, |board| {
                board_service::set_additional_instructions(board, &task_id, text.clone())
                    .is_ok()
                    .then(|| board.clone())
            }) {
                Ok(Some(_)) => {
                    on_changed();
                    dialog.close();
                }
                Ok(None) => {}
                Err(error) => {
                    status.set_text(&format!("Could not save: {error}"));
                    status.add_css_class("error-text");
                    status.set_visible(true);
                }
            }
        });
    }

    append_lifecycle_metadata(&tab, task);

    // Collapsible raw description, matching the mockup's "Raw content".
    let raw = task.description.trim();
    if !raw.is_empty() {
        let expander = gtk::Expander::builder()
            .label("Raw content")
            .css_classes(["task-detail-raw"])
            .build();
        let raw_label = gtk::Label::builder()
            .label(raw)
            .halign(gtk::Align::Start)
            .wrap(true)
            .selectable(true)
            .css_classes(["field-hint", "task-detail-raw-body"])
            .build();
        expander.set_child(Some(&raw_label));
        tab.append(&expander);
    } else {
        tab.append(
            &gtk::Label::builder()
                .label("No additional instructions provided.")
                .halign(gtk::Align::Start)
                .css_classes(["field-hint"])
                .build(),
        );
    }

    tab
}

fn append_lifecycle_metadata(tab: &gtk::Box, task: &Task) {
    let has_lifecycle = task.claimed_at.is_some()
        || task.heartbeat_at.is_some()
        || task.stale_after_secs.is_some()
        || task.paused.is_some()
        || task.blocked.is_some();
    if !has_lifecycle {
        return;
    }

    tab.append(&dialog_form::field_label("Lifecycle"));
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["task-detail-lifecycle-panel"])
        .build();
    if let Some(claimed_at) = task.claimed_at {
        card.append(&lifecycle_meta_line(
            "Claimed",
            format_timestamp(claimed_at),
        ));
    }
    if let Some(heartbeat_at) = task.heartbeat_at {
        card.append(&lifecycle_meta_line(
            "Heartbeat",
            format_timestamp(heartbeat_at),
        ));
    }
    if let Some(stale_after_secs) = task.stale_after_secs {
        card.append(&lifecycle_meta_line(
            "Stale after",
            format!("{stale_after_secs}s"),
        ));
    }
    if let Some(paused) = task.paused.as_ref() {
        let reason = paused.reason.as_deref().unwrap_or("No reason provided");
        card.append(&lifecycle_meta_line(
            "Paused",
            format!("{} · {reason}", format_timestamp(paused.paused_at)),
        ));
    }
    if let Some(blocked) = task.blocked.as_ref() {
        let category = blocked.category.as_deref().unwrap_or("uncategorized");
        card.append(&lifecycle_meta_line(
            "Blocked",
            format!(
                "{} · {category} · {}",
                format_timestamp(blocked.blocked_at),
                blocked.reason
            ),
        ));
    }
    tab.append(&card);
}

fn lifecycle_meta_line(label: &str, value: String) -> gtk::Label {
    gtk::Label::builder()
        .label(format!("{label}: {value}"))
        .halign(gtk::Align::Start)
        .wrap(true)
        .selectable(true)
        .css_classes(["field-hint", "task-detail-lifecycle-line"])
        .build()
}

fn build_knowledge_tab(task: &Task) -> gtk::Box {
    let tab = tab_page();

    if task.knowledge.is_empty() {
        let empty = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .css_classes(["kanban-empty-state"])
            .build();
        empty.append(
            &gtk::Label::builder()
                .label("No knowledge captured yet")
                .css_classes(["kanban-empty-title"])
                .build(),
        );
        empty.append(
            &gtk::Label::builder()
                .label("Knowledge is added by AI agents as they work on this task")
                .wrap(true)
                .justify(gtk::Justification::Center)
                .css_classes(["field-hint"])
                .build(),
        );
        tab.append(&empty);
    } else {
        for entry in &task.knowledge {
            tab.append(&build_knowledge_entry(entry));
        }
    }
    tab
}

fn build_knowledge_entry(entry: &KnowledgeEntry) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["task-detail-knowledge-entry"])
        .build();
    card.append(
        &gtk::Label::builder()
            .label(&entry.title)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["card-title"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label(&entry.content)
            .halign(gtk::Align::Start)
            .wrap(true)
            .selectable(true)
            .css_classes(["field-hint"])
            .build(),
    );

    let meta = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    for chip in [entry.source.as_deref(), entry.category.as_deref()]
        .into_iter()
        .flatten()
    {
        meta.append(
            &gtk::Label::builder()
                .label(chip)
                .css_classes(["status-chip"])
                .build(),
        );
    }
    meta.append(
        &gtk::Label::builder()
            .label(format_timestamp(entry.created_at))
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["field-hint"])
            .build(),
    );
    card.append(&meta);
    card
}

fn build_attachments_tab(
    window: &adw::ApplicationWindow,
    project_root: &Path,
    task_id: &str,
    on_changed: &Rc<dyn Fn()>,
) -> gtk::Box {
    let tab = tab_page();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    header.append(
        &gtk::Label::builder()
            .label("ATTACHMENTS")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["eyebrow", "field-label"])
            .build(),
    );
    let count_label = gtk::Label::builder()
        .label(format!("0 / {MAX_ATTACHMENTS}"))
        .css_classes(["field-hint"])
        .build();
    header.append(&count_label);
    tab.append(&header);

    // Drop zone + browse button.
    let drop_zone = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .css_classes(["task-detail-dropzone"])
        .build();
    drop_zone.append(
        &gtk::Label::builder()
            .label("Drag a file here or click to browse")
            .css_classes(["field-hint"])
            .build(),
    );
    drop_zone.append(
        &gtk::Label::builder()
            .label("Up to 25.00 MB · images, PDFs, docs, sheets, txt, csv, md, json, zip")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["field-hint"])
            .build(),
    );
    let browse = icons::labeled_button("Choose files", icon_name::FOLDER, &["pill-button", "flat"]);
    drop_zone.append(&browse);
    tab.append(&drop_zone);

    let status = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["field-hint"])
        .build();
    tab.append(&status);

    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    tab.append(&list);

    // Late-bound refresh so per-row remove buttons can re-trigger it.
    let refresh_holder: Rc<RefCell<Option<RefreshFn>>> = Rc::new(RefCell::new(None));
    let refresh: RefreshFn = {
        let project_root = project_root.to_path_buf();
        let task_id = task_id.to_string();
        let list = list.clone();
        let count_label = count_label.clone();
        let on_changed = on_changed.clone();
        let refresh_holder = refresh_holder.clone();
        Rc::new(move || {
            let board = board_store::load(&project_root);
            let attachments = board_service::get_task(&board, &task_id)
                .map(|task| task.attachments.clone())
                .unwrap_or_default();
            count_label.set_text(&format!("{} / {MAX_ATTACHMENTS}", attachments.len()));
            clear_box(&list);
            if attachments.is_empty() {
                list.append(
                    &gtk::Label::builder()
                        .label("No attachments yet.")
                        .halign(gtk::Align::Start)
                        .css_classes(["field-hint"])
                        .build(),
                );
                return;
            }
            for attachment in attachments {
                let row = build_attachment_row(&attachment);
                {
                    let project_root = project_root.clone();
                    let task_id = task_id.clone();
                    let path = attachment.path.clone();
                    let on_changed = on_changed.clone();
                    let refresh_holder = refresh_holder.clone();
                    row.remove.connect_clicked(move |_| {
                        remove_attachment(&project_root, &task_id, &path);
                        if let Some(refresh) = refresh_holder.borrow().clone() {
                            refresh();
                        }
                        on_changed();
                    });
                }
                list.append(&row.widget);
            }
        })
    };
    *refresh_holder.borrow_mut() = Some(refresh.clone());
    refresh();

    let import = build_import_closure(
        project_root,
        task_id,
        refresh.clone(),
        on_changed.clone(),
        status.clone(),
    );

    // Browse button → native file picker (gtk 4.8 has no FileDialog).
    {
        let window = window.clone();
        let import = import.clone();
        browse.connect_clicked(move |_| {
            let chooser = gtk::FileChooserNative::new(
                Some("Choose files"),
                Some(&window),
                gtk::FileChooserAction::Open,
                Some("Choose"),
                Some("Cancel"),
            );
            chooser.set_select_multiple(true);
            // Hold the native dialog alive until it responds.
            let holder: Rc<RefCell<Option<gtk::FileChooserNative>>> =
                Rc::new(RefCell::new(Some(chooser.clone())));
            let import = import.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    let files = chooser.files();
                    let mut paths = Vec::new();
                    for index in 0..files.n_items() {
                        if let Some(file) = files.item(index).and_downcast::<gtk::gio::File>()
                            && let Some(path) = file.path()
                        {
                            paths.push(path);
                        }
                    }
                    import(&paths);
                }
                holder.borrow_mut().take();
            });
            chooser.show();
        });
    }

    // Drag-and-drop onto the zone.
    install_file_drop(&drop_zone, import);

    tab
}

struct AttachmentRow {
    widget: gtk::Box,
    remove: gtk::Button,
}

fn build_attachment_row(attachment: &TaskAttachment) -> AttachmentRow {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["task-detail-attachment-row"])
        .build();

    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    info.append(
        &gtk::Label::builder()
            .label(&attachment.name)
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["card-title"])
            .build(),
    );
    info.append(
        &gtk::Label::builder()
            .label(format_size(attachment.size_bytes))
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );
    row.append(&info);

    let remove = icons::icon_button(
        icon_name::DELETE,
        "Remove attachment",
        &["flat", "surface-button"],
    );
    row.append(&remove);

    AttachmentRow {
        widget: row,
        remove,
    }
}

/// Build the closure that imports a batch of dropped/picked files into the task.
fn build_import_closure(
    project_root: &Path,
    task_id: &str,
    refresh: RefreshFn,
    on_changed: Rc<dyn Fn()>,
    status: gtk::Label,
) -> ImportFn {
    let project_root = project_root.to_path_buf();
    let task_id = task_id.to_string();
    Rc::new(move |paths: &[PathBuf]| {
        let board = board_store::load(&project_root);
        let mut remaining = board_service::get_task(&board, &task_id)
            .map(|task| MAX_ATTACHMENTS.saturating_sub(task.attachments.len()))
            .unwrap_or(0);

        let mut errors = Vec::new();
        for path in paths {
            if remaining == 0 {
                errors.push(format!("Attachment limit reached ({MAX_ATTACHMENTS}).",));
                break;
            }
            match import_attachment(&project_root, &task_id, path) {
                Ok(attachment) => {
                    if let Err(error) = board_store::update(&project_root, |board| {
                        board_service::add_attachment(board, &task_id, attachment.clone())
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    }) {
                        errors.push(error.to_string());
                    } else {
                        remaining -= 1;
                    }
                }
                Err(error) => errors.push(error),
            }
        }

        refresh();
        on_changed();
        if errors.is_empty() {
            status.set_visible(false);
        } else {
            status.set_text(&errors.join(" "));
            status.add_css_class("error-text");
            status.set_visible(true);
        }
    })
}

fn install_file_drop(drop_zone: &gtk::Box, import: ImportFn) {
    let drop_target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    {
        let drop_zone = drop_zone.clone();
        drop_target.connect_enter(move |_, _, _| {
            drop_zone.add_css_class("is-drop-target");
            gtk::gdk::DragAction::COPY
        });
    }
    {
        let drop_zone = drop_zone.clone();
        drop_target.connect_leave(move |_| {
            drop_zone.remove_css_class("is-drop-target");
        });
    }
    {
        let drop_zone = drop_zone.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            drop_zone.remove_css_class("is-drop-target");
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let paths: Vec<PathBuf> = files.files().into_iter().filter_map(|f| f.path()).collect();
            if paths.is_empty() {
                return false;
            }
            import(&paths);
            true
        });
    }
    drop_zone.add_controller(drop_target);
}

fn build_footer(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    task: &Task,
    task_id: &str,
    on_run: &Rc<dyn Fn(String)>,
    on_delete: &Rc<dyn Fn(String)>,
) -> gtk::Box {
    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["task-detail-footer"])
        .build();

    let meta = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    meta.append(
        &gtk::Label::builder()
            .label(format!("Created {}", format_timestamp(task.created_at)))
            .css_classes(["field-hint"])
            .build(),
    );
    meta.append(
        &gtk::Label::builder()
            .label(format!("Updated {}", format_timestamp(task.updated_at)))
            .css_classes(["field-hint"])
            .build(),
    );
    footer.append(&meta);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let short_id = &task_id[..task_id.len().min(8)];
    actions.append(
        &gtk::Label::builder()
            .label(format!("# {short_id}"))
            .css_classes(["status-chip", "settings-meta-chip"])
            .build(),
    );
    let copy = icons::icon_button(icon_name::COPY, "Copy task id", &["flat", "surface-button"]);
    {
        let window = window.clone();
        let task_id = task_id.to_string();
        copy.connect_clicked(move |_| {
            window.clipboard().set_text(&task_id);
        });
    }
    actions.append(&copy);

    let spacer = gtk::Box::builder().hexpand(true).build();
    actions.append(&spacer);

    let run = icons::labeled_button("Run", icon_name::RUN, &["pill-button", "suggested-action"]);
    {
        let dialog = dialog.clone();
        let task_id = task_id.to_string();
        let on_run = on_run.clone();
        run.connect_clicked(move |_| {
            on_run(task_id.clone());
            dialog.close();
        });
    }
    actions.append(&run);

    let delete = icons::labeled_button("Delete", icon_name::DELETE, &["pill-button", "flat"]);
    {
        let window = window.clone();
        let dialog = dialog.clone();
        let task_id = task_id.to_string();
        let on_delete = on_delete.clone();
        delete.connect_clicked(move |_| {
            let dialog = dialog.clone();
            let task_id = task_id.clone();
            let on_delete = on_delete.clone();
            dialog_chrome::confirm_destructive_action(
                &window,
                "Delete task?",
                "This permanently removes the task and its attachments from the board.",
                "Delete",
                move || {
                    on_delete(task_id.clone());
                    dialog.close();
                },
            );
        });
    }
    actions.append(&delete);

    let close = icons::labeled_button(
        "Close",
        icon_name::CLOSE,
        &["pill-button", "surface-button"],
    );
    {
        let dialog = dialog.clone();
        close.connect_clicked(move |_| {
            dialog.close();
        });
    }
    actions.append(&close);

    footer.append(&actions);
    footer
}

fn tab_page() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .css_classes(["task-detail-tab-page"])
        .build()
}

/// Copy `source` into `<project_root>/.terminaltiler/attachments/<task_id>/`, returning the
/// recorded metadata. Validates the extension and size; de-dupes the destination name.
fn import_attachment(
    project_root: &Path,
    task_id: &str,
    source: &Path,
) -> Result<TaskAttachment, String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "File has no readable name.".to_string())?
        .to_string();
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!("'{name}' has an unsupported file type."));
    }
    let metadata =
        fs::metadata(source).map_err(|error| format!("Cannot read '{name}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("'{name}' is not a file."));
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!("'{name}' is larger than 25.00 MB."));
    }

    let relative_dir = PathBuf::from(board_store::BOARD_DIR_NAME)
        .join("attachments")
        .join(task_id);
    let dir = project_root.join(&relative_dir);
    fs::create_dir_all(&dir).map_err(|error| format!("Cannot create attachments dir: {error}"))?;

    let dest_name = unique_destination_name(&dir, &name);
    let dest = dir.join(&dest_name);
    fs::copy(source, &dest).map_err(|error| format!("Cannot copy '{name}': {error}"))?;

    Ok(TaskAttachment {
        path: relative_dir.join(&dest_name).display().to_string(),
        name,
        mime_type: guess_mime(&extension),
        size_bytes: metadata.len(),
        added_at: now_epoch_secs(),
    })
}

/// Delete an attachment's backing file (best effort) and drop it from the board.
fn remove_attachment(project_root: &Path, task_id: &str, path: &str) {
    let removed = board_store::update(project_root, |board| {
        board_service::remove_attachment(board, task_id, path)
            .ok()
            .flatten()
    });
    if let Ok(Some(attachment)) = removed {
        let _ = fs::remove_file(project_root.join(&attachment.path));
    }
}

/// Pick a filename in `dir` that does not collide, suffixing `-1`, `-2`, … before the extension.
fn unique_destination_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|ext| ext.to_str());
    for index in 1.. {
        let candidate = match extension {
            Some(ext) => format!("{stem}-{index}.{ext}"),
            None => format!("{stem}-{index}"),
        };
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("the index range is unbounded")
}

fn guess_mime(extension: &str) -> Option<String> {
    let mime = match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(mime.to_string())
}

fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= MIB {
        format!("{:.2} MB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_timestamp(seconds: u64) -> String {
    gtk::glib::DateTime::from_unix_local(seconds as i64)
        .and_then(|dt| dt.format("%b %e, %I:%M %p"))
        .map(|formatted| formatted.trim().to_string())
        .unwrap_or_else(|_| "—".to_string())
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "terminaltiler-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn imported_attachment_records_path_where_file_is_copied() {
        let project_root = temp_project("import-attachment");
        fs::create_dir_all(&project_root).unwrap();
        let source = project_root.join("shot.png");
        fs::write(&source, b"png-bytes").unwrap();

        let attachment = import_attachment(&project_root, "task-1", &source).unwrap();

        assert_eq!(
            attachment.path,
            ".terminaltiler/attachments/task-1/shot.png"
        );
        assert_eq!(
            fs::read(project_root.join(&attachment.path)).unwrap(),
            b"png-bytes"
        );
        assert!(
            !project_root
                .join("attachments")
                .join("task-1")
                .join("shot.png")
                .exists()
        );

        let _ = fs::remove_dir_all(&project_root);
    }

    #[test]
    fn removed_attachment_deletes_backing_file_from_board_dir() {
        let project_root = temp_project("remove-attachment");
        let mut board = crate::model::board::Board::default();
        let task_id = crate::services::board::create_task(
            &mut board,
            "Task",
            "",
            crate::model::board::TaskStatus::Todo,
        )
        .id
        .clone();
        let attachment_path = format!(".terminaltiler/attachments/{task_id}/shot.png");
        let backing_file = project_root.join(&attachment_path);
        fs::create_dir_all(backing_file.parent().unwrap()).unwrap();
        fs::write(&backing_file, b"png-bytes").unwrap();

        crate::services::board::add_attachment(
            &mut board,
            &task_id,
            TaskAttachment {
                path: attachment_path.clone(),
                name: "shot.png".into(),
                mime_type: Some("image/png".into()),
                size_bytes: 9,
                added_at: 0,
            },
        )
        .unwrap();
        board_store::save(&project_root, &board).unwrap();

        remove_attachment(&project_root, &task_id, &attachment_path);

        assert!(!backing_file.exists());
        let board = board_store::load(&project_root);
        assert!(
            crate::services::board::get_task(&board, &task_id)
                .unwrap()
                .attachments
                .is_empty()
        );

        let _ = fs::remove_dir_all(&project_root);
    }
}
