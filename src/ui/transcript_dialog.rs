use adw::prelude::*;

use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

pub(crate) fn present(parent: &impl IsA<gtk::Widget>, transcript: &str) {
    let Some(window) = parent
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = adw::Dialog::new();
    dialog.set_title("Recent Transcript");
    dialog.set_follows_content_size(false);
    dialog.set_content_width(820);
    dialog.set_content_height(480);
    dialog_chrome::sync_dialog_chrome_classes(&window, &dialog, "transcript-dialog-window");

    let area = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let text = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    text.buffer().set_text(transcript);
    scroller.set_child(Some(&text));
    area.append(&scroller);

    let close_button = icons::labeled_button("Close", icon_name::CLOSE, &["pill-button", "flat"]);
    close_button.set_halign(gtk::Align::End);
    area.append(&close_button);
    dialog.set_child(Some(&area));
    dialog.set_default_widget(Some(&close_button));
    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            dialog.close();
        });
    }

    dialog.present(Some(&window));
}
