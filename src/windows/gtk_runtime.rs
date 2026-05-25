#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::io::{Read, Write};
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::rc::Rc;
    use std::sync::OnceLock;
    use std::sync::mpsc;
    use std::time::Duration;

    use adw::prelude::*;
    use gtk::glib::translate::ToGlibPtr;
    use gtk::{gio, glib};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2_11,
        ICoreWebView2Controller, ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
        ICoreWebView2Environment, ICoreWebView2NewWindowRequestedEventArgs,
    };
    use webview2_com::{
        ContextMenuRequestedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler, DocumentTitleChangedEventHandler,
        NavigationCompletedEventHandler, NewWindowRequestedEventHandler, take_pwstr,
        wait_with_pump,
    };
    use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND as Win32Hwnd, RECT as WinRect};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::System::WinRT::EventRegistrationToken;
    use windows::core::{Error as WindowsError, HSTRING, Interface, PCWSTR, PWSTR};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    use windows_sys::Win32::System::Threading::TerminateProcess;

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
    use crate::windows::vt::VtBuffer;
    use crate::windows::{workspace, wsl};

    const MIN_TERMINAL_FONT_POINTS: i32 = 7;
    const MAX_TERMINAL_FONT_POINTS: i32 = 20;
    const DEFAULT_TERMINAL_COPY_SHORTCUT: &str = "<Ctrl><Shift>C";
    const DEFAULT_TERMINAL_PASTE_SHORTCUT: &str = "<Ctrl><Shift>V";
    const TERMINAL_RUNTIME_COLUMNS: usize = 80;
    const TERMINAL_RUNTIME_ROWS: usize = 24;
    const TERMINAL_RUNTIME_POLL_MS: u64 = 80;
    const WEBVIEW_RUNTIME_POLL_MS: u64 = 100;

    unsafe extern "C" {
        fn gdk_win32_surface_get_handle(surface: *mut gdk::ffi::GdkSurface) -> *mut c_void;
    }

    #[derive(Default)]
    struct TerminalRuntimeState {
        stdin_tx: Option<mpsc::Sender<String>>,
        active: bool,
        process_handle: Option<isize>,
        next_generation: u64,
        active_generation: u64,
    }

    enum TerminalRuntimeEvent {
        Output(String),
        ProcessStarted {
            generation: u64,
            process_handle: isize,
        },
        ProcessEnded {
            generation: u64,
        },
    }

    #[derive(Default)]
    struct WebRuntimeState {
        environment: Option<ICoreWebView2Environment>,
        controller: Option<ICoreWebView2Controller>,
        webview: Option<ICoreWebView2>,
        document_title_token: Option<EventRegistrationToken>,
        navigation_completed_token: Option<EventRegistrationToken>,
        new_window_token: Option<EventRegistrationToken>,
        context_menu_token: Option<EventRegistrationToken>,
        current_url: String,
        auto_refresh_seconds: Option<u32>,
        refresh_tick: u32,
        last_bounds: Option<WinRect>,
        initialized: bool,
        shutdown: bool,
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
        let terminal_buffer = Rc::new(RefCell::new(VtBuffer::new(
            TERMINAL_RUNTIME_COLUMNS,
            TERMINAL_RUNTIME_ROWS,
        )));
        {
            let mut terminal_buffer = terminal_buffer.borrow_mut();
            terminal_buffer.process(&format!(
                "[terminaltiler] starting {} in {}\r\n",
                tile.title,
                tab.workspace_root.display()
            ));
            render_terminal_runtime_buffer(&buffer, &terminal_buffer);
        }

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
            let terminal_buffer = terminal_buffer.clone();
            let state = state.clone();
            let terminal_output = terminal_output.clone();
            gtk::glib::timeout_add_local(
                Duration::from_millis(TERMINAL_RUNTIME_POLL_MS),
                move || {
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            TerminalRuntimeEvent::Output(chunk) => {
                                let mut terminal_buffer = terminal_buffer.borrow_mut();
                                terminal_buffer.process(&chunk);
                                flush_terminal_runtime_responses(
                                    &mut terminal_buffer,
                                    &state,
                                    &terminal_output,
                                );
                                render_terminal_runtime_buffer(&buffer, &terminal_buffer);
                            }
                            TerminalRuntimeEvent::ProcessStarted {
                                generation,
                                process_handle,
                            } => {
                                let terminate_stale_process = {
                                    let mut state = state.borrow_mut();
                                    if state.active_generation == generation && state.active {
                                        state.process_handle = Some(process_handle);
                                        false
                                    } else {
                                        true
                                    }
                                };
                                if terminate_stale_process && process_handle != 0 {
                                    let _ = unsafe { TerminateProcess(process_handle as _, 1) };
                                }
                            }
                            TerminalRuntimeEvent::ProcessEnded { generation } => {
                                let mut state = state.borrow_mut();
                                if state.active_generation == generation {
                                    state.active = false;
                                    state.stdin_tx = None;
                                    state.process_handle = None;
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
        let shutdown = Rc::new({
            let state = state.clone();
            move |reason: &str| terminate_terminal_runtime(&state, reason)
        });
        let active_process_checker = Rc::new({
            let state = state.clone();
            move || {
                let state = state.borrow();
                state.active || state.process_handle.is_some()
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
            url_applier: None,
            web_settings_applier: None,
            shutdown: Some(shutdown),
            active_process_checker: Some(active_process_checker),
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
            state.process_handle = None;
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
            let _ = event_tx.send(TerminalRuntimeEvent::ProcessStarted {
                generation,
                process_handle: child.as_raw_handle() as isize,
            });

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

    fn pipe_reader_to_output<R>(mut reader: R, event_tx: mpsc::Sender<TerminalRuntimeEvent>)
    where
        R: std::io::Read + Send + 'static,
    {
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(bytes_read) => {
                        let text = String::from_utf8_lossy(&chunk[..bytes_read]).into_owned();
                        let _ = event_tx.send(TerminalRuntimeEvent::Output(text));
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

    fn render_terminal_runtime_buffer(buffer: &gtk::TextBuffer, terminal: &VtBuffer) {
        let total_rows = terminal.total_rows();
        let mut rendered = String::with_capacity((terminal.columns() + 1) * total_rows);
        let cursor = terminal.cursor_visible().then(|| {
            let (column, row) = terminal.cursor();
            (column, terminal.history_len() + row)
        });

        for row in 0..total_rows {
            if row > 0 {
                rendered.push('\n');
            }
            for column in 0..terminal.columns() {
                let mut ch = terminal.display_cell(row, column).ch;
                if cursor == Some((column, row)) {
                    ch = if ch == ' ' { '█' } else { '▌' };
                }
                rendered.push(ch);
            }
        }

        buffer.set_text(&rendered);
    }

    fn flush_terminal_runtime_responses(
        terminal: &mut VtBuffer,
        state: &Rc<RefCell<TerminalRuntimeState>>,
        output: &gtk::TextView,
    ) {
        let pending_input = terminal.take_pending_input();
        if !pending_input.is_empty() {
            let response = String::from_utf8_lossy(&pending_input).into_owned();
            let _ = send_terminal_runtime_payload(state, response);
        }

        if let Some(clipboard_text) = terminal.take_pending_clipboard_write() {
            output.clipboard().set_text(&clipboard_text);
        }
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
            state.stdin_tx = None;
            false
        }
    }

    fn terminate_terminal_runtime(state: &Rc<RefCell<TerminalRuntimeState>>, reason: &str) {
        let process_handle = {
            let mut state = state.borrow_mut();
            if !state.active && state.process_handle.is_none() {
                return;
            }
            logging::info(format!(
                "terminating Windows GTK terminal runtime reason='{reason}' generation={}",
                state.active_generation
            ));
            state.active = false;
            state.stdin_tx = None;
            state.process_handle.take()
        };

        if let Some(process_handle) = process_handle
            && process_handle != 0
        {
            let terminated = unsafe { TerminateProcess(process_handle as _, 1) };
            if terminated == 0 {
                logging::error(format!(
                    "Windows GTK terminal runtime termination failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
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
        let state = Rc::new(RefCell::new(WebRuntimeState {
            current_url: url.clone(),
            auto_refresh_seconds: tile.auto_refresh_seconds,
            ..WebRuntimeState::default()
        }));
        let runtime_status = match workspace::probe_webview2_runtime() {
            Ok(()) => "Embedding WebView2 content".to_string(),
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

        let detail = web_runtime_detail(&runtime_status, &url, tile.auto_refresh_seconds);
        let detail_label = gtk::Label::builder()
            .label(&detail)
            .halign(gtk::Align::Start)
            .margin_start(12)
            .wrap(true)
            .css_classes(["tile-meta"])
            .build();
        surface.append(&detail_label);

        let web_host = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["web-tile-frame", "windows-gtk-webview-host"])
            .build();
        make_shrinkable(&web_host);
        surface.append(&web_host);

        let context_popover = build_web_runtime_context_menu(&web_host, state.clone());

        let open_button = icons::labeled_button(
            "Open Externally",
            icon_name::WEB,
            &["flat", "surface-button"],
        );
        open_button.set_halign(gtk::Align::Start);
        open_button.set_margin_start(12);
        {
            let state = state.clone();
            open_button.connect_clicked(move |_| {
                let url = state.borrow().current_url.clone();
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
            let state = state.clone();
            let runtime_status = runtime_status.clone();
            let title = title.clone();
            let detail_label = detail_label.clone();
            move |url: &str| {
                apply_web_runtime_settings(
                    &state,
                    &runtime_status,
                    &title,
                    &detail_label,
                    url,
                    None,
                    false,
                );
            }
        });
        let web_settings_applier = Rc::new({
            let state = state.clone();
            let runtime_status = runtime_status.clone();
            let title = title.clone();
            let detail_label = detail_label.clone();
            move |url: &str, auto_refresh_seconds: Option<u32>| {
                apply_web_runtime_settings(
                    &state,
                    &runtime_status,
                    &title,
                    &detail_label,
                    url,
                    auto_refresh_seconds,
                    true,
                );
            }
        });
        let shutdown = Rc::new({
            let state = state.clone();
            move |reason: &str| shutdown_web_runtime(&state, reason)
        });

        start_web_runtime_pump(
            &web_host.clone().upcast::<gtk::Widget>(),
            state.clone(),
            runtime_status.clone(),
            title.clone(),
            detail_label.clone(),
            context_popover,
        );

        TileRuntimeSurface {
            widget: surface.upcast(),
            command_sender: None,
            appearance_applier: None,
            url_applier: Some(url_applier),
            web_settings_applier: Some(web_settings_applier),
            shutdown: Some(shutdown),
            active_process_checker: None,
            recovery_binder: None,
        }
    }

    fn build_web_runtime_context_menu(
        parent: &gtk::Box,
        state: Rc<RefCell<WebRuntimeState>>,
    ) -> gtk::Popover {
        let popover = context_menu::popover(parent);
        let menu = context_menu::menu_box();

        let reload_button = context_menu::action_button("Reload", Some("F5"));
        {
            let state = state.clone();
            let popover = popover.clone();
            reload_button.connect_clicked(move |_| {
                if let Some(webview) = state.borrow().webview.clone()
                    && let Err(error) = unsafe { webview.Reload() }
                {
                    logging::error(format!(
                        "Windows GTK WebView2 context reload failed: {error}"
                    ));
                }
                popover.popdown();
            });
        }
        menu.append(&reload_button);

        let copy_url_button = context_menu::action_button("Copy URL", None);
        {
            let state = state.clone();
            let popover = popover.clone();
            let parent = parent.clone();
            copy_url_button.connect_clicked(move |_| {
                let url = state.borrow().current_url.clone();
                if !url.trim().is_empty() {
                    parent.display().clipboard().set_text(&url);
                }
                popover.popdown();
            });
        }
        menu.append(&copy_url_button);

        let open_external_button = context_menu::action_button("Open in Browser", None);
        {
            let state = state.clone();
            let popover = popover.clone();
            open_external_button.connect_clicked(move |_| {
                let url = state.borrow().current_url.clone();
                if !url.trim().is_empty()
                    && let Err(error) =
                        gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
                {
                    logging::error(format!(
                        "Windows GTK WebView2 context open failed for '{url}': {error}"
                    ));
                }
                popover.popdown();
            });
        }
        menu.append(&open_external_button);

        popover.set_child(Some(&menu));
        popover
    }

    fn apply_web_runtime_settings(
        state: &Rc<RefCell<WebRuntimeState>>,
        runtime_status: &str,
        title: &gtk::Label,
        detail_label: &gtk::Label,
        url: &str,
        auto_refresh_seconds: Option<u32>,
        update_refresh: bool,
    ) {
        let url = normalize_web_url(url);
        let (should_navigate, next_refresh) = {
            let mut state = state.borrow_mut();
            let should_navigate = state.current_url != url;
            if should_navigate {
                state.current_url = url.clone();
            }
            if update_refresh {
                state.auto_refresh_seconds = auto_refresh_seconds;
                state.refresh_tick = 0;
            }
            (should_navigate, state.auto_refresh_seconds)
        };

        let domain = domain_from_url(&url);
        title.set_text(&domain);
        title.set_tooltip_text(Some(&url));
        detail_label.set_text(&web_runtime_detail(runtime_status, &url, next_refresh));

        if should_navigate
            && let Some(webview) = state.borrow().webview.clone()
            && let Err(error) = unsafe { webview.Navigate(&HSTRING::from(url.as_str())) }
        {
            logging::error(format!("Windows GTK WebView2 navigation failed: {error}"));
        }
    }

    fn start_web_runtime_pump(
        host: &gtk::Widget,
        state: Rc<RefCell<WebRuntimeState>>,
        runtime_status: String,
        title: gtk::Label,
        detail_label: gtk::Label,
        context_popover: gtk::Popover,
    ) {
        let host = host.clone();
        glib::timeout_add_local(Duration::from_millis(WEBVIEW_RUNTIME_POLL_MS), move || {
            if state.borrow().shutdown {
                return glib::ControlFlow::Break;
            }

            if !state.borrow().initialized {
                if let Some(hwnd) = gtk_widget_hwnd(&host) {
                    initialize_gtk_webview_runtime(
                        hwnd,
                        &host,
                        &state,
                        &runtime_status,
                        &title,
                        &detail_label,
                        &context_popover,
                    );
                }
            }

            sync_gtk_webview_bounds(&host, &state);
            tick_gtk_webview_refresh(&state);
            glib::ControlFlow::Continue
        });
    }

    fn initialize_gtk_webview_runtime(
        parent_hwnd: windows_sys::Win32::Foundation::HWND,
        host: &gtk::Widget,
        state: &Rc<RefCell<WebRuntimeState>>,
        runtime_status: &str,
        title: &gtk::Label,
        detail_label: &gtk::Label,
        context_popover: &gtk::Popover,
    ) {
        {
            let mut state = state.borrow_mut();
            if state.initialized || state.shutdown {
                return;
            }
            state.initialized = true;
        }

        let environment = match create_webview_environment() {
            Ok(environment) => environment,
            Err(error) => {
                detail_label.set_text(&web_runtime_detail(
                    &error,
                    &state.borrow().current_url,
                    None,
                ));
                logging::error(format!("Windows GTK WebView2 environment failed: {error}"));
                return;
            }
        };
        let controller = match create_webview_controller(parent_hwnd, &environment) {
            Ok(controller) => controller,
            Err(error) => {
                detail_label.set_text(&web_runtime_detail(
                    &error,
                    &state.borrow().current_url,
                    None,
                ));
                logging::error(format!("Windows GTK WebView2 controller failed: {error}"));
                return;
            }
        };
        let webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => webview,
            Err(error) => {
                detail_label.set_text(&web_runtime_detail(
                    &format!("WebView2 controller access failed: {error}"),
                    &state.borrow().current_url,
                    None,
                ));
                logging::error(format!(
                    "Windows GTK WebView2 controller access failed: {error}"
                ));
                return;
            }
        };

        configure_gtk_webview(&webview);
        {
            let mut state = state.borrow_mut();
            state.environment = Some(environment);
            state.controller = Some(controller);
            state.webview = Some(webview.clone());
        }
        bind_gtk_webview_status(
            &webview,
            state.clone(),
            runtime_status.to_string(),
            title.clone(),
            detail_label.clone(),
        );
        bind_gtk_webview_interactions(&webview, state.clone(), context_popover.clone());

        let (url, auto_refresh_seconds) = {
            let state = state.borrow();
            (state.current_url.clone(), state.auto_refresh_seconds)
        };
        detail_label.set_text(&web_runtime_detail(
            runtime_status,
            &url,
            auto_refresh_seconds,
        ));
        sync_gtk_webview_bounds(host, state);
        logging::info(format!("Windows GTK WebView2 tile navigating to {url}"));
        if let Err(error) = unsafe { webview.Navigate(&HSTRING::from(url.as_str())) } {
            logging::error(format!(
                "Windows GTK WebView2 initial navigation failed: {error}"
            ));
        }
    }

    fn configure_gtk_webview(webview: &ICoreWebView2) {
        unsafe {
            let Ok(settings) = webview.Settings() else {
                return;
            };
            let _ = settings.SetIsStatusBarEnabled(false);
            let _ = settings.SetIsZoomControlEnabled(false);
        }
    }

    fn bind_gtk_webview_interactions(
        webview: &ICoreWebView2,
        state: Rc<RefCell<WebRuntimeState>>,
        context_popover: gtk::Popover,
    ) {
        let mut new_window_token = EventRegistrationToken::default();
        let new_window_registration = unsafe {
            webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    handle_gtk_webview_new_window_request(&args)
                })),
                &mut new_window_token,
            )
        };

        let mut context_menu_token = EventRegistrationToken::default();
        let context_menu_registration = match webview.cast::<ICoreWebView2_11>() {
            Ok(webview11) => Some(unsafe {
                webview11.add_ContextMenuRequested(
                    &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else {
                            return Ok(());
                        };
                        let mut point = windows::Win32::Foundation::POINT::default();
                        args.Location(&mut point)?;
                        args.SetHandled(true)?;
                        context_menu::popup_at(&context_popover, point.x as f64, point.y as f64);
                        Ok(())
                    })),
                    &mut context_menu_token,
                )
            }),
            Err(error) => {
                logging::info(format!(
                    "Windows GTK WebView2 context menu hook unavailable: {error}"
                ));
                None
            }
        };

        let mut state = state.borrow_mut();
        if let Err(error) = new_window_registration {
            logging::error(format!(
                "Registering Windows GTK WebView2 popup handler failed: {error}"
            ));
        } else {
            state.new_window_token = Some(new_window_token);
        }
        if let Some(context_menu_registration) = context_menu_registration {
            if let Err(error) = context_menu_registration {
                logging::error(format!(
                    "Registering Windows GTK WebView2 context menu handler failed: {error}"
                ));
            } else {
                state.context_menu_token = Some(context_menu_token);
            }
        }
    }

    fn handle_gtk_webview_new_window_request(
        args: &ICoreWebView2NewWindowRequestedEventArgs,
    ) -> windows::core::Result<()> {
        let mut requested_uri = PWSTR::null();
        unsafe {
            args.Uri(&mut requested_uri)?;
        }
        let requested_uri = take_pwstr(requested_uri);

        let mut is_user_initiated = windows::Win32::Foundation::BOOL::default();
        unsafe {
            args.IsUserInitiated(&mut is_user_initiated)?;
            args.SetHandled(true)?;
        }

        if !is_user_initiated.as_bool() || requested_uri.trim().is_empty() {
            return Ok(());
        }

        if let Err(error) =
            gio::AppInfo::launch_default_for_uri(&requested_uri, None::<&gio::AppLaunchContext>)
        {
            logging::error(format!(
                "Windows GTK WebView2 popup open failed for '{requested_uri}': {error}"
            ));
        }

        Ok(())
    }

    fn bind_gtk_webview_status(
        webview: &ICoreWebView2,
        state: Rc<RefCell<WebRuntimeState>>,
        runtime_status: String,
        title: gtk::Label,
        detail_label: gtk::Label,
    ) {
        let mut title_token = EventRegistrationToken::default();
        let title_state = state.clone();
        let title_status = runtime_status.clone();
        let title_label = title.clone();
        let title_detail = detail_label.clone();
        let title_registration = unsafe {
            webview.add_DocumentTitleChanged(
                &DocumentTitleChangedEventHandler::create(Box::new(move |webview, _| {
                    let Some(webview) = webview else {
                        return Ok(());
                    };
                    let mut web_title = PWSTR::null();
                    webview.DocumentTitle(&mut web_title)?;
                    let web_title = take_pwstr(web_title);
                    if !web_title.trim().is_empty() {
                        title_label.set_text(&web_title);
                    }
                    let state = title_state.borrow();
                    title_detail.set_text(&web_runtime_detail(
                        &title_status,
                        &state.current_url,
                        state.auto_refresh_seconds,
                    ));
                    Ok(())
                })),
                &mut title_token,
            )
        };

        let mut nav_token = EventRegistrationToken::default();
        let nav_state = state.clone();
        let nav_status = runtime_status.clone();
        let nav_title = title;
        let nav_detail = detail_label;
        let nav_registration = unsafe {
            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |webview, _| {
                    let Some(webview) = webview else {
                        return Ok(());
                    };
                    let mut source = PWSTR::null();
                    webview.Source(&mut source)?;
                    let source = normalize_web_url(&take_pwstr(source));
                    let mut state = nav_state.borrow_mut();
                    state.current_url = source.clone();
                    nav_title.set_tooltip_text(Some(&source));
                    nav_detail.set_text(&web_runtime_detail(
                        &nav_status,
                        &source,
                        state.auto_refresh_seconds,
                    ));
                    Ok(())
                })),
                &mut nav_token,
            )
        };

        let mut state = state.borrow_mut();
        if let Err(error) = title_registration {
            logging::error(format!(
                "Registering Windows GTK WebView2 title handler failed: {error}"
            ));
        } else {
            state.document_title_token = Some(title_token);
        }
        if let Err(error) = nav_registration {
            logging::error(format!(
                "Registering Windows GTK WebView2 navigation handler failed: {error}"
            ));
        } else {
            state.navigation_completed_token = Some(nav_token);
        }
    }

    fn sync_gtk_webview_bounds(host: &gtk::Widget, state: &Rc<RefCell<WebRuntimeState>>) {
        let Some(bounds) = gtk_widget_root_bounds(host) else {
            return;
        };
        let controller = {
            let mut state = state.borrow_mut();
            if state.last_bounds == Some(bounds) {
                return;
            }
            state.last_bounds = Some(bounds);
            state.controller.clone()
        };
        if let Some(controller) = controller {
            let _ = unsafe { controller.SetBounds(bounds) };
        }
    }

    fn tick_gtk_webview_refresh(state: &Rc<RefCell<WebRuntimeState>>) {
        let webview = {
            let mut state = state.borrow_mut();
            let Some(interval_seconds) = state.auto_refresh_seconds.filter(|seconds| *seconds > 0)
            else {
                return;
            };
            let interval_ticks =
                ((u64::from(interval_seconds) * 1_000) / WEBVIEW_RUNTIME_POLL_MS).max(1) as u32;
            state.refresh_tick = state.refresh_tick.saturating_add(1);
            if state.refresh_tick < interval_ticks {
                return;
            }
            state.refresh_tick = 0;
            state.webview.clone()
        };
        if let Some(webview) = webview
            && let Err(error) = unsafe { webview.Reload() }
        {
            logging::error(format!("Windows GTK WebView2 refresh failed: {error}"));
        }
    }

    fn shutdown_web_runtime(state: &Rc<RefCell<WebRuntimeState>>, reason: &str) {
        let (
            webview,
            title_token,
            navigation_token,
            new_window_token,
            context_menu_token,
            controller,
        ) = {
            let mut state = state.borrow_mut();
            if state.shutdown {
                return;
            }
            logging::info(format!(
                "closing Windows GTK WebView2 runtime reason='{reason}'"
            ));
            state.shutdown = true;
            let webview = state.webview.take();
            let title_token = state.document_title_token.take();
            let navigation_token = state.navigation_completed_token.take();
            let new_window_token = state.new_window_token.take();
            let context_menu_token = state.context_menu_token.take();
            let controller = state.controller.take();
            state.environment = None;
            (
                webview,
                title_token,
                navigation_token,
                new_window_token,
                context_menu_token,
                controller,
            )
        };
        if let Some(webview) = webview {
            if let Some(token) = title_token {
                let _ = unsafe { webview.remove_DocumentTitleChanged(token) };
            }
            if let Some(token) = navigation_token {
                let _ = unsafe { webview.remove_NavigationCompleted(token) };
            }
            if let Some(token) = new_window_token {
                let _ = unsafe { webview.remove_NewWindowRequested(token) };
            }
            if let Some(token) = context_menu_token
                && let Ok(webview11) = webview.cast::<ICoreWebView2_11>()
            {
                let _ = unsafe { webview11.remove_ContextMenuRequested(token) };
            }
        }
        if let Some(controller) = controller
            && let Err(error) = unsafe { controller.Close() }
        {
            logging::error(format!("Windows GTK WebView2 close failed: {error}"));
        }
    }

    fn gtk_widget_hwnd(widget: &gtk::Widget) -> Option<windows_sys::Win32::Foundation::HWND> {
        let native = widget.native()?;
        let surface = native.surface()?;
        let handle = unsafe { gdk_win32_surface_get_handle(surface.to_glib_none().0) };
        (!handle.is_null()).then_some(handle as windows_sys::Win32::Foundation::HWND)
    }

    fn gtk_widget_root_bounds(widget: &gtk::Widget) -> Option<WinRect> {
        let root = widget.root()?.upcast::<gtk::Widget>();
        let bounds = widget.compute_bounds(&root)?;
        let left = bounds.x().round().max(0.0) as i32;
        let top = bounds.y().round().max(0.0) as i32;
        let width = bounds.width().round().max(1.0) as i32;
        let height = bounds.height().round().max(1.0) as i32;
        Some(WinRect {
            left,
            top,
            right: left.saturating_add(width),
            bottom: top.saturating_add(height),
        })
    }

    fn ensure_webview_com_initialized() -> Result<(), String> {
        static WEBVIEW_COM_INIT: OnceLock<Result<(), String>> = OnceLock::new();

        WEBVIEW_COM_INIT
            .get_or_init(|| unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|error| format!("CoInitializeEx failed for WebView2: {error}"))
            })
            .clone()
    }

    fn create_webview_environment() -> Result<ICoreWebView2Environment, String> {
        ensure_webview_com_initialized()?;

        let (tx, rx) = mpsc::channel();
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR::null(),
                None::<
                    &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2EnvironmentOptions,
                >,
                &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
                    move |error_code, environment| {
                        error_code?;
                        tx.send(environment.ok_or_else(|| WindowsError::from(E_POINTER)))
                            .map_err(|_| WindowsError::from(E_UNEXPECTED))
                    },
                )),
            )
            .map_err(|error| format!("CreateCoreWebView2EnvironmentWithOptions failed: {error}"))?;
        }

        wait_with_pump(rx)
            .map_err(|error| format!("Waiting for WebView2 environment failed: {error}"))?
            .map_err(|error| format!("Creating WebView2 environment failed: {error}"))
    }

    fn create_webview_controller(
        parent_hwnd: windows_sys::Win32::Foundation::HWND,
        environment: &ICoreWebView2Environment,
    ) -> Result<ICoreWebView2Controller, String> {
        let (tx, rx) = mpsc::channel();
        let handler: ICoreWebView2CreateCoreWebView2ControllerCompletedHandler =
            CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |error_code, controller| {
                    error_code?;
                    tx.send(controller.ok_or_else(|| WindowsError::from(E_POINTER)))
                        .map_err(|_| WindowsError::from(E_UNEXPECTED))
                },
            ));

        unsafe {
            environment
                .CreateCoreWebView2Controller(Win32Hwnd(parent_hwnd as _), &handler)
                .map_err(|error| format!("CreateCoreWebView2Controller failed: {error}"))?;
        }

        wait_with_pump(rx)
            .map_err(|error| format!("Waiting for WebView2 controller failed: {error}"))?
            .map_err(|error| format!("Creating WebView2 controller failed: {error}"))
    }

    fn web_runtime_detail(
        runtime_status: &str,
        url: &str,
        auto_refresh_seconds: Option<u32>,
    ) -> String {
        match auto_refresh_seconds.filter(|seconds| *seconds > 0) {
            Some(seconds) => format!("{runtime_status}: {url} • refresh {seconds}s"),
            None => format!("{runtime_status}: {url}"),
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub(crate) use imp::build_tile_runtime_surface;
