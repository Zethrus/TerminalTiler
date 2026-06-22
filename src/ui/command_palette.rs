use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::model::assets::Runbook;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

/// Activation callbacks for the rows currently shown, indexed by row position.
type RowActions = Rc<RefCell<Vec<Rc<dyn Fn()>>>>;

#[derive(Clone)]
pub struct PaletteAction {
    pub title: String,
    pub subtitle: String,
    pub on_activate: Rc<dyn Fn()>,
}

#[derive(Clone)]
pub struct AppActionCallbacks {
    pub product_display_name: String,
    pub open_settings: Rc<dyn Fn()>,
    pub open_stats: Rc<dyn Fn()>,
    pub open_assets_manager: Rc<dyn Fn()>,
    pub open_about: Rc<dyn Fn()>,
    pub open_shortcuts: Rc<dyn Fn()>,
    pub new_tab: Rc<dyn Fn()>,
    pub open_companion: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
pub struct WorkspaceActionCallbacks {
    pub focus_next_alert: Rc<dyn Fn()>,
    pub toggle_maximize: Rc<dyn Fn()>,
    pub add_terminal_tile: Rc<dyn Fn()>,
    pub add_web_tile: Rc<dyn Fn()>,
    pub open_board: Rc<dyn Fn()>,
    pub runbooks: Vec<RunbookAction>,
}

#[derive(Clone)]
pub struct RunbookAction {
    pub runbook: Runbook,
    pub on_activate: Rc<dyn Fn()>,
}

pub fn app_actions(callbacks: AppActionCallbacks) -> Vec<PaletteAction> {
    let about_title = format!("About {}", callbacks.product_display_name);
    let mut actions = vec![
        PaletteAction {
            title: "Open Settings".into(),
            subtitle: "Application preferences and shortcuts.".into(),
            on_activate: callbacks.open_settings,
        },
        PaletteAction {
            title: "Open Usage Statistics".into(),
            subtitle: "Characters, words, and WPM typed today, this week, and all time.".into(),
            on_activate: callbacks.open_stats,
        },
        PaletteAction {
            title: "Open Assets Manager".into(),
            subtitle: "Edit global or workspace scoped assets.".into(),
            on_activate: callbacks.open_assets_manager,
        },
        PaletteAction {
            title: about_title,
            subtitle: "Version, license, source, and open-core model.".into(),
            on_activate: callbacks.open_about,
        },
        PaletteAction {
            title: "Keyboard Shortcuts".into(),
            subtitle: "View all active keyboard shortcuts.".into(),
            on_activate: callbacks.open_shortcuts,
        },
        PaletteAction {
            title: "New Tab".into(),
            subtitle: "Open a fresh launch deck tab.".into(),
            on_activate: callbacks.new_tab,
        },
    ];

    if let Some(open_companion) = callbacks.open_companion {
        actions.push(PaletteAction {
            title: "Open Account / Sync".into(),
            subtitle: "Account, activation, device, and sync controls.".into(),
            on_activate: open_companion,
        });
    }

    actions
}

pub fn active_tab_actions(rename_active_tab: Rc<dyn Fn()>) -> Vec<PaletteAction> {
    vec![PaletteAction {
        title: "Rename Active Tab".into(),
        subtitle: "Set a custom workspace title.".into(),
        on_activate: rename_active_tab,
    }]
}

pub fn workspace_actions(callbacks: WorkspaceActionCallbacks) -> Vec<PaletteAction> {
    let mut actions = vec![
        PaletteAction {
            title: "Focus Next Alert".into(),
            subtitle: "Jump to the next unread workspace alert.".into(),
            on_activate: callbacks.focus_next_alert,
        },
        PaletteAction {
            title: "Maximize / Restore Focused Pane".into(),
            subtitle: "Expand the focused pane to fill the workspace, or restore it.".into(),
            on_activate: callbacks.toggle_maximize,
        },
        PaletteAction {
            title: "Add Terminal Tile".into(),
            subtitle: "Insert a new terminal pane beside the focused pane.".into(),
            on_activate: callbacks.add_terminal_tile,
        },
        PaletteAction {
            title: "Add Web Tile".into(),
            subtitle: "Insert a new browser tile beside the focused pane.".into(),
            on_activate: callbacks.add_web_tile,
        },
        PaletteAction {
            title: "Open Kanban Board".into(),
            subtitle: "Open this workspace's per-project task board.".into(),
            on_activate: callbacks.open_board,
        },
    ];

    for runbook_action in callbacks.runbooks {
        actions.push(PaletteAction {
            title: format!("Run Runbook: {}", runbook_action.runbook.name),
            subtitle: runbook_subtitle(&runbook_action.runbook),
            on_activate: runbook_action.on_activate,
        });
    }

    actions
}

fn runbook_subtitle(runbook: &Runbook) -> String {
    if runbook.description.trim().is_empty() {
        runbook.target.label()
    } else {
        runbook.description.clone()
    }
}

pub fn present(window: &adw::ApplicationWindow, actions: Vec<PaletteAction>) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Command Palette");
    dialog.set_follows_content_size(false);
    dialog.set_content_width(720);
    dialog.set_content_height(560);
    dialog_chrome::sync_dialog_chrome_classes(window, &dialog, "command-palette-window");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let search = gtk::Entry::builder()
        .placeholder_text("Search commands")
        .primary_icon_name(icon_name::SEARCH)
        .hexpand(true)
        .build();
    content.append(&search);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Browse);
    list.add_css_class("command-palette-list");
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    scroller.set_child(Some(&list));
    content.append(&scroller);

    let close_button = icons::labeled_button("Close", icon_name::CLOSE, &["pill-button", "flat"]);
    close_button.set_halign(gtk::Align::End);
    content.append(&close_button);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&close_button));

    let actions = Rc::new(actions);
    let row_actions: RowActions = Rc::new(RefCell::new(Vec::new()));

    let activate_row: Rc<dyn Fn(&gtk::ListBoxRow)> = {
        let row_actions = row_actions.clone();
        let dialog = dialog.clone();
        Rc::new(move |row| {
            let index = row.index();
            if index < 0 {
                return;
            }
            let on_activate = row_actions.borrow().get(index as usize).cloned();
            if let Some(on_activate) = on_activate {
                on_activate();
                dialog.close();
            }
        })
    };

    let rebuild: Rc<dyn Fn()> = {
        let actions = actions.clone();
        let list = list.clone();
        let search = search.clone();
        let row_actions = row_actions.clone();
        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            row_actions.borrow_mut().clear();

            let query = search.text().trim().to_ascii_lowercase();
            for action in actions.iter().filter(|action| {
                query.is_empty()
                    || action.title.to_ascii_lowercase().contains(&query)
                    || action.subtitle.to_ascii_lowercase().contains(&query)
            }) {
                let shell = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(4)
                    .margin_top(8)
                    .margin_bottom(8)
                    .margin_start(8)
                    .margin_end(8)
                    .build();
                shell.append(
                    &gtk::Label::builder()
                        .label(&action.title)
                        .halign(gtk::Align::Start)
                        .css_classes(["card-title"])
                        .build(),
                );
                shell.append(
                    &gtk::Label::builder()
                        .label(&action.subtitle)
                        .halign(gtk::Align::Start)
                        .wrap(true)
                        .css_classes(["field-hint"])
                        .build(),
                );
                let row = gtk::ListBoxRow::builder()
                    .child(&shell)
                    .css_classes(["command-palette-row"])
                    .build();
                list.append(&row);
                row_actions.borrow_mut().push(action.on_activate.clone());
            }

            // Auto-select the first result so Enter runs it immediately.
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        })
    };
    rebuild();

    {
        let activate_row = activate_row.clone();
        list.connect_row_activated(move |_, row| activate_row(row));
    }

    {
        let rebuild = rebuild.clone();
        search.connect_changed(move |_| rebuild());
    }

    // Enter inside the search box runs the highlighted result.
    {
        let list = list.clone();
        let activate_row = activate_row.clone();
        search.connect_activate(move |_| {
            if let Some(row) = list.selected_row() {
                activate_row(&row);
            }
        });
    }

    // Arrow keys move the highlight without leaving the search box; Escape closes.
    {
        let list = list.clone();
        let dialog = dialog.clone();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::Down => {
                move_selection(&list, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                move_selection(&list, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Escape => {
                dialog.close();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        search.add_controller(key_controller);
    }

    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    dialog.present(Some(window));
    search.grab_focus();
}

/// Move the list selection by `delta` rows, clamped to the available range.
/// Selection (not focus) moves, so the search box keeps the keyboard.
fn move_selection(list: &gtk::ListBox, delta: i32) {
    let mut count = 0;
    while list.row_at_index(count).is_some() {
        count += 1;
    }
    if count == 0 {
        return;
    }
    let current = list.selected_row().map(|row| row.index()).unwrap_or(-1);
    let next = (current + delta).clamp(0, count - 1);
    if let Some(row) = list.row_at_index(next) {
        list.select_row(Some(&row));
    }
}
