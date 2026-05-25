use gtk::prelude::*;

const ACTIVE_TILE_CLASS: &str = "is-active-tile";

pub(crate) fn set_tile_active_class<W: IsA<gtk::Widget>>(widget: &W, is_active: bool) {
    if is_active {
        widget.add_css_class(ACTIVE_TILE_CLASS);
    } else {
        widget.remove_css_class(ACTIVE_TILE_CLASS);
    }
}
