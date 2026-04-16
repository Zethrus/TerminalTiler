use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::logging;
use crate::storage::asset_store::AssetStore;
use crate::storage::preference_store::PreferenceStore;
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::SessionStore;
use crate::tray;
use crate::ui::window;

pub const APP_ID: &str = "dev.zethrus.terminaltiler";

pub fn run() -> adw::glib::ExitCode {
    logging::init();
    logging::info("application startup");
    crate::platform::configure_webkit_process_environment();

    let (tray_tx, tray_rx) = mpsc::channel();
    let tray_rx = Rc::new(RefCell::new(Some(tray_rx)));
    let tray_controller = tray::TrayController::start(tray_tx);
    let app = adw::Application::builder().application_id(APP_ID).build();

    {
        let tray_rx = tray_rx.clone();
        let tray_controller = tray_controller.clone();
        app.connect_startup(move |app| {
            load_css();
            if let Some(receiver) = tray_rx.borrow_mut().take() {
                install_tray_command_pump(app, receiver, tray_controller.clone());
            }
        });
    }
    {
        let tray_controller = tray_controller.clone();
        app.connect_activate(move |app| {
            logging::info("application activated");
            let _ = ensure_main_window(app, &tray_controller);
        });
    }
    {
        let tray_controller = tray_controller.clone();
        app.connect_shutdown(move |_| {
            tray_controller.shutdown();
            logging::info("application shutdown");
        });
    }

    app.run()
}

fn install_tray_command_pump(
    app: &adw::Application,
    receiver: mpsc::Receiver<tray::TrayCommand>,
    tray_controller: tray::TrayController,
) {
    let app = app.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        while let Ok(command) = receiver.try_recv() {
            handle_tray_command(&app, &tray_controller, command);
        }

        gtk::glib::ControlFlow::Continue
    });
}

fn handle_tray_command(
    app: &adw::Application,
    tray_controller: &tray::TrayController,
    command: tray::TrayCommand,
) {
    match command {
        tray::TrayCommand::Show => {
            let _ = ensure_main_window(app, tray_controller);
        }
        tray::TrayCommand::OpenSettings => {
            if let Some(window) = ensure_main_window(app, tray_controller)
                && let Err(error) =
                    gtk::prelude::WidgetExt::activate_action(&window, "win.open-settings", None)
            {
                logging::error(format!(
                    "failed to activate settings action from tray: {}",
                    error
                ));
            }
        }
        tray::TrayCommand::Quit => {
            if let Some(window) = primary_window(app) {
                tray_controller.set_window_hidden(false);
                if let Err(error) =
                    gtk::prelude::WidgetExt::activate_action(&window, "win.quit-app", None)
                {
                    logging::error(format!(
                        "failed to activate quit action from tray: {}",
                        error
                    ));
                    app.quit();
                }
            } else {
                app.quit();
            }
        }
    }
}

fn ensure_main_window(
    app: &adw::Application,
    tray_controller: &tray::TrayController,
) -> Option<adw::ApplicationWindow> {
    if let Some(window) = primary_window(app) {
        restore_window(&window);
        tray_controller.set_window_hidden(false);
        return Some(window);
    }

    let preference_store = PreferenceStore::new();
    let preset_store = PresetStore::new();
    preset_store.ensure_seeded();
    let asset_store = AssetStore::new();
    asset_store.ensure_seeded();
    let session_store = SessionStore::new();
    let session_outcome = session_store.load_with_status();
    window::present(
        app,
        preference_store,
        preset_store,
        asset_store,
        session_store,
        session_outcome.session,
        session_outcome.warning,
        tray_controller.clone(),
    );

    let window = primary_window(app);
    if let Some(window) = &window {
        restore_window(window);
        tray_controller.set_window_hidden(false);
    }

    window
}

fn primary_window(app: &adw::Application) -> Option<adw::ApplicationWindow> {
    app.windows()
        .into_iter()
        .find_map(|window| window.downcast::<adw::ApplicationWindow>().ok())
}

fn restore_window(window: &adw::ApplicationWindow) {
    window.set_visible(true);
    window.present();
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("../../resources/style.css"));

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
