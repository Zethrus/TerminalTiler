use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSourceKind {
    OutputHelper,
    PaneExit,
    Reconnect,
    Runbook,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertEvent {
    pub id: u64,
    pub source: AlertSourceKind,
    pub severity: AlertSeverity,
    pub title: String,
    pub detail: String,
    pub pane_id: Option<String>,
    pub unread: bool,
    pub allows_reconnect: bool,
}

#[derive(Clone, Debug)]
pub struct AlertEventInput {
    pub source: AlertSourceKind,
    pub severity: AlertSeverity,
    pub title: String,
    pub detail: String,
    pub pane_id: Option<String>,
    pub allows_reconnect: bool,
}

impl AlertEventInput {
    pub fn new(source: AlertSourceKind, severity: AlertSeverity, title: impl Into<String>) -> Self {
        Self {
            source,
            severity,
            title: title.into(),
            detail: String::new(),
            pane_id: None,
            allows_reconnect: false,
        }
    }
}

#[derive(Clone, Default)]
pub struct AlertStore {
    inner: Rc<AlertStoreInner>,
}

#[derive(Default)]
struct AlertStoreInner {
    next_id: Cell<u64>,
    alerts: RefCell<Vec<AlertEvent>>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl AlertStore {
    pub fn push(&self, input: AlertEventInput) -> u64 {
        let id = self.inner.next_id.get() + 1;
        self.inner.next_id.set(id);
        {
            self.inner.alerts.borrow_mut().push(AlertEvent {
                id,
                source: input.source,
                severity: input.severity,
                title: input.title,
                detail: input.detail,
                pane_id: input.pane_id,
                unread: true,
                allows_reconnect: input.allows_reconnect,
            });
        }
        self.notify();
        id
    }

    pub fn snapshot(&self) -> Vec<AlertEvent> {
        self.inner.alerts.borrow().clone()
    }

    pub fn unread_count(&self) -> usize {
        self.inner
            .alerts
            .borrow()
            .iter()
            .filter(|alert| alert.unread)
            .count()
    }

    pub fn mark_read(&self, id: u64) {
        let changed = {
            if let Some(alert) = self
                .inner
                .alerts
                .borrow_mut()
                .iter_mut()
                .find(|alert| alert.id == id)
            {
                if alert.unread {
                    alert.unread = false;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if changed {
            self.notify();
        }
    }

    pub fn mark_all_read(&self) {
        let changed = {
            let mut changed = false;
            for alert in self.inner.alerts.borrow_mut().iter_mut() {
                if alert.unread {
                    alert.unread = false;
                    changed = true;
                }
            }
            changed
        };

        if changed {
            self.notify();
        }
    }

    #[allow(dead_code)]
    pub fn subscribe(&self, listener: Rc<dyn Fn()>) {
        self.inner.listeners.borrow_mut().push(listener);
    }

    fn notify(&self) {
        let listeners = self.inner.listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn listeners_can_read_store_during_push_and_mark_read_transitions() {
        let store = AlertStore::default();
        let callback_count = Rc::new(Cell::new(0usize));

        store.subscribe(Rc::new({
            let store = store.clone();
            let callback_count = callback_count.clone();
            move || {
                callback_count.set(callback_count.get() + 1);
                let snapshot = store.snapshot();
                let unread = store.unread_count();
                assert_eq!(unread, snapshot.iter().filter(|alert| alert.unread).count());
            }
        }));

        let id = store.push(AlertEventInput::new(
            AlertSourceKind::Runbook,
            AlertSeverity::Info,
            "Listener re-entry",
        ));
        assert_eq!(callback_count.get(), 1);
        assert_eq!(store.unread_count(), 1);

        store.mark_read(id);
        assert_eq!(callback_count.get(), 2);
        assert_eq!(store.unread_count(), 0);

        store.mark_all_read();
        assert_eq!(callback_count.get(), 2);
    }

    #[test]
    fn listeners_can_read_store_during_mark_all_read() {
        let store = AlertStore::default();
        let callback_count = Rc::new(Cell::new(0usize));

        store.push(AlertEventInput::new(
            AlertSourceKind::PaneExit,
            AlertSeverity::Warning,
            "First alert",
        ));
        store.push(AlertEventInput::new(
            AlertSourceKind::Reconnect,
            AlertSeverity::Error,
            "Second alert",
        ));

        store.subscribe(Rc::new({
            let store = store.clone();
            let callback_count = callback_count.clone();
            move || {
                callback_count.set(callback_count.get() + 1);
                assert_eq!(store.unread_count(), 0);
                assert!(store.snapshot().iter().all(|alert| !alert.unread));
            }
        }));

        store.mark_all_read();
        assert_eq!(callback_count.get(), 1);
        assert_eq!(store.unread_count(), 0);
    }
}
