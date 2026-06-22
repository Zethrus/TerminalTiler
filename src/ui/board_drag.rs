use gtk::glib;

#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "TerminalTilerKanbanTaskDragPayload")]
pub(crate) struct KanbanTaskDragPayload(String);

impl KanbanTaskDragPayload {
    pub(crate) fn new(task_id: impl Into<String>) -> Self {
        Self(task_id.into())
    }

    pub(crate) fn into_task_id(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::KanbanTaskDragPayload;
    use adw::prelude::*;
    use gdk::prelude::StaticType;

    #[test]
    fn kanban_task_drag_payload_round_trips_task_id() {
        let payload = KanbanTaskDragPayload::new("task-a");

        assert_eq!(payload.into_task_id(), "task-a");
    }

    #[test]
    fn kanban_task_drag_payload_has_distinct_static_type() {
        assert_ne!(KanbanTaskDragPayload::static_type(), String::static_type());

        let value = KanbanTaskDragPayload::new("task-a").to_value();
        assert!(value.get::<String>().is_err());
        assert_eq!(
            value.get::<KanbanTaskDragPayload>().unwrap(),
            KanbanTaskDragPayload::new("task-a")
        );
    }
}
