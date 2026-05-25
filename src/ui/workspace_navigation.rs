use gtk::prelude::*;

pub(crate) fn sync_web_navigation_controls(
    path_label: &gtk::Label,
    url_entry: &gtk::Entry,
    url_reload_button: &gtk::Button,
    has_web_tiles: bool,
    current_url: Option<&str>,
    controls_enabled: bool,
) {
    path_label.set_visible(!has_web_tiles);
    url_entry.set_visible(has_web_tiles);
    url_reload_button.set_visible(has_web_tiles);
    url_entry.set_sensitive(controls_enabled);
    url_reload_button.set_sensitive(controls_enabled);

    if has_web_tiles {
        url_entry.set_text(current_url.unwrap_or_default());
    }
}
