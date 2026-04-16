use std::rc::Rc;

use adw::prelude::*;

#[derive(Clone)]
pub struct PaletteAction {
    pub title: String,
    pub subtitle: String,
    pub on_activate: Rc<dyn Fn()>,
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

    let close_button = gtk::Button::with_label("Close");
    close_button.add_css_class("pill-button");
    close_button.add_css_class("flat");
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
