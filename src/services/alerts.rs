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
        if let Some(alert) = self
            .inner
            .alerts
            .borrow_mut()
            .iter_mut()
            .find(|alert| alert.id == id)
        {
            alert.unread = false;
            self.notify();
        }
    }

    pub fn mark_all_read(&self) {
        let mut changed = false;
        for alert in self.inner.alerts.borrow_mut().iter_mut() {
            if alert.unread {
                alert.unread = false;
                changed = true;
            }
        }
        if changed {
            self.notify();
        }
    }

    pub fn subscribe(&self, listener: Rc<dyn Fn()>) {
        self.inner.listeners.borrow_mut().push(listener);
    }

    fn notify(&self) {
        for listener in self.inner.listeners.borrow().iter() {
            listener();
        }
    }
}
