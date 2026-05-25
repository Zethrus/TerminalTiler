use std::rc::Rc;

use adw::prelude::*;

use crate::model::assets::Runbook;
use crate::product;
use crate::ui::icons::{self, name as icon_name};

#[derive(Clone)]
pub struct PaletteAction {
    pub title: String,
    pub subtitle: String,
    pub on_activate: Rc<dyn Fn()>,
}

#[derive(Clone)]
pub struct AppActionCallbacks {
    pub open_settings: Rc<dyn Fn()>,
    pub open_assets_manager: Rc<dyn Fn()>,
    pub open_about: Rc<dyn Fn()>,
    pub new_tab: Rc<dyn Fn()>,
    pub open_companion: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
pub struct WorkspaceActionCallbacks {
    pub focus_next_alert: Rc<dyn Fn()>,
    pub add_web_tile: Rc<dyn Fn()>,
    pub runbooks: Vec<RunbookAction>,
}

#[derive(Clone)]
pub struct RunbookAction {
    pub runbook: Runbook,
    pub on_activate: Rc<dyn Fn()>,
}

pub fn app_actions(callbacks: AppActionCallbacks) -> Vec<PaletteAction> {
    let mut actions = vec![
        PaletteAction {
            title: "Open Settings".into(),
            subtitle: "Application preferences and shortcuts.".into(),
            on_activate: callbacks.open_settings,
        },
        PaletteAction {
            title: "Open Assets Manager".into(),
            subtitle: "Edit global or workspace scoped assets.".into(),
            on_activate: callbacks.open_assets_manager,
        },
        PaletteAction {
            title: format!("About {}", product::PRODUCT_DISPLAY_NAME),
            subtitle: "Version, license, source, and open-core model.".into(),
            on_activate: callbacks.open_about,
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
            title: "Add Web Tile".into(),
            subtitle: "Insert a new browser tile beside the focused pane.".into(),
            on_activate: callbacks.add_web_tile,
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
    list.set_selection_mode(gtk::SelectionMode::None);
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
    let rebuild: Rc<dyn Fn()> = {
        let actions = actions.clone();
        let list = list.clone();
        let search = search.clone();
        let dialog = dialog.clone();
        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let query = search.text().trim().to_ascii_lowercase();
            for action in actions.iter().filter(|action| {
                query.is_empty()
                    || action.title.to_ascii_lowercase().contains(&query)
                    || action.subtitle.to_ascii_lowercase().contains(&query)
            }) {
                let row_button = gtk::Button::builder()
                    .css_classes(["flat"])
                    .hexpand(true)
                    .halign(gtk::Align::Fill)
                    .build();
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
                row_button.set_child(Some(&shell));
                let on_activate = action.on_activate.clone();
                let dialog = dialog.clone();
                row_button.connect_clicked(move |_| {
                    on_activate();
                    dialog.close();
                });
                list.append(&row_button);
            }
        })
    };
    rebuild();

    {
        let rebuild = rebuild.clone();
        search.connect_changed(move |_| rebuild());
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
