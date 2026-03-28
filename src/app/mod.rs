pub mod logging;

use adw::prelude::*;

use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::SessionStore;
use crate::ui::window;

pub const APP_ID: &str = "dev.zethrus.terminaltiler";

pub fn run() -> adw::glib::ExitCode {
    logging::init();
    logging::info("application startup");

    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| load_css());
    app.connect_activate(|app| {
        logging::info("application activated");
        let preset_store = PresetStore::new();
        preset_store.ensure_seeded();
        let session_store = SessionStore::new();
        let session_outcome = session_store.load_with_status();
        window::present(
            app,
            preset_store,
            session_store,
            session_outcome.session,
            session_outcome.warning,
        );
    });
    app.connect_shutdown(|_| logging::info("application shutdown"));

    app.run()
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
