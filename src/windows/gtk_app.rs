#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::rc::Rc;

    use adw::prelude::*;

    use crate::extension::RuntimeOptions;
    use crate::logging;
    use crate::model::preset::{ApplicationDensity, ThemeMode};
    use crate::services::session_restore::session_for_restore_mode;
    use crate::storage::asset_store::AssetStore;
    use crate::storage::preference_store::{AppPreferences, PreferenceStore};
    use crate::storage::preset_store::PresetStore;
    use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
    use crate::ui::launch_screen::{LaunchScreenActions, LaunchScreenInput};
    use crate::windows::{workspace, wsl};

    const GTK_APP_ID: &str = "dev.zethrus.terminaltiler.windows.gtk";

    pub fn run() -> ExitCode {
        run_with_options(RuntimeOptions::default())
    }

    pub fn run_with_options(options: RuntimeOptions) -> ExitCode {
        logging::init();
        logging::info("windows GTK shell startup");

        let app_id = options.product.app_id.as_deref().unwrap_or(GTK_APP_ID);
        let app = adw::Application::builder().application_id(app_id).build();

        app.connect_startup(|_| {
            crate::gtk_shell::load_css_for_default_display();
            logging::info("windows GTK shell loaded canonical GTK CSS");
        });

        app.connect_activate(move |app| {
            present_launch_window(app, &options);
        });

        glib_exit_to_process_exit(app.run())
    }

    fn glib_exit_to_process_exit(code: gtk::glib::ExitCode) -> ExitCode {
        let value = code.value();
        if value == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(value.clamp(1, u8::MAX as i32) as u8)
        }
    }

    fn present_launch_window(app: &adw::Application, options: &RuntimeOptions) {
        if let Some(window) = primary_window(app) {
            window.present();
            return;
        }

        let preference_store = PreferenceStore::new();
        let preferences = preference_store.load();
        let preset_store = PresetStore::new();
        preset_store.ensure_seeded();
        let preset_outcome = preset_store.load_presets_with_status();
        let asset_store = AssetStore::new();
        asset_store.ensure_seeded();
        let asset_outcome = asset_store.load_assets_with_status();
        let session_store = SessionStore::new();
        let session_outcome = session_store.load_with_status();
        let load_warning = combine_warnings(
            combine_warnings(preset_outcome.warning, asset_outcome.warning),
            session_outcome.warning,
        );

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&options.product.app_title)
            .default_width(1180)
            .default_height(780)
            .content_width(1180)
            .content_height(780)
            .build();
        window.add_css_class("window-shell");
        window.add_css_class("windows-gtk-shell");
        apply_theme_mode(&window, preferences.default_theme);
        apply_window_density(&window, preferences.default_density);

        let overlay = adw::ToastOverlay::new();
        let launch_preferences = preferences.clone();
        let launch_overlay = overlay.clone();
        let actions = LaunchScreenActions {
            on_theme_preview: Rc::new({
                let window = window.clone();
                move |theme| apply_theme_mode(&window, theme)
            }),
            on_density_preview: Rc::new({
                let window = window.clone();
                move |density| apply_window_density(&window, density)
            }),
            on_launch: Rc::new(move |preset, workspace_root| {
                launch_workspace_from_gtk(
                    &launch_overlay,
                    &launch_preferences,
                    preset,
                    workspace_root,
                );
            }),
            on_cancel: Rc::new({
                let app = app.clone();
                move || app.quit()
            }),
            on_presets_changed: Rc::new(|| {
                logging::info("Windows GTK shell preset list changed; relaunch to refresh deck");
            }),
        };

        let launch = crate::ui::launch_screen::build(
            LaunchScreenInput {
                load_warning,
                presets: preset_outcome.presets,
                assets: asset_outcome.assets,
                default_theme: preferences.default_theme,
                default_density: preferences.default_density,
                default_restore_mode: preferences.default_restore_mode,
                preset_store,
            },
            actions,
        );
        overlay.set_child(Some(&launch));
        window.set_content(Some(&overlay));
        window.present();

        if let Some(session) = session_outcome
            .session
            .as_ref()
            .and_then(|session| session_for_restore_mode(session, preferences.default_restore_mode))
        {
            let overlay = overlay.clone();
            let preferences = preferences.clone();
            gtk::glib::idle_add_local_once(move || {
                restore_saved_session_from_gtk(&overlay, &preferences, session);
            });
        }
    }

    fn restore_saved_session_from_gtk(
        overlay: &adw::ToastOverlay,
        preferences: &AppPreferences,
        session: SavedSession,
    ) {
        match wsl::probe_runtime(preferences.windows_wsl_distribution.as_deref())
            .and_then(|runtime| workspace::open_saved_workspaces(&session, &runtime))
        {
            Ok((windows, panes)) => {
                logging::info(format!(
                    "opened {windows} restored Windows workspace host window(s) from GTK shell with {panes} pane(s)"
                ));
                overlay.add_toast(adw::Toast::new("Restored saved workspace session"));
            }
            Err(error) => {
                logging::error(format!("Windows GTK shell session restore failed: {error}"));
                overlay.add_toast(adw::Toast::new(&format!("Restore failed: {error}")));
            }
        }
    }

    fn primary_window(app: &adw::Application) -> Option<adw::ApplicationWindow> {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<adw::ApplicationWindow>().ok())
    }

    fn apply_theme_mode(window: &adw::ApplicationWindow, theme: ThemeMode) {
        let manager = adw::StyleManager::default();
        manager.set_color_scheme(match theme {
            ThemeMode::System => adw::ColorScheme::Default,
            ThemeMode::Light => adw::ColorScheme::ForceLight,
            ThemeMode::Dark => adw::ColorScheme::ForceDark,
        });

        window.remove_css_class("theme-light");
        window.remove_css_class("theme-dark");
        window.add_css_class(if manager.is_dark() {
            "theme-dark"
        } else {
            "theme-light"
        });
    }

    fn apply_window_density(window: &adw::ApplicationWindow, density: ApplicationDensity) {
        window.remove_css_class("profile-comfortable");
        window.remove_css_class("profile-standard");
        window.remove_css_class("profile-compact");
        window.add_css_class(density.css_class());
    }

    fn launch_workspace_from_gtk(
        overlay: &adw::ToastOverlay,
        preferences: &AppPreferences,
        preset: crate::model::preset::WorkspacePreset,
        workspace_root: PathBuf,
    ) {
        let session = SavedSession {
            tabs: vec![SavedTab {
                preset,
                workspace_root,
                custom_title: None,
                terminal_zoom_steps: 0,
            }],
            active_tab_index: 0,
        };

        match wsl::probe_runtime(preferences.windows_wsl_distribution.as_deref())
            .and_then(|runtime| workspace::open_saved_workspaces(&session, &runtime))
        {
            Ok((windows, panes)) => {
                logging::info(format!(
                    "Windows GTK shell opened {windows} workspace host window(s) with {panes} pane(s)"
                ));
                overlay.add_toast(adw::Toast::new(
                    "Workspace opened in the Windows runtime host",
                ));
            }
            Err(error) => {
                logging::error(format!("Windows GTK shell launch failed: {error}"));
                overlay.add_toast(adw::Toast::new(&format!("Launch failed: {error}")));
            }
        }
    }

    fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
        match (first, second) {
            (Some(first), Some(second)) if !second.trim().is_empty() => {
                Some(format!("{first}\n{second}"))
            }
            (Some(first), _) => Some(first),
            (_, Some(second)) => Some(second),
            (None, None) => None,
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub use imp::{run, run_with_options};
