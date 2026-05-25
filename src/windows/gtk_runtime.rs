#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::cell::{Cell, RefCell};
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
    use crate::ui::context_menu;
    use crate::ui::icons::{self, name as icon_name};
    use crate::ui::tile_chrome::{domain_from_url, make_shrinkable};
    use crate::ui::workspace_preview::{TileRuntimeRecoveryBinder, TileRuntimeSurface};
    use crate::windows::{workspace, wsl};

    const MIN_TERMINAL_FONT_POINTS: i32 = 7;
    const MAX_TERMINAL_FONT_POINTS: i32 = 20;
    const DEFAULT_TERMINAL_COPY_SHORTCUT: &str = "<Ctrl><Shift>C";
    const DEFAULT_TERMINAL_PASTE_SHORTCUT: &str = "<Ctrl><Shift>V";
    const TERMINAL_RUNTIME_POLL_MS: u64 = 80;

    #[derive(Default)]
    struct TerminalRuntimeState {
        stdin_tx: Option<mpsc::Sender<String>>,
        active: bool,
        next_generation: u64,
        active_generation: u64,
    }

    enum TerminalRuntimeEvent {
        Output(String),
        ProcessEnded { generation: u64 },
    }

    pub(crate) fn build_tile_runtime_surface(
        tile: &TileSpec,
        tab: &SavedTab,
        assets: &WorkspaceAssets,
    ) -> TileRuntimeSurface {
        match tile.tile_kind {
            TileKind::Terminal => build_terminal_runtime_surface(tile, tab, assets),
            TileKind::WebView => build_web_runtime_surface(tile),
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

        let state = Rc::new(RefCell::new(TerminalRuntimeState::default()));
        let recovery_bind_generation = Rc::new(Cell::new(0u64));
        let (event_tx, event_rx) = mpsc::channel::<TerminalRuntimeEvent>();
        start_terminal_process(
            &state,
            tile.clone(),
            tab.clone(),
            assets.clone(),
            event_tx.clone(),
        );

        {
            let buffer = buffer.clone();
            let state = state.clone();
            gtk::glib::timeout_add_local(
                Duration::from_millis(TERMINAL_RUNTIME_POLL_MS),
                move || {
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            TerminalRuntimeEvent::Output(chunk) => {
                                append_buffer_text(&buffer, &chunk)
                            }
                            TerminalRuntimeEvent::ProcessEnded { generation } => {
                                let mut state = state.borrow_mut();
                                if state.active_generation == generation {
                                    state.active = false;
                                    state.stdin_tx = None;
                                }
                            }
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                },
            );
        }

        {
            let input = input.clone();
            let state = state.clone();
            send_button.connect_clicked(move |_| send_entry_text(&input, &state));
        }
        {
            let state = state.clone();
            input.connect_activate(move |entry| send_entry_text(entry, &state));
        }

        let restart_runtime = Rc::new({
            let state = state.clone();
            let tile = tile.clone();
            let tab = tab.clone();
            let assets = assets.clone();
            let event_tx = event_tx.clone();
            move || {
                start_terminal_process(
                    &state,
                    tile.clone(),
                    tab.clone(),
                    assets.clone(),
                    event_tx.clone(),
                );
            }
        });
        install_terminal_output_context_menu(
            &terminal_output,
            &input,
            &state,
            restart_runtime.clone(),
        );
        install_terminal_output_shortcuts(&terminal_output, &input, &state);

        let command_sender = Rc::new({
            let state = state.clone();
            move |command: &str| send_terminal_runtime_payload(&state, command.to_string())
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
            url_applier: None,
            recovery_binder: Some(TileRuntimeRecoveryBinder {
                bind: Rc::new({
                    let state = state.clone();
                    let restart_runtime = restart_runtime.clone();
                    let recovery_bind_generation = recovery_bind_generation.clone();
                    move |shell, status, recovery_button| {
                        bind_terminal_recovery_controls(
                            shell,
                            status,
                            recovery_button,
                            state.clone(),
                            restart_runtime.clone(),
                            recovery_bind_generation.clone(),
                        );
                    }
                }),
            }),
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

    fn start_terminal_process(
        state: &Rc<RefCell<TerminalRuntimeState>>,
        tile: TileSpec,
        tab: SavedTab,
        assets: WorkspaceAssets,
        event_tx: mpsc::Sender<TerminalRuntimeEvent>,
    ) {
        if state.borrow().active {
            return;
        }
        let (stdin_tx, stdin_rx) = mpsc::channel::<String>();
        let generation = {
            let mut state = state.borrow_mut();
            state.next_generation = state.next_generation.saturating_add(1);
            state.active_generation = state.next_generation;
            state.active = true;
            state.stdin_tx = Some(stdin_tx);
            state.active_generation
        };
        if generation > 1 {
            let _ = event_tx.send(TerminalRuntimeEvent::Output(
                "\r\n[terminaltiler] reconnecting terminal session\r\n".into(),
            ));
        }
        spawn_terminal_process(tile, tab, assets, generation, stdin_rx, event_tx);
    }

    fn spawn_terminal_process(
        tile: TileSpec,
        tab: SavedTab,
        assets: WorkspaceAssets,
        generation: u64,
        stdin_rx: mpsc::Receiver<String>,
        event_tx: mpsc::Sender<TerminalRuntimeEvent>,
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
                    let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                        "\r\n[terminaltiler] launch failed: {error}\r\n"
                    )));
                    let _ = event_tx.send(TerminalRuntimeEvent::ProcessEnded { generation });
                    logging::error(format!(
                        "Windows GTK terminal runtime launch failed for tile {}: {error}",
                        tile.id
                    ));
                    return;
                }
            };

            let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                "\r\n[terminaltiler] runtime: {:?}; cwd: {}\r\n> {} {}\r\n",
                command.runtime,
                command.working_directory,
                command.program,
                command.args.join(" ")
            )));

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
                    let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                        "\r\n[terminaltiler] failed to spawn {}: {error}\r\n",
                        command.program
                    )));
                    let _ = event_tx.send(TerminalRuntimeEvent::ProcessEnded { generation });
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
                        let _ = stdin.flush();
                    }
                });
            }

            if let Some(stdout) = child.stdout.take() {
                pipe_reader_to_output(stdout, event_tx.clone());
            }
            if let Some(stderr) = child.stderr.take() {
                pipe_reader_to_output(stderr, event_tx.clone());
            }

            match child.wait() {
                Ok(status) => {
                    let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                        "\r\n[terminaltiler] terminal process exited with {status}\r\n"
                    )));
                }
                Err(error) => {
                    let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                        "\r\n[terminaltiler] terminal wait failed: {error}\r\n"
                    )));
                }
            }
            let _ = event_tx.send(TerminalRuntimeEvent::ProcessEnded { generation });
        });
    }

    fn pipe_reader_to_output<R>(reader: R, event_tx: mpsc::Sender<TerminalRuntimeEvent>)
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
                        let _ = event_tx.send(TerminalRuntimeEvent::Output(line.clone()));
                    }
                    Err(error) => {
                        let _ = event_tx.send(TerminalRuntimeEvent::Output(format!(
                            "\r\n[terminaltiler] terminal output read failed: {error}\r\n"
                        )));
                        break;
                    }
                }
            }
        });
    }

    fn send_entry_text(entry: &gtk::Entry, state: &Rc<RefCell<TerminalRuntimeState>>) {
        let text = entry.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let payload = if text.ends_with('\n') {
            text
        } else {
            format!("{text}\r\n")
        };
        if send_terminal_runtime_payload(state, payload) {
            entry.set_text("");
        }
    }

    fn send_terminal_runtime_payload(
        state: &Rc<RefCell<TerminalRuntimeState>>,
        payload: String,
    ) -> bool {
        if payload.is_empty() {
            return false;
        }

        let stdin_tx = {
            let state = state.borrow();
            if !state.active {
                return false;
            }
            state.stdin_tx.clone()
        };

        if stdin_tx.is_some_and(|stdin_tx| stdin_tx.send(payload).is_ok()) {
            true
        } else {
            let mut state = state.borrow_mut();
            state.active = false;
            state.stdin_tx = None;
            false
        }
    }

    fn append_buffer_line(buffer: &gtk::TextBuffer, text: &str) {
        append_buffer_text(buffer, &format!("{text}\n"));
    }

    fn append_buffer_text(buffer: &gtk::TextBuffer, text: &str) {
        let mut end = buffer.end_iter();
        buffer.insert(&mut end, text);
    }

    fn install_terminal_output_context_menu(
        output: &gtk::TextView,
        input: &gtk::Entry,
        state: &Rc<RefCell<TerminalRuntimeState>>,
        restart_runtime: Rc<dyn Fn()>,
    ) {
        let popover = context_menu::popover(output);
        let menu = context_menu::menu_box();

        let copy_button = context_menu::action_button("Copy", Some("Ctrl+Shift+C"));
        copy_button.set_sensitive(output.buffer().has_selection());
        {
            let output = output.clone();
            let popover = popover.clone();
            copy_button.connect_clicked(move |_| {
                copy_terminal_output_selection(&output);
                popover.popdown();
            });
        }
        {
            let copy_button = copy_button.clone();
            output.buffer().connect_has_selection_notify(move |buffer| {
                copy_button.set_sensitive(buffer.has_selection());
            });
        }
        menu.append(&copy_button);

        let paste_button = context_menu::action_button("Paste", Some("Ctrl+Shift+V"));
        {
            let output = output.clone();
            let state = state.clone();
            let popover = popover.clone();
            paste_button.connect_clicked(move |_| {
                paste_clipboard_into_terminal_runtime(&output, &state);
                popover.popdown();
            });
        }
        menu.append(&paste_button);

        let reconnect_button = context_menu::action_button("Reconnect", None);
        {
            let restart_runtime = restart_runtime.clone();
            let popover = popover.clone();
            reconnect_button.connect_clicked(move |_| {
                restart_runtime();
                popover.popdown();
            });
        }
        menu.append(&reconnect_button);

        let focus_input_button = context_menu::action_button("Focus Command Input", None);
        {
            let input = input.clone();
            let popover = popover.clone();
            focus_input_button.connect_clicked(move |_| {
                input.grab_focus();
                popover.popdown();
            });
        }
        menu.append(&focus_input_button);

        popover.set_child(Some(&menu));

        let right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let output = output.clone();
            let popover = popover.clone();
            let copy_button = copy_button.clone();
            let paste_button = paste_button.clone();
            let reconnect_button = reconnect_button.clone();
            let state = state.clone();
            right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                output.grab_focus();
                copy_button.set_sensitive(output.buffer().has_selection());
                let active = state.borrow().active;
                paste_button.set_sensitive(active);
                reconnect_button.set_sensitive(!active);
                context_menu::popup_at(&popover, x, y);
            });
        }
        output.add_controller(right_click);
    }

    fn install_terminal_output_shortcuts(
        output: &gtk::TextView,
        input: &gtk::Entry,
        state: &Rc<RefCell<TerminalRuntimeState>>,
    ) {
        let shortcut_controller = gtk::ShortcutController::new();
        shortcut_controller.set_scope(gtk::ShortcutScope::Local);

        let output_for_copy = output.clone();
        let copy_action = gtk::CallbackAction::new(move |_, _| {
            if copy_terminal_output_selection(&output_for_copy) {
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        add_terminal_output_shortcut(
            &shortcut_controller,
            DEFAULT_TERMINAL_COPY_SHORTCUT,
            "copy",
            &copy_action,
        );

        let output_for_paste = output.clone();
        let state_for_paste = state.clone();
        let paste_action = gtk::CallbackAction::new(move |_, _| {
            paste_clipboard_into_terminal_runtime(&output_for_paste, &state_for_paste);
            gtk::glib::Propagation::Stop
        });
        add_terminal_output_shortcut(
            &shortcut_controller,
            DEFAULT_TERMINAL_PASTE_SHORTCUT,
            "paste",
            &paste_action,
        );

        output.add_controller(shortcut_controller);

        let input_shortcut_controller = gtk::ShortcutController::new();
        input_shortcut_controller.set_scope(gtk::ShortcutScope::Local);
        let output_for_copy = output.clone();
        let copy_action = gtk::CallbackAction::new(move |_, _| {
            if copy_terminal_output_selection(&output_for_copy) {
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        add_terminal_output_shortcut(
            &input_shortcut_controller,
            DEFAULT_TERMINAL_COPY_SHORTCUT,
            "copy",
            &copy_action,
        );
        input.add_controller(input_shortcut_controller);
    }

    fn copy_terminal_output_selection(output: &gtk::TextView) -> bool {
        let buffer = output.buffer();
        if !buffer.has_selection() {
            return false;
        }

        output.grab_focus();
        buffer.copy_clipboard(&output.clipboard());
        true
    }

    fn paste_clipboard_into_terminal_runtime(
        output: &gtk::TextView,
        state: &Rc<RefCell<TerminalRuntimeState>>,
    ) {
        output.grab_focus();
        let state = state.clone();
        output
            .clipboard()
            .read_text_async(None::<&gio::Cancellable>, move |result| match result {
                Ok(Some(text)) => {
                    let text = text.to_string();
                    if !text.is_empty() {
                        let payload = if text.ends_with('\n') {
                            text
                        } else {
                            format!("{text}\r\n")
                        };
                        let _ = send_terminal_runtime_payload(&state, payload);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    logging::error(format!(
                        "Windows GTK terminal runtime clipboard paste failed: {error}"
                    ));
                }
            });
    }

    fn add_terminal_output_shortcut(
        shortcut_controller: &gtk::ShortcutController,
        accelerator: &str,
        shortcut_name: &str,
        action: &gtk::CallbackAction,
    ) {
        let Some(trigger) = gtk::ShortcutTrigger::parse_string(accelerator) else {
            logging::error(format!(
                "failed to parse Windows GTK terminal {shortcut_name} shortcut '{accelerator}'"
            ));
            return;
        };

        shortcut_controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action.clone())));
    }

    fn bind_terminal_recovery_controls(
        shell: &gtk::Box,
        status: &gtk::Label,
        recovery_button: &gtk::Button,
        state: Rc<RefCell<TerminalRuntimeState>>,
        restart_runtime: Rc<dyn Fn()>,
        bind_generation: Rc<Cell<u64>>,
    ) {
        let current_generation = bind_generation.get().saturating_add(1);
        bind_generation.set(current_generation);

        let active_status_line = status.text().to_string();
        let active_status_tooltip = status.tooltip_text().map(|value| value.to_string());
        let popover = build_terminal_recovery_popover(
            recovery_button,
            restart_runtime,
            active_status_line.clone(),
        );

        {
            let popover = popover.clone();
            recovery_button.connect_clicked(move |_| {
                popover.popup();
            });
        }

        sync_terminal_recovery_state(
            shell,
            status,
            recovery_button,
            &state,
            &active_status_line,
            &active_status_tooltip,
        );

        let shell_weak = shell.downgrade();
        let status_weak = status.downgrade();
        let recovery_button_weak = recovery_button.downgrade();
        gtk::glib::timeout_add_local(Duration::from_millis(TERMINAL_RUNTIME_POLL_MS), move || {
            if bind_generation.get() != current_generation {
                return gtk::glib::ControlFlow::Break;
            }
            let (Some(shell), Some(status), Some(recovery_button)) = (
                shell_weak.upgrade(),
                status_weak.upgrade(),
                recovery_button_weak.upgrade(),
            ) else {
                return gtk::glib::ControlFlow::Break;
            };
            sync_terminal_recovery_state(
                &shell,
                &status,
                &recovery_button,
                &state,
                &active_status_line,
                &active_status_tooltip,
            );
            gtk::glib::ControlFlow::Continue
        });
    }

    fn sync_terminal_recovery_state(
        shell: &gtk::Box,
        status: &gtk::Label,
        recovery_button: &gtk::Button,
        state: &Rc<RefCell<TerminalRuntimeState>>,
        active_status_line: &str,
        active_status_tooltip: &Option<String>,
    ) {
        if state.borrow().active {
            shell.remove_css_class("is-disconnected");
            status.remove_css_class("recovery-chip");
            status.set_text(active_status_line);
            status.set_tooltip_text(active_status_tooltip.as_deref());
            recovery_button.set_visible(false);
            recovery_button.set_sensitive(false);
        } else {
            shell.add_css_class("is-disconnected");
            status.add_css_class("recovery-chip");
            status.set_text("Disconnected  •  Reconnect session");
            status.set_tooltip_text(Some(
                "This Windows GTK terminal process exited. Reconnect the configured session.",
            ));
            recovery_button.set_visible(true);
            recovery_button.set_sensitive(true);
        }
    }

    fn build_terminal_recovery_popover(
        recovery_button: &gtk::Button,
        restart_runtime: Rc<dyn Fn()>,
        active_status_line: String,
    ) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.add_css_class("terminal-recovery-popover");
        popover.set_autohide(true);
        popover.set_has_arrow(true);
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_parent(recovery_button);

        let shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(10)
            .margin_end(10)
            .build();
        shell.append(
            &gtk::Label::builder()
                .label("Session ended")
                .halign(gtk::Align::Start)
                .css_classes(["card-title"])
                .build(),
        );
        shell.append(
            &gtk::Label::builder()
                .label(format!(
                    "{}\nReconnect the configured session in this pane.",
                    active_status_line
                ))
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["field-hint"])
                .build(),
        );

        let reconnect_button = icons::labeled_button(
            "Reconnect Session",
            icon_name::RECOVER,
            &["flat", "surface-button"],
        );
        reconnect_button.set_focus_on_click(false);
        {
            let popover = popover.clone();
            reconnect_button.connect_clicked(move |_| {
                restart_runtime();
                popover.popdown();
            });
        }
        shell.append(&reconnect_button);

        popover.set_child(Some(&shell));
        popover
    }

    fn build_web_runtime_surface(tile: &TileSpec) -> TileRuntimeSurface {
        let url = normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));
        let current_url = Rc::new(RefCell::new(url.clone()));
        let runtime_status = match workspace::probe_webview2_runtime() {
            Ok(()) => "WebView2 available".to_string(),
            Err(error) => format!("WebView2 unavailable: {error}"),
        };
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
        title.set_tooltip_text(Some(&url));
        surface.append(&title);

        let detail = web_runtime_detail(&runtime_status, &url);
        let detail_label = gtk::Label::builder()
            .label(&detail)
            .halign(gtk::Align::Start)
            .margin_start(12)
            .wrap(true)
            .css_classes(["tile-meta"])
            .build();
        surface.append(&detail_label);

        let open_button =
            icons::labeled_button("Open Web Tile", icon_name::WEB, &["flat", "surface-button"]);
        open_button.set_halign(gtk::Align::Start);
        open_button.set_margin_start(12);
        {
            let current_url = current_url.clone();
            open_button.connect_clicked(move |_| {
                let url = current_url.borrow().clone();
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

        let url_applier = Rc::new({
            let current_url = current_url.clone();
            let runtime_status = runtime_status.clone();
            let title = title.clone();
            let detail_label = detail_label.clone();
            move |url: &str| {
                let url = normalize_web_url(url);
                if current_url.borrow().as_str() == url {
                    return;
                }
                current_url.replace(url.clone());
                let domain = domain_from_url(&url);
                title.set_text(&domain);
                title.set_tooltip_text(Some(&url));
                detail_label.set_text(&web_runtime_detail(&runtime_status, &url));
            }
        });

        TileRuntimeSurface {
            widget: surface.upcast(),
            command_sender: None,
            appearance_applier: None,
            url_applier: Some(url_applier),
            recovery_binder: None,
        }
    }

    fn web_runtime_detail(runtime_status: &str, url: &str) -> String {
        format!("{runtime_status}: {url}")
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub(crate) use imp::build_tile_runtime_surface;
