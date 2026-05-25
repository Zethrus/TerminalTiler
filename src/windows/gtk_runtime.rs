#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use adw::prelude::*;
    use gtk::gio;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    use crate::logging;
    use crate::model::assets::WorkspaceAssets;
    use crate::model::layout::{DEFAULT_WEB_URL, TileKind, TileSpec, normalize_web_url};
    use crate::model::preset::ApplicationDensity;
    use crate::services::launch_resolution::resolve_tile_launch;
    use crate::storage::session_store::SavedTab;
    use crate::ui::icons::{self, name as icon_name};
    use crate::ui::tile_chrome::{domain_from_url, make_shrinkable};
    use crate::ui::workspace_preview::TileRuntimeSurface;
    use crate::windows::{workspace, wsl};

    const MIN_TERMINAL_FONT_POINTS: i32 = 7;
    const MAX_TERMINAL_FONT_POINTS: i32 = 20;

    pub(crate) fn build_tile_runtime_surface(
        tile: &TileSpec,
        tab: &SavedTab,
        assets: &WorkspaceAssets,
    ) -> TileRuntimeSurface {
        match tile.tile_kind {
            TileKind::Terminal => build_terminal_runtime_surface(tile, tab, assets),
            TileKind::WebView => TileRuntimeSurface::widget(build_web_runtime_surface(tile)),
        }
    }

    fn build_terminal_runtime_surface(
        tile: &TileSpec,
        tab: &SavedTab,
        assets: &WorkspaceAssets,
    ) -> TileRuntimeSurface {
        let surface = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["terminal-surface", "windows-gtk-terminal-runtime"])
            .build();
        make_shrinkable(&surface);

        let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        append_buffer_line(
            &buffer,
            &format!(
                "[terminaltiler] starting {} in {}",
                tile.title,
                tab.workspace_root.display()
            ),
        );

        let terminal_output = gtk::TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .monospace(true)
            .cursor_visible(false)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["terminal-runtime-output"])
            .build();
        make_shrinkable(&terminal_output);
        let appearance_provider = gtk::CssProvider::new();
        terminal_output.style_context().add_provider(
            &appearance_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        apply_terminal_runtime_appearance(
            &appearance_provider,
            tab.preset.density,
            tab.terminal_zoom_steps,
        );

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&terminal_output)
            .build();
        make_shrinkable(&scroller);
        surface.append(&scroller);

        let input_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(10)
            .margin_end(10)
            .margin_bottom(10)
            .build();
        let input = gtk::Entry::builder()
            .placeholder_text("Send command to this Windows terminal pane")
            .hexpand(true)
            .css_classes(["workspace-broadcast-entry"])
            .build();
        let send_button =
            icons::labeled_button("Send", icon_name::BROADCAST, &["flat", "surface-button"]);
        input_row.append(&input);
        input_row.append(&send_button);
        surface.append(&input_row);

        let (stdin_tx, stdin_rx) = mpsc::channel::<String>();
        let (output_tx, output_rx) = mpsc::channel::<String>();
        spawn_terminal_process(
            tile.clone(),
            tab.clone(),
            assets.clone(),
            stdin_rx,
            output_tx,
        );

        {
            let buffer = buffer.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
                while let Ok(chunk) = output_rx.try_recv() {
                    append_buffer_text(&buffer, &chunk);
                }
                gtk::glib::ControlFlow::Continue
            });
        }

        {
            let input = input.clone();
            let stdin_tx = stdin_tx.clone();
            send_button.connect_clicked(move |_| send_entry_text(&input, &stdin_tx));
        }
        {
            let stdin_tx = stdin_tx.clone();
            input.connect_activate(move |entry| send_entry_text(entry, &stdin_tx));
        }

        let command_sender = Rc::new({
            let stdin_tx = stdin_tx.clone();
            move |command: &str| {
                let command = command.trim();
                !command.is_empty() && stdin_tx.send(command.to_string()).is_ok()
            }
        });

        let appearance_applier = Rc::new({
            let appearance_provider = appearance_provider.clone();
            move |density, zoom_steps| {
                apply_terminal_runtime_appearance(&appearance_provider, density, zoom_steps);
            }
        });

        TileRuntimeSurface {
            widget: surface.upcast(),
            command_sender: Some(command_sender),
            appearance_applier: Some(appearance_applier),
        }
    }

    fn apply_terminal_runtime_appearance(
        provider: &gtk::CssProvider,
        density: ApplicationDensity,
        zoom_steps: i32,
    ) {
        provider.load_from_data(&format!(
            ".terminal-runtime-output {{ font-family: \"JetBrains Mono\", monospace; font-size: {}pt; }}",
            effective_terminal_font_points(density, zoom_steps)
        ));
    }

    fn clamp_terminal_zoom_steps(density: ApplicationDensity, zoom_steps: i32) -> i32 {
        let base_points = density.terminal_font_points();
        (base_points + zoom_steps).clamp(MIN_TERMINAL_FONT_POINTS, MAX_TERMINAL_FONT_POINTS)
            - base_points
    }

    fn effective_terminal_font_points(density: ApplicationDensity, zoom_steps: i32) -> i32 {
        density.terminal_font_points() + clamp_terminal_zoom_steps(density, zoom_steps)
    }

    fn spawn_terminal_process(
        tile: TileSpec,
        tab: SavedTab,
        assets: WorkspaceAssets,
        stdin_rx: mpsc::Receiver<String>,
        output_tx: mpsc::Sender<String>,
    ) {
        std::thread::spawn(move || {
            let launch =
                resolve_tile_launch(&tile, &tab.workspace_root, &assets).and_then(|resolved| {
                    let runtime = wsl::probe_runtime(None)?;
                    wsl::build_launch_command(&tile, &tab.workspace_root, &resolved, &runtime)
                });

            let command = match launch {
                Ok(command) => command,
                Err(error) => {
                    let _ =
                        output_tx.send(format!("\r\n[terminaltiler] launch failed: {error}\r\n"));
                    logging::error(format!(
                        "Windows GTK terminal runtime launch failed for tile {}: {error}",
                        tile.id
                    ));
                    return;
                }
            };

            let _ = output_tx.send(format!(
                "\r\n[terminaltiler] runtime: {:?}; cwd: {}\r\n> {} {}\r\n",
                command.runtime,
                command.working_directory,
                command.program,
                command.args.join(" ")
            ));

            let mut child = match Command::new(&command.program)
                .args(&command.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
            {
                Ok(child) => child,
                Err(error) => {
                    let _ = output_tx.send(format!(
                        "\r\n[terminaltiler] failed to spawn {}: {error}\r\n",
                        command.program
                    ));
                    logging::error(format!(
                        "Windows GTK terminal runtime failed to spawn '{}': {error}",
                        command.program
                    ));
                    return;
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                std::thread::spawn(move || {
                    while let Ok(line) = stdin_rx.recv() {
                        if stdin.write_all(line.as_bytes()).is_err() {
                            break;
                        }
                        if stdin.write_all(b"\r\n").is_err() {
                            break;
                        }
                        let _ = stdin.flush();
                    }
                });
            }

            if let Some(stdout) = child.stdout.take() {
                pipe_reader_to_output(stdout, output_tx.clone());
            }
            if let Some(stderr) = child.stderr.take() {
                pipe_reader_to_output(stderr, output_tx.clone());
            }

            match child.wait() {
                Ok(status) => {
                    let _ = output_tx.send(format!(
                        "\r\n[terminaltiler] terminal process exited with {status}\r\n"
                    ));
                }
                Err(error) => {
                    let _ = output_tx.send(format!(
                        "\r\n[terminaltiler] terminal wait failed: {error}\r\n"
                    ));
                }
            }
        });
    }

    fn pipe_reader_to_output<R>(reader: R, output_tx: mpsc::Sender<String>)
    where
        R: std::io::Read + Send + 'static,
    {
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let _ = output_tx.send(line.clone());
                    }
                    Err(error) => {
                        let _ = output_tx.send(format!(
                            "\r\n[terminaltiler] terminal output read failed: {error}\r\n"
                        ));
                        break;
                    }
                }
            }
        });
    }

    fn send_entry_text(entry: &gtk::Entry, stdin_tx: &mpsc::Sender<String>) {
        let text = entry.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        if stdin_tx.send(text).is_ok() {
            entry.set_text("");
        }
    }

    fn append_buffer_line(buffer: &gtk::TextBuffer, text: &str) {
        append_buffer_text(buffer, &format!("{text}\n"));
    }

    fn append_buffer_text(buffer: &gtk::TextBuffer, text: &str) {
        let mut end = buffer.end_iter();
        buffer.insert(&mut end, text);
    }

    fn build_web_runtime_surface(tile: &TileSpec) -> gtk::Widget {
        let url = normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
        let surface = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["terminal-surface", "windows-gtk-web-runtime"])
            .build();
        make_shrinkable(&surface);

        let title = gtk::Label::builder()
            .label(domain_from_url(&url))
            .halign(gtk::Align::Start)
            .margin_top(12)
            .margin_start(12)
            .css_classes(["tile-directory"])
            .build();
        surface.append(&title);

        let detail = match workspace::probe_webview2_runtime() {
            Ok(()) => format!("WebView2 available: {url}"),
            Err(error) => format!("WebView2 unavailable: {error}\n{url}"),
        };
        surface.append(
            &gtk::Label::builder()
                .label(&detail)
                .halign(gtk::Align::Start)
                .margin_start(12)
                .wrap(true)
                .css_classes(["tile-meta"])
                .build(),
        );

        let open_button =
            icons::labeled_button("Open Web Tile", icon_name::WEB, &["flat", "surface-button"]);
        open_button.set_halign(gtk::Align::Start);
        open_button.set_margin_start(12);
        {
            let url = url.clone();
            open_button.connect_clicked(move |_| {
                if let Err(error) =
                    gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                {
                    logging::error(format!(
                        "Windows GTK web runtime failed to open '{url}': {error}"
                    ));
                }
            });
        }
        surface.append(&open_button);

        surface.upcast()
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub(crate) use imp::build_tile_runtime_surface;
