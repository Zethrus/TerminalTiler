use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::extension::RuntimeOptions;
use crate::logging;
use crate::storage::asset_store::AssetStore;
use crate::storage::preference_store::PreferenceStore;
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::SessionStore;
use crate::tray;
use crate::ui::window;
use crate::update::{self, ReleaseInfo, UpdateEvent, UpdateService};

pub fn run() -> adw::glib::ExitCode {
    run_with_options_and_updates(RuntimeOptions::default(), true)
}

pub fn run_with_options(options: RuntimeOptions) -> adw::glib::ExitCode {
    run_with_options_and_updates(options, false)
}

fn run_with_options_and_updates(
    options: RuntimeOptions,
    enable_updates: bool,
) -> adw::glib::ExitCode {
    logging::init();
    logging::info("application startup");
    crate::platform::configure_webkit_process_environment();

    // Only the Core `run()` entrypoint opts into the public Core release
    // channel. Embedded callers using run_with_options() keep full control of
    // their own update/provenance policy.
    let update_runtime = if enable_updates && update::automatic_updates_enabled() {
        let (service, receiver) = UpdateService::start();
        Some((service, receiver))
    } else {
        None
    };
    let update_service = Rc::new(RefCell::new(
        update_runtime.as_ref().map(|(service, _)| service.clone()),
    ));
    let update_receiver = Rc::new(RefCell::new(update_runtime.map(|(_, receiver)| receiver)));
    let pending_update = Rc::new(RefCell::new(None::<ReleaseInfo>));
    let pending_update_failure = Rc::new(RefCell::new(None::<update::UpdateResult>));

    let (tray_tx, tray_rx) = mpsc::channel();
    let tray_rx = Rc::new(RefCell::new(Some(tray_rx)));
    let tray_controller = tray::TrayController::start(tray_tx, options.product.clone());
    let app_id = options.product.effective_gtk_application_id();
    let app = adw::Application::builder().application_id(app_id).build();

    {
        let tray_rx = tray_rx.clone();
        let tray_controller = tray_controller.clone();
        let options = options.clone();
        let update_service = update_service.clone();
        let update_receiver = update_receiver.clone();
        let pending_update = pending_update.clone();
        let pending_update_failure = pending_update_failure.clone();
        app.connect_startup(move |app| {
            crate::gtk_shell::load_css_for_default_display();
            crate::gtk_shell::configure_application_icons_for(&options.product.icon_name);
            if let Some(receiver) = tray_rx.borrow_mut().take() {
                install_tray_command_pump(app, receiver, tray_controller.clone(), options.clone());
            }
            if let Some(service) = update_service.borrow().clone() {
                install_update_pump(
                    app,
                    update_receiver.clone(),
                    service,
                    pending_update.clone(),
                    pending_update_failure.clone(),
                );
            }
        });
    }
    {
        let tray_controller = tray_controller.clone();
        let options = options.clone();
        app.connect_activate(move |app| {
            logging::info("application activated");
            let _ = ensure_main_window(app, &tray_controller, &options);
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

fn install_update_pump(
    app: &adw::Application,
    receiver: Rc<RefCell<Option<mpsc::Receiver<UpdateEvent>>>>,
    service: UpdateService,
    pending_update: Rc<RefCell<Option<ReleaseInfo>>>,
    pending_update_failure: Rc<RefCell<Option<update::UpdateResult>>>,
) {
    let app = app.clone();
    let mut startup_result_checked = false;
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        if !startup_result_checked {
            startup_result_checked = true;
            if let Some(result) = update::take_update_result()
                && !result.success
            {
                *pending_update_failure.borrow_mut() = Some(result);
            }
        }
        let mut downloaded = None;
        if let Some(receiver) = receiver.borrow_mut().as_mut() {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    UpdateEvent::Available(release) => {
                        *pending_update.borrow_mut() = Some(release);
                    }
                    UpdateEvent::Downloaded { release, artifact } => {
                        downloaded = Some((release, artifact));
                    }
                    UpdateEvent::DownloadFailed { version, error } => {
                        if let Some(window) = primary_window(&app) {
                            crate::ui::dialog_chrome::present_notice(
                                &window,
                                "update-error-dialog",
                                "Update failed",
                                &format!("TerminalTiler could not install {version}: {error}"),
                            );
                        }
                    }
                }
            }
        }

        if pending_update.borrow().is_some()
            && let Some(window) = primary_window(&app)
            && let Some(release) = pending_update.borrow_mut().take()
        {
            present_update_dialog(&app, &window, service.clone(), release);
        }

        if pending_update_failure.borrow().is_some()
            && let Some(window) = primary_window(&app)
            && let Some(result) = pending_update_failure.borrow_mut().take()
        {
            crate::ui::dialog_chrome::present_notice(
                &window,
                "update-error-dialog",
                "Previous update failed",
                &format!(
                    "TerminalTiler could not install {}: {}",
                    result.version,
                    result
                        .error
                        .unwrap_or_else(|| "unknown installer error".into())
                ),
            );
        }

        if let Some((release, artifact)) = downloaded {
            if let Some(installation) = update::detect_installation() {
                match update::spawn_updater(&release, &artifact, &installation) {
                    Ok(()) => {
                        logging::info(format!(
                            "update helper started for TerminalTiler {}",
                            release.version
                        ));
                        if let Some(window) = primary_window(&app) {
                            if gtk::prelude::WidgetExt::activate_action(
                                &window,
                                "win.quit-app",
                                None,
                            )
                            .is_err()
                            {
                                app.quit();
                            }
                        } else {
                            app.quit();
                        }
                    }
                    Err(error) => {
                        logging::error(format!("could not start update helper: {error}"));
                        if let Some(window) = primary_window(&app) {
                            crate::ui::dialog_chrome::present_notice(
                                &window,
                                "update-error-dialog",
                                "Update failed",
                                &format!("Could not restart TerminalTiler for the update: {error}"),
                            );
                        }
                    }
                }
            } else if let Some(window) = primary_window(&app) {
                crate::ui::dialog_chrome::present_notice(
                    &window,
                    "update-error-dialog",
                    "Update failed",
                    "The installation provenance changed, so TerminalTiler left the current installation untouched.",
                );
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}

fn present_update_dialog(
    _app: &adw::Application,
    window: &adw::ApplicationWindow,
    service: UpdateService,
    release: ReleaseInfo,
) {
    let notes = release.notes.trim();
    let notes = if notes.is_empty() {
        "This release contains improvements and fixes.".to_string()
    } else {
        notes.chars().take(1200).collect::<String>()
    };
    let version = release.version.to_string();
    let download_release = release.clone();
    let release_url = format!(
        "https://github.com/Zethrus/TerminalTiler/releases/tag/{}",
        release.tag
    );
    let modal = crate::ui::dialog_chrome::PremiumModal::new(
        "update-dialog",
        &format!("TerminalTiler {version} is available"),
    )
    .icon(
        crate::ui::icons::name::DIALOG_INFO,
        crate::ui::dialog_chrome::ModalAccent::Amber,
    )
    .body(&notes)
    .action(
        "Later",
        crate::ui::dialog_chrome::ModalActionRole::Secondary,
        true,
        || {},
    )
    .action(
        "View Release",
        crate::ui::dialog_chrome::ModalActionRole::Ghost,
        false,
        move || open_release_url(&release_url),
    )
    .action(
        "Install and Restart",
        crate::ui::dialog_chrome::ModalActionRole::Primary,
        false,
        move || service.download(download_release.clone()),
    );
    modal.present(Some(window));
}

fn open_release_url(url: &str) {
    if !url.starts_with("https://github.com/Zethrus/TerminalTiler/") {
        return;
    }
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn install_tray_command_pump(
    app: &adw::Application,
    receiver: mpsc::Receiver<tray::TrayCommand>,
    tray_controller: tray::TrayController,
    options: RuntimeOptions,
) {
    let app = app.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        while let Ok(command) = receiver.try_recv() {
            handle_tray_command(&app, &tray_controller, &options, command);
        }

        gtk::glib::ControlFlow::Continue
    });
}

fn handle_tray_command(
    app: &adw::Application,
    tray_controller: &tray::TrayController,
    options: &RuntimeOptions,
    command: tray::TrayCommand,
) {
    match command {
        tray::TrayCommand::Show => {
            let _ = ensure_main_window(app, tray_controller, options);
        }
        tray::TrayCommand::OpenSettings => {
            if let Some(window) = ensure_main_window(app, tray_controller, options)
                && let Err(error) =
                    gtk::prelude::WidgetExt::activate_action(&window, "win.open-settings", None)
            {
                logging::error(format!(
                    "failed to activate settings action from tray: {}",
                    error
                ));
            }
        }
        tray::TrayCommand::OpenStats => {
            if let Some(window) = ensure_main_window(app, tray_controller, options)
                && let Err(error) =
                    gtk::prelude::WidgetExt::activate_action(&window, "win.open-stats", None)
            {
                logging::error(format!(
                    "failed to activate stats action from tray: {}",
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
    options: &RuntimeOptions,
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
        options.clone(),
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
