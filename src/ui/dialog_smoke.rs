use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};

use crate::logging;

const DIALOG_SMOKE_ENV: &str = "TERMINALTILER_DIALOG_CLOSE_SMOKE";
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const SETTLE_DELAY: Duration = Duration::from_millis(120);
const STEP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct DialogSmokeState {
    settings_dialog: RefCell<Option<adw::Dialog>>,
    settings_close: RefCell<Option<Rc<dyn Fn()>>>,
    assets_dialog: RefCell<Option<adw::Dialog>>,
    assets_prompt: RefCell<Option<adw::Dialog>>,
}

thread_local! {
    static DIALOG_SMOKE_STATE: RefCell<Option<Rc<DialogSmokeState>>> = const { RefCell::new(None) };
}

pub(crate) fn is_enabled() -> bool {
    std::env::var_os(DIALOG_SMOKE_ENV).is_some()
}

fn state() -> Option<Rc<DialogSmokeState>> {
    if !is_enabled() {
        return None;
    }

    DIALOG_SMOKE_STATE.with(|slot| {
        if slot.borrow().is_none() {
            slot.replace(Some(Rc::new(DialogSmokeState::default())));
        }
        slot.borrow().clone()
    })
}

pub(crate) fn register_settings_dialog(dialog: &adw::Dialog) {
    let Some(state) = state() else {
        return;
    };

    state.settings_dialog.replace(Some(dialog.clone()));
    let state_for_closed = state.clone();
    let dialog_for_closed = dialog.clone();
    dialog.connect_closed(move |_| {
        clear_if_current(&state_for_closed.settings_dialog, &dialog_for_closed);
    });
}

pub(crate) fn register_settings_close(request_close: Rc<dyn Fn()>) {
    let Some(state) = state() else {
        return;
    };

    state.settings_close.replace(Some(request_close));
}

pub(crate) fn register_assets_dialog(dialog: &adw::Dialog) {
    let Some(state) = state() else {
        return;
    };

    state.assets_dialog.replace(Some(dialog.clone()));
    let state_for_closed = state.clone();
    let dialog_for_closed = dialog.clone();
    dialog.connect_closed(move |_| {
        clear_if_current(&state_for_closed.assets_dialog, &dialog_for_closed);
    });
}

pub(crate) fn register_assets_prompt(dialog: &adw::Dialog) {
    let Some(state) = state() else {
        return;
    };

    state.assets_prompt.replace(Some(dialog.clone()));
    let state_for_closed = state.clone();
    let dialog_for_closed = dialog.clone();
    dialog.connect_closed(move |_| {
        clear_if_current(&state_for_closed.assets_prompt, &dialog_for_closed);
    });
}

pub(crate) fn build_assets_smoke_action_group(mark_dirty: Rc<dyn Fn()>) -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();
    let action = gio::SimpleAction::new("mark-dirty", None);
    action.connect_activate(move |_, _| {
        mark_dirty();
    });
    group.add_action(&action);
    group
}

pub(crate) fn build_prompt_smoke_action_group(
    keep_editing: Rc<dyn Fn()>,
    discard_changes: Rc<dyn Fn()>,
) -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();

    let keep_action = gio::SimpleAction::new("keep-editing", None);
    keep_action.connect_activate(move |_, _| {
        keep_editing();
    });
    group.add_action(&keep_action);

    let discard_action = gio::SimpleAction::new("discard-changes", None);
    discard_action.connect_activate(move |_, _| {
        discard_changes();
    });
    group.add_action(&discard_action);

    group
}

pub(crate) fn start(window: &adw::ApplicationWindow) {
    if !is_enabled() {
        return;
    }

    let window = window.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = run(window).await;
        match result {
            Ok(()) => {
                logging::info("dialog close smoke passed");
                println!("DIALOG_SMOKE PASS");
                std::process::exit(0);
            }
            Err(error) => {
                logging::error(format!("dialog close smoke failed: {error}"));
                eprintln!("DIALOG_SMOKE FAIL: {error}");
                std::process::exit(1);
            }
        }
    });
}

async fn run(window: adw::ApplicationWindow) -> Result<(), String> {
    glib::timeout_future(POLL_INTERVAL).await;

    println!("TEST settings close-attempt");
    gtk::prelude::WidgetExt::activate_action(&window, "win.open-settings", None)
        .map_err(|error| format!("failed to open settings dialog: {error}"))?;
    let settings_dialog =
        wait_for_dialog(current_settings_dialog, "Application Settings dialog").await?;
    glib::timeout_future(SETTLE_DELAY).await;
    invoke_settings_close()?;
    wait_until(
        || settings_dialog.parent().is_none(),
        "Application Settings dialog to close",
    )
    .await?;
    println!("PASS settings close-attempt");

    println!("TEST assets clean close-attempt");
    gtk::prelude::WidgetExt::activate_action(&window, "win.open-assets", None)
        .map_err(|error| format!("failed to open assets manager: {error}"))?;
    let assets_dialog = wait_for_dialog(current_assets_dialog, "Assets Manager dialog").await?;
    glib::timeout_future(SETTLE_DELAY).await;
    gtk::prelude::WidgetExt::activate_action(&assets_dialog, "window.close", None)
        .map_err(|error| format!("failed to request clean assets close: {error}"))?;
    wait_until(
        || assets_dialog.parent().is_none(),
        "Assets Manager clean close",
    )
    .await?;
    if current_assets_prompt().is_some() {
        return Err(String::from(
            "clean assets close unexpectedly showed the discard prompt",
        ));
    }
    println!("PASS assets clean close-attempt");

    println!("TEST assets dirty close-attempt");
    gtk::prelude::WidgetExt::activate_action(&window, "win.open-assets", None)
        .map_err(|error| format!("failed to reopen assets manager: {error}"))?;
    let assets_dialog = wait_for_dialog(current_assets_dialog, "Assets Manager dialog").await?;
    glib::timeout_future(SETTLE_DELAY).await;
    gtk::prelude::WidgetExt::activate_action(&assets_dialog, "dialog-smoke.mark-dirty", None)
        .map_err(|error| format!("failed to mark assets manager dirty: {error}"))?;
    gtk::prelude::WidgetExt::activate_action(&assets_dialog, "window.close", None)
        .map_err(|error| format!("failed to request dirty assets close: {error}"))?;

    let prompt = wait_for_dialog(
        current_assets_prompt,
        "Discard unsaved assets changes prompt",
    )
    .await?;
    glib::timeout_future(SETTLE_DELAY).await;
    gtk::prelude::WidgetExt::activate_action(&prompt, "dialog-smoke.keep-editing", None)
        .map_err(|error| format!("failed to keep editing from discard prompt: {error}"))?;
    wait_until(
        || prompt.parent().is_none(),
        "discard prompt to close after Keep Editing",
    )
    .await?;
    if assets_dialog.parent().is_none() {
        return Err(String::from(
            "assets dialog closed after Keep Editing instead of staying open",
        ));
    }

    let assets_dialog = wait_for_dialog(current_assets_dialog, "Assets Manager dialog").await?;
    gtk::prelude::WidgetExt::activate_action(&assets_dialog, "window.close", None)
        .map_err(|error| format!("failed to request dirty assets close a second time: {error}"))?;
    let prompt = wait_for_dialog(
        current_assets_prompt,
        "Discard unsaved assets changes prompt",
    )
    .await?;
    glib::timeout_future(SETTLE_DELAY).await;
    gtk::prelude::WidgetExt::activate_action(&prompt, "dialog-smoke.discard-changes", None)
        .map_err(|error| format!("failed to discard changes from prompt: {error}"))?;
    wait_until(
        || assets_dialog.parent().is_none(),
        "Assets Manager dirty discard close",
    )
    .await?;
    wait_until(
        || prompt.parent().is_none(),
        "discard prompt to close after Discard Changes",
    )
    .await?;
    println!("PASS assets dirty close-attempt");

    Ok(())
}

async fn wait_for_dialog<F>(mut getter: F, description: &str) -> Result<adw::Dialog, String>
where
    F: FnMut() -> Option<adw::Dialog>,
{
    let started = Instant::now();
    loop {
        if let Some(dialog) = getter() {
            return Ok(dialog);
        }
        if started.elapsed() >= STEP_TIMEOUT {
            return Err(format!("timed out waiting for {description}"));
        }
        glib::timeout_future(POLL_INTERVAL).await;
    }
}

async fn wait_until<F>(mut predicate: F, description: &str) -> Result<(), String>
where
    F: FnMut() -> bool,
{
    let started = Instant::now();
    loop {
        if predicate() {
            return Ok(());
        }
        if started.elapsed() >= STEP_TIMEOUT {
            return Err(format!("timed out waiting for {description}"));
        }
        glib::timeout_future(POLL_INTERVAL).await;
    }
}

fn current_settings_dialog() -> Option<adw::Dialog> {
    state().and_then(|state| visible_dialog(&state.settings_dialog))
}

fn invoke_settings_close() -> Result<(), String> {
    let request_close = state()
        .and_then(|state| state.settings_close.borrow().clone())
        .ok_or_else(|| String::from("settings close callback was not registered"))?;
    request_close();
    Ok(())
}

fn current_assets_dialog() -> Option<adw::Dialog> {
    state().and_then(|state| visible_dialog(&state.assets_dialog))
}

fn current_assets_prompt() -> Option<adw::Dialog> {
    state().and_then(|state| visible_dialog(&state.assets_prompt))
}

fn clear_if_current(slot: &RefCell<Option<adw::Dialog>>, dialog: &adw::Dialog) {
    let should_clear = slot
        .borrow()
        .as_ref()
        .map(|current| current.as_ptr() == dialog.as_ptr())
        .unwrap_or(false);
    if should_clear {
        slot.borrow_mut().take();
    }
}

fn visible_dialog(slot: &RefCell<Option<adw::Dialog>>) -> Option<adw::Dialog> {
    slot.borrow()
        .clone()
        .filter(|dialog| dialog.parent().is_some())
}
