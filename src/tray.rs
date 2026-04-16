use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};

use crate::logging;

#[derive(Clone, Copy, Debug)]
pub enum TrayCommand {
    Show,
    OpenSettings,
    Quit,
}

#[derive(Clone)]
pub struct TrayController {
    available: Arc<AtomicBool>,
    window_hidden: Arc<AtomicBool>,
    handle: Rc<RefCell<Option<Handle<AppTray>>>>,
}

impl TrayController {
    pub fn start(command_tx: Sender<TrayCommand>) -> Self {
        let available = Arc::new(AtomicBool::new(false));
        let window_hidden = Arc::new(AtomicBool::new(false));

        let handle = match (AppTray {
            available: available.clone(),
            window_hidden: window_hidden.clone(),
            command_tx,
        })
        .spawn()
        {
            Ok(handle) => {
                available.store(true, Ordering::Relaxed);
                Some(handle)
            }
            Err(error) => {
                logging::info(format!(
                    "tray integration unavailable, close-to-background will fall back to normal close: {}",
                    error
                ));
                None
            }
        };

        Self {
            available,
            window_hidden,
            handle: Rc::new(RefCell::new(handle)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.handle.borrow().is_some() && self.available.load(Ordering::Relaxed)
    }

    pub fn set_window_hidden(&self, hidden: bool) {
        self.window_hidden.store(hidden, Ordering::Relaxed);

        if let Some(handle) = self.handle.borrow().as_ref() {
            handle.update(|tray| {
                tray.window_hidden.store(hidden, Ordering::Relaxed);
            });
        }
    }

    pub fn shutdown(&self) {
        if let Some(handle) = self.handle.borrow_mut().take() {
            handle.shutdown().wait();
        }
    }
}

struct AppTray {
    available: Arc<AtomicBool>,
    window_hidden: Arc<AtomicBool>,
    command_tx: Sender<TrayCommand>,
}

impl ksni::Tray for AppTray {
    fn id(&self) -> String {
        "dev.zethrus.terminaltiler".into()
    }

    fn title(&self) -> String {
        "TerminalTiler".into()
    }

    fn icon_name(&self) -> String {
        "terminaltiler".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "TerminalTiler".into(),
            description: if self.window_hidden.load(Ordering::Relaxed) {
                "Running in the background".into()
            } else {
                "Ready".into()
            },
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.command_tx.send(TrayCommand::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Show / Restore".into(),
                activate: Box::new(|tray: &mut AppTray| {
                    let _ = tray.command_tx.send(TrayCommand::Show);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: "Open Settings".into(),
                activate: Box::new(|tray: &mut AppTray| {
                    let _ = tray.command_tx.send(TrayCommand::OpenSettings);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut AppTray| {
                    let _ = tray.command_tx.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        self.available.store(true, Ordering::Relaxed);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.available.store(false, Ordering::Relaxed);
        true
    }
}
