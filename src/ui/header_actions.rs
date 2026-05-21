use crate::ui::icons;

pub(crate) fn build_header_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    icons::icon_button(
        icon_name,
        tooltip,
        &["flat", "tile-header-action", "tile-header-close"],
    )
}
