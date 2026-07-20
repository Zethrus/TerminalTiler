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
use crate::ui::update_dialog::UpdateDialogController;
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
    let active_update = Rc::new(RefCell::new(None::<Rc<UpdateDialogController>>));

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
        let active_update = active_update.clone();
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
                    active_update.clone(),
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
    active_update: Rc<RefCell<Option<Rc<UpdateDialogController>>>>,
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
                if let Some(controller) = active_update.borrow().as_ref() {
                    controller.handle_event(&event);
                }
                match event {
                    UpdateEvent::Available(release) => {
                        *pending_update.borrow_mut() = Some(release);
                    }
                    UpdateEvent::Downloaded { release, artifact } => {
                        downloaded = Some((release, artifact));
                    }
                    UpdateEvent::DownloadStarted { .. }
                    | UpdateEvent::DownloadProgress { .. }
                    | UpdateEvent::Verifying { .. }
                    | UpdateEvent::DownloadCancelled { .. }
                    | UpdateEvent::DownloadFailed { .. } => {}
                    UpdateEvent::DebInstallStarted { version } => {
                        logging::info(format!(
                            "requesting PolicyKit authorization for TerminalTiler {version}"
                        ));
                    }
                    UpdateEvent::DebInstallSucceeded { release } => {
                        let Some(installation) = update::detect_installation() else {
                            if let Some(controller) = active_update.borrow().as_ref() {
                                controller.show_restart_handoff_failure(
                                    "The Debian launcher could no longer be verified.",
                                );
                            }
                            continue;
                        };
                        if installation.kind != update::InstallerKind::Deb {
                            if let Some(controller) = active_update.borrow().as_ref() {
                                controller.show_restart_handoff_failure(
                                    "The Debian installation provenance changed.",
                                );
                            }
                            continue;
                        }
                        let app_for_handoff = app.clone();
                        let active_for_handoff = active_update.clone();
                        gtk::glib::idle_add_local_once(move || {
                            match update::spawn_deb_restart_helper(&installation, &release.version)
                            {
                                Ok(()) => {
                                    logging::info(format!(
                                        "TerminalTiler {} installed; delayed restart helper started",
                                        release.version
                                    ));
                                    if let Some(controller) =
                                        active_for_handoff.borrow().as_ref().cloned()
                                    {
                                        let app_for_quit = app_for_handoff.clone();
                                        controller.close_for_restart_handoff(move || {
                                            quit_after_update(&app_for_quit)
                                        });
                                    } else {
                                        quit_after_update(&app_for_handoff);
                                    }
                                }
                                Err(error) => {
                                    logging::error(format!(
                                        "could not prepare Debian restart helper: {error}"
                                    ));
                                    if let Some(controller) = active_for_handoff.borrow().as_ref() {
                                        controller.show_restart_handoff_failure(&format!(
                                            "A safe restart helper could not be prepared: {error}"
                                        ));
                                    }
                                }
                            }
                        });
                    }
                    UpdateEvent::DebInstallFailed { release, error } => {
                        logging::error(format!(
                            "could not install Debian update {}: {error}",
                            release.version
                        ));
                    }
                }
            }
        }

        // Keep newer availability events pending while the current release
        // owns the dialog. Replacing that controller would orphan its UI.
        if active_update.borrow().is_none()
            && pending_update.borrow().is_some()
            && let Some(window) = primary_window(&app)
            && let Some(release) = pending_update.borrow_mut().take()
        {
            present_update_dialog(
                &app,
                &window,
                service.clone(),
                release,
                active_update.clone(),
            );
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

        if let Some((release, artifact)) = downloaded
            && let Some(controller) = active_update.borrow().as_ref()
        {
            controller.install_artifact(release, artifact);
        }
        gtk::glib::ControlFlow::Continue
    });
}

fn quit_after_update(app: &adw::Application) {
    if let Some(window) = primary_window(app) {
        if gtk::prelude::WidgetExt::activate_action(&window, "win.quit-app", None).is_err() {
            app.quit();
        }
    } else {
        app.quit();
    }
}

fn present_update_dialog(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    service: UpdateService,
    release: ReleaseInfo,
    active_update: Rc<RefCell<Option<Rc<UpdateDialogController>>>>,
) {
    let later_active = active_update.clone();
    let controller = UpdateDialogController::new(
        window,
        service.clone(),
        release,
        Rc::new(move || *later_active.borrow_mut() = None),
    );
    let weak_controller = Rc::downgrade(&controller);
    let app_for_handoff = app.clone();
    controller.set_artifact_handler(Rc::new(move |release, artifact| {
        let Some(controller) = weak_controller.upgrade() else {
            return;
        };
        let Some(installation) = update::detect_installation() else {
            controller.show_install_request_failure(
                "the installation provenance changed, so the current installation was left untouched",
            );
            return;
        };
        if installation.kind == update::InstallerKind::Deb {
            if let Err(error) = service.install_deb(release, artifact) {
                controller.show_install_request_failure(&error);
            }
            return;
        }
        controller.show_restarting();
        let app_for_handoff = app_for_handoff.clone();
        gtk::glib::idle_add_local_once(move || {
            match update::spawn_updater(&release, &artifact, &installation) {
                Ok(()) => controller
                    .close_for_restart_handoff(move || quit_after_update(&app_for_handoff)),
                Err(error) => controller.show_install_request_failure(&error),
            }
        });
    }));
    *active_update.borrow_mut() = Some(controller.clone());
    controller.present_release();
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
