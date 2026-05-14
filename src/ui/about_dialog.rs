use adw::prelude::*;
use gtk::gio;

use crate::extension::ProductInfo;
use crate::logging;
use crate::product;
use crate::ui::icons::{self, name as icon_name};

pub fn present(window: &adw::ApplicationWindow, product_info: &ProductInfo) {
    let dialog = adw::Dialog::new();
    dialog.set_title(&format!("About {}", product_info.display_name));
    dialog.set_follows_content_size(false);
    dialog.set_content_width(560);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    content.append(
        &gtk::Label::builder()
            .label(&product_info.display_name)
            .halign(gtk::Align::Start)
            .css_classes(["section-title"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(format!(
                "{} v{}",
                product_info.display_name,
                product::PRODUCT_VERSION
            ))
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(product::PRODUCT_COPYRIGHT)
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(product::PRODUCT_LICENSE)
            .halign(gtk::Align::Start)
            .css_classes(["status-chip"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(product::OPEN_CORE_STATEMENT)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );
    if let Some(extra_copy) = product_info
        .about_extra_copy
        .as_deref()
        .filter(|copy| !copy.trim().is_empty())
    {
        content.append(
            &gtk::Label::builder()
                .label(extra_copy)
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["field-hint"])
                .build(),
        );
    }

    let links = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Start)
        .build();
    links.append(&link_button("Website", product::PRODUCT_HOMEPAGE));
    links.append(&link_button("Source", product::PRODUCT_SOURCE_URL));
    links.append(&link_button("Issues", product::PRODUCT_ISSUES_URL));
    content.append(&links);

    let close_button = icons::labeled_button(
        "Close",
        icon_name::CLOSE,
        &["pill-button", "suggested-action"],
    );
    close_button.set_halign(gtk::Align::End);
    content.append(&close_button);

    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&close_button));

    {
        let dialog = dialog.clone();
        close_button.connect_clicked(move |_| {
            let _ = dialog.close();
        });
    }

    dialog.present(Some(window));
}

fn link_button(label: &str, uri: &'static str) -> gtk::Button {
    let button = icons::labeled_button(label, icon_name::WEB, &["pill-button", "flat"]);
    button.set_tooltip_text(Some(uri));
    button.connect_clicked(move |_| {
        if let Err(error) =
            gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
        {
            logging::error(format!("failed to open product link '{}': {}", uri, error));
        }
    });
    button
}
