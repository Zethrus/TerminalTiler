use std::rc::Rc;

use gtk::prelude::*;

#[derive(Clone)]
pub struct PaletteAction {
    pub title: String,
    pub subtitle: String,
    pub on_activate: Rc<dyn Fn()>,
}

#[allow(deprecated)]
pub fn present(window: &adw::ApplicationWindow, actions: Vec<PaletteAction>) {
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Command Palette")
        .default_width(720)
        .default_height(560)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

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

    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
    search.grab_focus();
}
