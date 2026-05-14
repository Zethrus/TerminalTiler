use gtk::glib;

#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "TerminalTilerTileDragPayload")]
pub(crate) struct TileDragPayload(String);

impl TileDragPayload {
    pub(crate) fn new(tile_id: impl Into<String>) -> Self {
        Self(tile_id.into())
    }

    pub(crate) fn into_tile_id(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TileDragPayload;
    use adw::prelude::*;
    use gdk::prelude::StaticType;

    #[test]
    fn tile_reorder_payload_round_trips_tile_id() {
        let payload = TileDragPayload::new("pane-a");

        assert_eq!(payload.into_tile_id(), "pane-a");
    }

    #[test]
    fn tile_reorder_payload_is_not_a_plain_string_drop() {
        assert_ne!(TileDragPayload::static_type(), String::static_type());

        let value = TileDragPayload::new("pane-a").to_value();
        assert!(value.get::<String>().is_err());
        assert_eq!(
            value.get::<TileDragPayload>().unwrap(),
            TileDragPayload::new("pane-a")
        );
    }
}
