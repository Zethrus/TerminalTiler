use gtk::prelude::*;

pub(super) fn build_header_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .focus_on_click(false)
        .css_classes(["flat", "tile-header-action", "tile-header-close"])
        .build();
    button.set_tooltip_text(Some(tooltip));
    if let Some(img) = button.first_child() {
        let _ = img.pango_context();
    }
    button
}
