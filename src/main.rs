mod app;
mod model;
mod storage;
mod terminal;
mod ui;

fn main() -> adw::glib::ExitCode {
    app::run()
}
