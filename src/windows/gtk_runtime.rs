#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
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
        ICoreWebView2Controller, ICoreWebView2Environment,
        ICoreWebView2NewWindowRequestedEventArgs,
    };
    use webview2_com::{
        ContextMenuRequestedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler, DocumentTitleChangedEventHandler,
        NavigationCompletedEventHandler, NewWindowRequestedEventHandler, take_pwstr,
    };
    use windows::Win32::Foundation::{E_POINTER, HWND as Win32Hwnd, RECT as WinRect};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::System::WinRT::EventRegistrationToken;
    use windows::core::{Error as WindowsError, HSTRING, Interface, PCWSTR, PWSTR};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    use windows_sys::Win32::System::Threading::TerminateProcess;

    use crate::app_paths;
    use crate::dropped_paths::{self, DroppedPathTarget};
    use crate::logging;
    use crate::model::assets::{OutputSeverity, PaneStatusSnapshot, WorkspaceAssets};
    use crate::model::layout::{DEFAULT_WEB_URL, TileKind, TileSpec, normalize_web_url};
    use crate::model::preset::ApplicationDensity;
    use crate::services::launch_resolution::resolve_tile_launch;
    use crate::services::output_helpers::{CompiledOutputHelpers, helper_summary_text};
    use crate::storage::session_store::SavedTab;
    use crate::terminal_palette::{TerminalPalette, terminal_palette};
    use crate::transcript::TranscriptBuffer;
    use crate::ui::appearance::resolved_theme_uses_dark_palette;
    use crate::ui::context_menu;
    use crate::ui::icons::{self, name as icon_name};
    use crate::ui::terminal_context_menu::{self, TerminalContextMenuInput};
    use crate::ui::terminal_recovery_popover;
    use crate::ui::tile_chrome::{domain_from_url, make_shrinkable};
    use crate::ui::transcript_dialog;
    use crate::ui::web_context_menu::{self, WebContextMenuInput};
    use crate::ui::workspace_preview::{TileRuntimeRecoveryBinder, TileRuntimeSurface};
    use crate::windows::vt::{VtBuffer, VtColor, VtStyle};
    use crate::windows::{workspace, wsl};

    const MIN_TERMINAL_FONT_POINTS: i32 = 7;
    const MAX_TERMINAL_FONT_POINTS: i32 = 20;
    const DEFAULT_TERMINAL_COPY_SHORTCUT: &str = "<Ctrl><Shift>C";
    const DEFAULT_TERMINAL_PASTE_SHORTCUT: &str = "<Ctrl><Shift>V";
    const TERMINAL_RUNTIME_COLUMNS: usize = 80;
    const TERMINAL_RUNTIME_ROWS: usize = 24;
    const TERMINAL_RUNTIME_POLL_MS: u64 = 80;
    const WEBVIEW_RUNTIME_POLL_MS: u64 = 100;
    const WEBVIEW_PARENT_HWND_LOG_POLLS: u32 = 10;
    const WEBVIEW_ENVIRONMENT_CALLBACK_TIMEOUT_SECONDS: u64 = 45;
    const WEBVIEW_CONTROLLER_CALLBACK_TIMEOUT_SECONDS: u64 = 45;

    unsafe extern "C" {
        fn gdk_win32_surface_get_handle(surface: *mut gdk::ffi::GdkSurface) -> *mut c_void;
    }

    #[derive(Default)]
    struct TerminalRuntimeState {
        stdin_tx: Option<mpsc::Sender<String>>,
        active: bool,
        process_handle: Option<isize>,
        launch_runtime: Option<wsl::WindowsLaunchRuntime>,
        transcript: TranscriptBuffer,
        next_generation: u64,
        active_generation: u64,
    }

    enum TerminalRuntimeEvent {
        Output(String),
        ProcessStarted {
            generation: u64,
            process_handle: isize,
            runtime: wsl::WindowsLaunchRuntime,
        },
        ProcessEnded {
            generation: u64,
        },
    }

    #[derive(Clone, Copy)]
    enum TerminalLaunchMode {
        ConfiguredSession,
        LocalShell,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct TerminalTextStyleKey {
        use_dark_palette: bool,
        fg: VtColor,
        bg: VtColor,
        bold: bool,
        underline: bool,
        inverse: bool,
        hyperlink: bool,
    }

    impl TerminalTextStyleKey {
        fn from_style(style: VtStyle, use_dark_palette: bool) -> Self {
            Self {
                use_dark_palette,
                fg: style.fg,
                bg: style.bg,
                bold: style.bold,
                underline: style.underline,
                inverse: style.inverse,
                hyperlink: style.hyperlink_id.is_some(),
            }
        }

        fn is_plain(self) -> bool {
            self.fg == VtColor::DefaultForeground
                && self.bg == VtColor::DefaultBackground
                && !self.bold
                && !self.underline
                && !self.inverse
                && !self.hyperlink
        }
    }

    #[derive(Clone, Copy)]
    struct TerminalTextRange {
        start: i32,
        end: i32,
        style: TerminalTextStyleKey,
    }

    #[derive(Clone)]
    struct TerminalRuntimeChromeContext {
        tile: TileSpec,
        workspace_root: PathBuf,
        assets: WorkspaceAssets,
        output_helpers: CompiledOutputHelpers,
        terminal_buffer: Rc<RefCell<VtBuffer>>,
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
        hwnd_poll_count: u32,
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
        let terminal_style_tags = Rc::new(RefCell::new(HashMap::new()));
        let output_helpers = CompiledOutputHelpers::new(&tile.output_helpers);
        let chrome_context = TerminalRuntimeChromeContext {
            tile: tile.clone(),
            workspace_root: tab.workspace_root.clone(),
            assets: assets.clone(),
            output_helpers,
            terminal_buffer: terminal_buffer.clone(),
        };
        let use_dark_palette = Rc::new(Cell::new(resolved_theme_uses_dark_palette(
            tab.preset.theme,
        )));
        {
            let mut terminal_buffer = terminal_buffer.borrow_mut();
            terminal_buffer.process(&format!(
                "[terminaltiler] starting {} in {}\r\n",
                tile.title,
                tab.workspace_root.display()
            ));
            render_terminal_runtime_buffer(
                &buffer,
                &terminal_buffer,
                &mut terminal_style_tags.borrow_mut(),
                use_dark_palette.get(),
            );
        }

        let terminal_output = gtk::TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .focusable(true)
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
            use_dark_palette.get(),
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

        let state = Rc::new(RefCell::new(TerminalRuntimeState::default()));
        let recovery_bind_generation = Rc::new(Cell::new(0u64));
        let (event_tx, event_rx) = mpsc::channel::<TerminalRuntimeEvent>();
        start_terminal_process(
            &state,
            tile.clone(),
            tab.clone(),
            assets.clone(),
            TerminalLaunchMode::ConfiguredSession,
            event_tx.clone(),
        );

        {
            let buffer = buffer.clone();
            let terminal_buffer = terminal_buffer.clone();
            let terminal_style_tags = terminal_style_tags.clone();
            let use_dark_palette = use_dark_palette.clone();
            let state = state.clone();
            let terminal_output = terminal_output.clone();
            gtk::glib::timeout_add_local(
                Duration::from_millis(TERMINAL_RUNTIME_POLL_MS),
                move || {
                    while let Ok(event) = event_rx.try_recv() {
                        match event {
                            TerminalRuntimeEvent::Output(chunk) => {
                                state.borrow_mut().transcript.push_output(&chunk);
                                let mut terminal_buffer = terminal_buffer.borrow_mut();
                                terminal_buffer.process(&chunk);
                                flush_terminal_runtime_responses(
                                    &mut terminal_buffer,
                                    &state,
                                    &terminal_output,
                                );
                                render_terminal_runtime_buffer(
                                    &buffer,
                                    &terminal_buffer,
                                    &mut terminal_style_tags.borrow_mut(),
                                    use_dark_palette.get(),
                                );
                            }
                            TerminalRuntimeEvent::ProcessStarted {
                                generation,
                                process_handle,
                                runtime,
                            } => {
                                let terminate_stale_process = {
                                    let mut state = state.borrow_mut();
                                    if state.active_generation == generation && state.active {
                                        state.process_handle = Some(process_handle);
                                        state.launch_runtime = Some(runtime);
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
                                    state.launch_runtime = None;
                                    state.process_handle = None;
                                }
                            }
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                },
            );
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
                    TerminalLaunchMode::ConfiguredSession,
                    event_tx.clone(),
                );
            }
        });
        let open_local_shell = Rc::new({
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
                    TerminalLaunchMode::LocalShell,
                    event_tx.clone(),
                );
            }
        });
        install_terminal_output_context_menu(
            &terminal_output,
            &state,
            restart_runtime.clone(),
            open_local_shell.clone(),
        );
        install_terminal_output_shortcuts(&terminal_output, &state);
        install_terminal_input_key_controller(&terminal_output, &state, terminal_buffer.clone());

        let command_sender = Rc::new({
            let state = state.clone();
            move |command: &str| send_terminal_runtime_payload(&state, command.to_string())
        });
        let dropped_paths_sender = Rc::new({
            let state = state.clone();
            move |paths: &[PathBuf], show_recovery_prompt: Option<&dyn Fn()>| {
                paste_dropped_paths_into_terminal_runtime(&state, paths, show_recovery_prompt)
            }
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
            let buffer = buffer.clone();
            let terminal_buffer = terminal_buffer.clone();
            let terminal_style_tags = terminal_style_tags.clone();
            let use_dark_palette_cell = use_dark_palette.clone();
            move |next_use_dark_palette, density, zoom_steps| {
                use_dark_palette_cell.set(next_use_dark_palette);
                apply_terminal_runtime_appearance(
                    &appearance_provider,
                    next_use_dark_palette,
                    density,
                    zoom_steps,
                );
                render_terminal_runtime_buffer(
                    &buffer,
                    &terminal_buffer.borrow(),
                    &mut terminal_style_tags.borrow_mut(),
                    next_use_dark_palette,
                );
            }
        });

        TileRuntimeSurface {
            widget: surface.upcast(),
            command_sender: Some(command_sender),
            dropped_paths_sender: Some(dropped_paths_sender),
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
                    let chrome_context = chrome_context.clone();
                    move |shell, status, recovery_button, title_label| {
                        bind_terminal_recovery_controls(
                            shell,
                            status,
                            recovery_button,
                            title_label,
                            state.clone(),
                            restart_runtime.clone(),
                            open_local_shell.clone(),
                            recovery_bind_generation.clone(),
                            chrome_context.clone(),
                        )
                    }
                }),
            }),
        }
    }

    fn apply_terminal_runtime_appearance(
        provider: &gtk::CssProvider,
        use_dark_palette: bool,
        density: ApplicationDensity,
        zoom_steps: i32,
    ) {
        let palette = terminal_palette(use_dark_palette);
        provider.load_from_data(&format!(
            ".terminal-runtime-output {{ font-family: \"JetBrains Mono\", monospace; font-size: {}pt; color: {}; background: {}; }}",
            effective_terminal_font_points(density, zoom_steps),
            palette.foreground,
            palette.background
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
        mode: TerminalLaunchMode,
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
            state.launch_runtime = None;
            state.active_generation
        };
        if generation > 1 {
            let notice = match mode {
                TerminalLaunchMode::ConfiguredSession => {
                    "\r\n[terminaltiler] reconnecting terminal session\r\n"
                }
                TerminalLaunchMode::LocalShell => "\r\n[terminaltiler] opening local shell\r\n",
            };
            let _ = event_tx.send(TerminalRuntimeEvent::Output(notice.into()));
        }
        spawn_terminal_process(tile, tab, assets, mode, generation, stdin_rx, event_tx);
    }

    fn spawn_terminal_process(
        tile: TileSpec,
        tab: SavedTab,
        assets: WorkspaceAssets,
        mode: TerminalLaunchMode,
        generation: u64,
        stdin_rx: mpsc::Receiver<String>,
        event_tx: mpsc::Sender<TerminalRuntimeEvent>,
    ) {
        std::thread::spawn(move || {
            let launch = wsl::probe_runtime(None).and_then(|runtime| match mode {
                TerminalLaunchMode::ConfiguredSession => {
                    resolve_tile_launch(&tile, &tab.workspace_root, &assets).and_then(|resolved| {
                        wsl::build_launch_command(&tile, &tab.workspace_root, &resolved, &runtime)
                    })
                }
                TerminalLaunchMode::LocalShell => {
                    wsl::build_local_shell_command(&tile, &tab.workspace_root, &runtime)
                }
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
                runtime: command.runtime.clone(),
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

    fn render_terminal_runtime_buffer(
        buffer: &gtk::TextBuffer,
        terminal: &VtBuffer,
        style_tags: &mut HashMap<TerminalTextStyleKey, gtk::TextTag>,
        use_dark_palette: bool,
    ) {
        let total_rows = terminal.total_rows();
        let mut rendered = String::with_capacity((terminal.columns() + 1) * total_rows);
        let mut style_ranges = Vec::new();
        let mut char_offset = 0i32;
        let cursor = terminal.cursor_visible().then(|| {
            let (column, row) = terminal.cursor();
            (column, terminal.history_len() + row)
        });

        for row in 0..total_rows {
            if row > 0 {
                rendered.push('\n');
                char_offset += 1;
            }
            for column in 0..terminal.columns() {
                let mut cell = terminal.display_cell(row, column);
                let mut ch = cell.ch;
                if cursor == Some((column, row)) {
                    ch = if ch == ' ' { '█' } else { '▌' };
                    cell.style.inverse = !cell.style.inverse;
                }

                let start = char_offset;
                rendered.push(ch);
                char_offset += 1;
                let style = TerminalTextStyleKey::from_style(cell.style, use_dark_palette);
                if !style.is_plain() {
                    push_terminal_text_range(&mut style_ranges, start, char_offset, style);
                }
            }
        }

        buffer.set_text(&rendered);
        apply_terminal_text_ranges(buffer, style_tags, &style_ranges, use_dark_palette);
    }

    fn push_terminal_text_range(
        ranges: &mut Vec<TerminalTextRange>,
        start: i32,
        end: i32,
        style: TerminalTextStyleKey,
    ) {
        if let Some(last) = ranges.last_mut()
            && last.end == start
            && last.style == style
        {
            last.end = end;
            return;
        }
        ranges.push(TerminalTextRange { start, end, style });
    }

    fn apply_terminal_text_ranges(
        buffer: &gtk::TextBuffer,
        style_tags: &mut HashMap<TerminalTextStyleKey, gtk::TextTag>,
        ranges: &[TerminalTextRange],
        use_dark_palette: bool,
    ) {
        if ranges.is_empty() {
            return;
        }

        let palette = terminal_palette(use_dark_palette);
        for range in ranges {
            let tag = terminal_text_tag(buffer, style_tags, range.style, palette);
            let start = buffer.iter_at_offset(range.start);
            let end = buffer.iter_at_offset(range.end);
            buffer.apply_tag(&tag, &start, &end);
        }
    }

    fn terminal_text_tag(
        buffer: &gtk::TextBuffer,
        style_tags: &mut HashMap<TerminalTextStyleKey, gtk::TextTag>,
        style: TerminalTextStyleKey,
        palette: TerminalPalette,
    ) -> gtk::TextTag {
        if let Some(tag) = style_tags.get(&style) {
            return tag.clone();
        }

        let mut fg = style.fg;
        let mut bg = style.bg;
        if style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }

        let mut builder = gtk::TextTag::builder();
        if style.bold {
            builder = builder.weight(700).weight_set(true);
        }
        if style.underline || style.hyperlink {
            builder = builder
                .underline(gtk::pango::Underline::Single)
                .underline_set(true);
        }
        if fg != VtColor::DefaultForeground || style.inverse {
            let color = terminal_color_rgba(fg, palette);
            builder = builder.foreground_rgba(&color).foreground_set(true);
        }
        if bg != VtColor::DefaultBackground || style.inverse {
            let color = terminal_color_rgba(bg, palette);
            builder = builder.background_rgba(&color).background_set(true);
        }

        let tag = builder.build();
        buffer.tag_table().add(&tag);
        style_tags.insert(style, tag.clone());
        tag
    }

    fn terminal_color_rgba(color: VtColor, palette: TerminalPalette) -> gdk::RGBA {
        match color {
            VtColor::DefaultForeground => terminal_rgba(palette.foreground),
            VtColor::DefaultBackground => terminal_rgba(palette.background),
            VtColor::Indexed(index) => terminal_rgba(palette.palette[index.min(15) as usize]),
            VtColor::Rgb(red, green, blue) => gdk::RGBA::new(
                f32::from(red) / 255.0,
                f32::from(green) / 255.0,
                f32::from(blue) / 255.0,
                1.0,
            ),
        }
    }

    fn terminal_rgba(value: &str) -> gdk::RGBA {
        gdk::RGBA::parse(value).expect("terminal palette color should parse")
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

    fn install_terminal_input_key_controller(
        output: &gtk::TextView,
        state: &Rc<RefCell<TerminalRuntimeState>>,
        terminal_buffer: Rc<RefCell<VtBuffer>>,
    ) {
        let key_controller = gtk::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let state = state.clone();
            key_controller.connect_key_pressed(move |_, key, _, key_state| {
                let terminal_buffer = terminal_buffer.borrow();
                let Some(payload) = terminal_runtime_key_payload(key, key_state, &terminal_buffer)
                else {
                    return gtk::glib::Propagation::Proceed;
                };
                let manual_typing = terminal_runtime_manual_typing_text(key, key_state);
                if send_terminal_runtime_payload(&state, payload) {
                    if let Some(manual_typing) = manual_typing {
                        crate::stats_hub::recorder().record_manual_typing(&manual_typing);
                    }
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            });
        }
        output.add_controller(key_controller);

        let focus_click = gtk::GestureClick::builder().button(1).build();
        {
            let output = output.clone();
            focus_click.connect_pressed(move |_, _, _, _| {
                output.grab_focus();
            });
        }
        output.add_controller(focus_click);
    }

    fn terminal_runtime_key_payload(
        key: gtk::gdk::Key,
        state: gtk::gdk::ModifierType,
        terminal: &VtBuffer,
    ) -> Option<String> {
        let modifiers = state & gtk::accelerator_get_default_mod_mask();
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let alt = modifiers.contains(gtk::gdk::ModifierType::ALT_MASK)
            || modifiers.contains(gtk::gdk::ModifierType::META_MASK);

        if ctrl
            && shift
            && key.to_unicode().is_some_and(|value| {
                value.eq_ignore_ascii_case(&'c') || value.eq_ignore_ascii_case(&'v')
            })
        {
            return None;
        }

        let special = match key {
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => Some("\r"),
            gtk::gdk::Key::BackSpace => Some("\u{7f}"),
            gtk::gdk::Key::Tab | gtk::gdk::Key::ISO_Left_Tab => Some("\t"),
            gtk::gdk::Key::Escape => Some("\u{1b}"),
            gtk::gdk::Key::Left => Some(terminal_key_sequence(terminal, "\u{1b}[D", "\u{1b}OD")),
            gtk::gdk::Key::Right => Some(terminal_key_sequence(terminal, "\u{1b}[C", "\u{1b}OC")),
            gtk::gdk::Key::Up => Some(terminal_key_sequence(terminal, "\u{1b}[A", "\u{1b}OA")),
            gtk::gdk::Key::Down => Some(terminal_key_sequence(terminal, "\u{1b}[B", "\u{1b}OB")),
            gtk::gdk::Key::Home => Some(terminal_key_sequence(terminal, "\u{1b}[H", "\u{1b}OH")),
            gtk::gdk::Key::End => Some(terminal_key_sequence(terminal, "\u{1b}[F", "\u{1b}OF")),
            gtk::gdk::Key::Insert => Some("\u{1b}[2~"),
            gtk::gdk::Key::Delete => Some("\u{1b}[3~"),
            gtk::gdk::Key::Page_Up => Some("\u{1b}[5~"),
            gtk::gdk::Key::Page_Down => Some("\u{1b}[6~"),
            _ => None,
        };
        if let Some(sequence) = special {
            return Some(sequence.to_string());
        }

        let character = key.to_unicode()?;
        if ctrl {
            return control_character_payload(character);
        }

        if alt && !character.is_control() {
            return Some(format!("\u{1b}{character}"));
        }

        if modifiers == gtk::gdk::ModifierType::empty()
            || modifiers == gtk::gdk::ModifierType::SHIFT_MASK
        {
            return (!character.is_control()).then(|| character.to_string());
        }

        None
    }

    fn terminal_runtime_manual_typing_text(
        key: gtk::gdk::Key,
        state: gtk::gdk::ModifierType,
    ) -> Option<String> {
        let modifiers = state & gtk::accelerator_get_default_mod_mask();
        if !(modifiers.is_empty() || modifiers == gtk::gdk::ModifierType::SHIFT_MASK) {
            return None;
        }

        let character = key.to_unicode()?;
        (!character.is_control()).then(|| character.to_string())
    }

    fn control_character_payload(character: char) -> Option<String> {
        let upper = character.to_ascii_uppercase();
        let byte = match upper {
            '@' | ' ' => 0x00,
            'A'..='Z' => upper as u8 - b'A' + 1,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            '^' => 0x1e,
            '_' => 0x1f,
            '?' => 0x7f,
            _ => return None,
        };
        Some(char::from(byte).to_string())
    }

    fn terminal_key_sequence<'a>(
        terminal: &VtBuffer,
        normal_sequence: &'a str,
        application_sequence: &'a str,
    ) -> &'a str {
        if terminal.application_cursor_keys() {
            application_sequence
        } else {
            normal_sequence
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

        if let Some(stdin_tx) = stdin_tx {
            if stdin_tx.send(payload.clone()).is_ok() {
                state.borrow_mut().transcript.push_input(&payload);
                return true;
            }

            state.borrow_mut().stdin_tx = None;
        }

        false
    }

    fn paste_dropped_paths_into_terminal_runtime(
        state: &Rc<RefCell<TerminalRuntimeState>>,
        paths: &[PathBuf],
        show_recovery_prompt: Option<&dyn Fn()>,
    ) -> bool {
        if paths.is_empty() {
            return false;
        }

        let launch_runtime = {
            let state = state.borrow();
            if !state.active {
                if let Some(show_recovery_prompt) = show_recovery_prompt {
                    show_recovery_prompt();
                    return true;
                }
                return false;
            }
            state.launch_runtime.clone()
        };
        let Some(launch_runtime) = launch_runtime else {
            return false;
        };

        let target = match launch_runtime {
            wsl::WindowsLaunchRuntime::Wsl { ref distro } => DroppedPathTarget::Wsl { distro },
            wsl::WindowsLaunchRuntime::PowerShell { .. } => DroppedPathTarget::PowerShell,
            wsl::WindowsLaunchRuntime::Ssh { .. } => DroppedPathTarget::Posix,
        };
        let path_text = paths
            .iter()
            .map(|path| path.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let (payload, errors) =
            dropped_paths::serialize_for_target(path_text.iter().map(String::as_str), target);
        for error in errors {
            logging::error(format!("skipped Windows GTK dropped path: {error}"));
        }

        payload.is_some_and(|payload| send_terminal_runtime_payload(state, payload))
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
            state.launch_runtime = None;
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
        state: &Rc<RefCell<TerminalRuntimeState>>,
        restart_runtime: Rc<dyn Fn()>,
        open_local_shell: Rc<dyn Fn()>,
    ) {
        let context_menu = terminal_context_menu::install(
            output,
            TerminalContextMenuInput {
                grab_focus: Rc::new({
                    let output = output.clone();
                    move || {
                        output.grab_focus();
                    }
                }),
                has_selection: Rc::new({
                    let output = output.clone();
                    move || output.buffer().has_selection()
                }),
                can_paste: Rc::new({
                    let state = state.clone();
                    move || state.borrow().active
                }),
                can_reconnect: Rc::new({
                    let state = state.clone();
                    move || !state.borrow().active
                }),
                can_open_local_shell: Rc::new({
                    let state = state.clone();
                    move || !state.borrow().active
                }),
                copy: Rc::new({
                    let output = output.clone();
                    move || {
                        copy_terminal_output_selection(&output);
                    }
                }),
                paste: Rc::new({
                    let output = output.clone();
                    let state = state.clone();
                    move || {
                        paste_clipboard_into_terminal_runtime(&output, &state);
                    }
                }),
                reconnect: restart_runtime,
                open_local_shell,
                show_transcript: Rc::new({
                    let output = output.clone();
                    let state = state.clone();
                    move || {
                        let transcript = state.borrow().transcript.recent_transcript(240);
                        transcript_dialog::present(&output, &transcript);
                    }
                }),
                focus_command_input: None,
            },
        );
        {
            let copy_button = context_menu.copy_button.clone();
            output.buffer().connect_has_selection_notify(move |buffer| {
                copy_button.set_sensitive(buffer.has_selection());
            });
        }
    }

    fn install_terminal_output_shortcuts(
        output: &gtk::TextView,
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
        title_label: &gtk::Label,
        state: Rc<RefCell<TerminalRuntimeState>>,
        restart_runtime: Rc<dyn Fn()>,
        open_local_shell: Rc<dyn Fn()>,
        bind_generation: Rc<Cell<u64>>,
        chrome_context: TerminalRuntimeChromeContext,
    ) -> Option<Rc<dyn Fn()>> {
        let current_generation = bind_generation.get().saturating_add(1);
        bind_generation.set(current_generation);

        let default_title = title_label.text().to_string();
        let popover =
            terminal_recovery_popover::build(recovery_button, restart_runtime, open_local_shell);

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
            title_label,
            &state,
            &chrome_context,
            &default_title,
        );

        let shell_weak = shell.downgrade();
        let status_weak = status.downgrade();
        let recovery_button_weak = recovery_button.downgrade();
        let title_label_weak = title_label.downgrade();
        gtk::glib::timeout_add_local(Duration::from_millis(TERMINAL_RUNTIME_POLL_MS), move || {
            if bind_generation.get() != current_generation {
                return gtk::glib::ControlFlow::Break;
            }
            let (Some(shell), Some(status), Some(recovery_button), Some(title_label)) = (
                shell_weak.upgrade(),
                status_weak.upgrade(),
                recovery_button_weak.upgrade(),
                title_label_weak.upgrade(),
            ) else {
                return gtk::glib::ControlFlow::Break;
            };
            sync_terminal_recovery_state(
                &shell,
                &status,
                &recovery_button,
                &title_label,
                &state,
                &chrome_context,
                &default_title,
            );
            gtk::glib::ControlFlow::Continue
        });
        Some(Rc::new(move || {
            popover.popup();
        }))
    }

    fn sync_terminal_recovery_state(
        shell: &gtk::Box,
        status: &gtk::Label,
        recovery_button: &gtk::Button,
        title_label: &gtk::Label,
        state: &Rc<RefCell<TerminalRuntimeState>>,
        chrome_context: &TerminalRuntimeChromeContext,
        default_title: &str,
    ) {
        if state.borrow().active {
            shell.remove_css_class("is-disconnected");
            status.remove_css_class("recovery-chip");
            let terminal = chrome_context.terminal_buffer.borrow();
            sync_terminal_runtime_title(title_label, &terminal, default_title);
            let snapshot = status_snapshot_for_terminal_runtime(&terminal, chrome_context);
            let status_line = snapshot.to_line();
            status.set_text(&status_line);
            status.set_tooltip_text(Some(&status_line));
            sync_status_severity(status, snapshot.helper_severity);
            recovery_button.set_visible(false);
            recovery_button.set_sensitive(false);
        } else {
            shell.add_css_class("is-disconnected");
            status.add_css_class("recovery-chip");
            sync_status_severity(status, None);
            status.set_text("Disconnected  •  Reconnect or open local shell");
            status.set_tooltip_text(Some(
                "This Windows GTK terminal process exited. Reconnect the configured session or open a local shell.",
            ));
            recovery_button.set_visible(true);
            recovery_button.set_sensitive(true);
        }
    }

    fn sync_terminal_runtime_title(
        title_label: &gtk::Label,
        terminal: &VtBuffer,
        default_title: &str,
    ) {
        let title = terminal.window_title();
        let title = title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(default_title);
        title_label.set_text(title);
        title_label.set_tooltip_text(Some(title));
    }

    fn status_snapshot_for_terminal_runtime(
        terminal: &VtBuffer,
        context: &TerminalRuntimeChromeContext,
    ) -> PaneStatusSnapshot {
        let mut snapshot = crate::ui::pane_status::initial_status_snapshot(
            &context.tile,
            &context.workspace_root,
            &context.assets,
        );
        if let Some(cwd) = terminal.current_working_directory() {
            snapshot.location_label = short_location_from_path(cwd);
        } else if let Some(title) = terminal.window_title() {
            snapshot.location_label = title.to_string();
        }

        let title = terminal.window_title();
        let (matches, shell_label) =
            if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                (context.output_helpers.scan(title), title.to_string())
            } else {
                let recent = terminal_recent_output(terminal, 32);
                let matches = context.output_helpers.scan(&recent);
                let shell_label = if recent.trim().is_empty() {
                    context.tile.agent_label.clone()
                } else {
                    recent
                        .lines()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .map(str::trim)
                        .unwrap_or(&context.tile.agent_label)
                        .to_string()
                };
                (matches, shell_label)
            };
        let (helper_label, helper_severity) = helper_summary_text(&matches);
        snapshot.shell_label = shell_label;
        snapshot.helper_label = helper_label;
        snapshot.helper_severity = helper_severity;
        snapshot
    }

    fn terminal_recent_output(terminal: &VtBuffer, max_rows: usize) -> String {
        let total_rows = terminal.total_rows();
        let start_row = total_rows.saturating_sub(max_rows.max(1));
        let mut output = String::with_capacity((terminal.columns() + 1) * (total_rows - start_row));

        for row in start_row..total_rows {
            let mut line = String::with_capacity(terminal.columns());
            for column in 0..terminal.columns() {
                line.push(terminal.display_cell(row, column).ch);
            }
            if row > start_row {
                output.push('\n');
            }
            output.push_str(line.trim_end());
        }

        output
    }

    fn short_location_from_path(path: &str) -> String {
        PathBuf::from(path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| path.to_string())
    }

    fn sync_status_severity(status: &gtk::Label, severity: Option<OutputSeverity>) {
        status.remove_css_class("helper-info");
        status.remove_css_class("helper-warning");
        status.remove_css_class("helper-error");
        match severity {
            Some(OutputSeverity::Info) => status.add_css_class("helper-info"),
            Some(OutputSeverity::Warning) => status.add_css_class("helper-warning"),
            Some(OutputSeverity::Error) => status.add_css_class("helper-error"),
            None => {}
        }
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
            dropped_paths_sender: None,
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
        web_context_menu::build(
            parent,
            WebContextMenuInput {
                reload: Rc::new({
                    let state = state.clone();
                    move || {
                        if let Some(webview) = state.borrow().webview.clone()
                            && let Err(error) = unsafe { webview.Reload() }
                        {
                            logging::error(format!(
                                "Windows GTK WebView2 context reload failed: {error}"
                            ));
                        }
                    }
                }),
                current_url: Rc::new({
                    let state = state.clone();
                    move || Some(state.borrow().current_url.clone())
                }),
                open_error_context: "Windows GTK WebView2 context",
            },
        )
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
                    let poll_count = state.borrow().hwnd_poll_count;
                    logging::info(format!(
                        "Windows GTK WebView2 runtime surface discovered parent HWND after {poll_count} poll(s)"
                    ));
                    initialize_gtk_webview_runtime(
                        hwnd,
                        &host,
                        &state,
                        &runtime_status,
                        &title,
                        &detail_label,
                        &context_popover,
                    );
                } else {
                    let mut state = state.borrow_mut();
                    state.hwnd_poll_count = state.hwnd_poll_count.saturating_add(1);
                    if state.hwnd_poll_count == WEBVIEW_PARENT_HWND_LOG_POLLS {
                        logging::info(format!(
                            "Windows GTK WebView2 runtime surface still waiting for parent HWND after {WEBVIEW_PARENT_HWND_LOG_POLLS} polls"
                        ));
                    }
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

        if let Err(error) = ensure_webview_com_initialized() {
            report_gtk_webview_initialization_message(
                state,
                detail_label,
                &format!("Windows GTK WebView2 COM init failed: {error}"),
            );
            return;
        }

        let Some(user_data_dir) = app_paths::webview2_user_data_dir() else {
            report_gtk_webview_initialization_failure(
                state,
                detail_label,
                "Windows GTK WebView2 user data folder resolution failed",
                WindowsError::from(E_POINTER),
            );
            return;
        };
        if let Err(error) = std::fs::create_dir_all(&user_data_dir) {
            report_gtk_webview_initialization_io_failure(
                state,
                detail_label,
                "Windows GTK WebView2 user data folder creation failed",
                error,
            );
            return;
        }
        logging::info(format!(
            "Windows GTK WebView2 creating environment with user data folder {}",
            user_data_dir.display()
        ));
        let user_data_folder = HSTRING::from(user_data_dir.to_string_lossy().as_ref());

        let state_for_watchdog = state.clone();
        let detail_label_for_watchdog = detail_label.clone();
        glib::timeout_add_local_once(
            Duration::from_secs(WEBVIEW_ENVIRONMENT_CALLBACK_TIMEOUT_SECONDS),
            move || {
                let state = state_for_watchdog.borrow();
                if !state.shutdown && state.environment.is_none() {
                    drop(state);
                    report_gtk_webview_initialization_timeout(
                        &state_for_watchdog,
                        &detail_label_for_watchdog,
                        "Windows GTK WebView2 environment callback",
                        WEBVIEW_ENVIRONMENT_CALLBACK_TIMEOUT_SECONDS,
                    );
                }
            },
        );

        let host = host.clone();
        let state = state.clone();
        let runtime_status = runtime_status.to_string();
        let title = title.clone();
        let detail_label = detail_label.clone();
        let context_popover = context_popover.clone();
        let state_for_callback = state.clone();
        let detail_label_for_callback = detail_label.clone();
        let environment_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |error_code, environment| {
                if state_for_callback.borrow().shutdown {
                    logging::info(
                        "Windows GTK WebView2 environment callback ignored after shutdown",
                    );
                    return Ok(());
                }

                if let Err(error) = error_code {
                    report_gtk_webview_initialization_failure(
                        &state_for_callback,
                        &detail_label_for_callback,
                        "Windows GTK WebView2 environment failed",
                        error,
                    );
                    return Ok(());
                }

                let Some(environment) = environment else {
                    report_gtk_webview_initialization_failure(
                        &state_for_callback,
                        &detail_label_for_callback,
                        "Windows GTK WebView2 environment failed",
                        WindowsError::from(E_POINTER),
                    );
                    return Ok(());
                };

                logging::info("Windows GTK WebView2 environment created; creating controller");
                create_gtk_webview_controller_async(
                    parent_hwnd,
                    environment,
                    host.clone(),
                    state_for_callback.clone(),
                    runtime_status.clone(),
                    title.clone(),
                    detail_label_for_callback.clone(),
                    context_popover.clone(),
                );
                Ok(())
            },
        ));

        let result = unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(user_data_folder.as_ptr()),
                None::<
                    &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2EnvironmentOptions,
                >,
                &environment_handler,
            )
        };
        if let Err(error) = result {
            report_gtk_webview_initialization_failure(
                &state,
                &detail_label,
                "Windows GTK WebView2 environment creation failed",
                error,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_gtk_webview_controller_async(
        parent_hwnd: windows_sys::Win32::Foundation::HWND,
        environment: ICoreWebView2Environment,
        host: gtk::Widget,
        state: Rc<RefCell<WebRuntimeState>>,
        runtime_status: String,
        title: gtk::Label,
        detail_label: gtk::Label,
        context_popover: gtk::Popover,
    ) {
        {
            let mut state_ref = state.borrow_mut();
            if state_ref.shutdown {
                return;
            }
            state_ref.environment = Some(environment.clone());
        }

        let state_for_watchdog = state.clone();
        let detail_label_for_watchdog = detail_label.clone();
        glib::timeout_add_local_once(
            Duration::from_secs(WEBVIEW_CONTROLLER_CALLBACK_TIMEOUT_SECONDS),
            move || {
                let state = state_for_watchdog.borrow();
                if !state.shutdown && state.controller.is_none() {
                    drop(state);
                    report_gtk_webview_initialization_timeout(
                        &state_for_watchdog,
                        &detail_label_for_watchdog,
                        "Windows GTK WebView2 controller callback",
                        WEBVIEW_CONTROLLER_CALLBACK_TIMEOUT_SECONDS,
                    );
                }
            },
        );

        let environment_for_completion = environment.clone();
        let state_for_callback = state.clone();
        let detail_label_for_callback = detail_label.clone();
        let controller_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |error_code, controller| {
                if state_for_callback.borrow().shutdown {
                    logging::info(
                        "Windows GTK WebView2 controller callback ignored after shutdown",
                    );
                    return Ok(());
                }

                if let Err(error) = error_code {
                    report_gtk_webview_initialization_failure(
                        &state_for_callback,
                        &detail_label_for_callback,
                        "Windows GTK WebView2 controller failed",
                        error,
                    );
                    return Ok(());
                }

                let Some(controller) = controller else {
                    report_gtk_webview_initialization_failure(
                        &state_for_callback,
                        &detail_label_for_callback,
                        "Windows GTK WebView2 controller failed",
                        WindowsError::from(E_POINTER),
                    );
                    return Ok(());
                };

                complete_gtk_webview_initialization(
                    environment_for_completion.clone(),
                    controller,
                    &host,
                    &state_for_callback,
                    &runtime_status,
                    &title,
                    &detail_label_for_callback,
                    &context_popover,
                );
                Ok(())
            },
        ));

        let result = unsafe {
            environment
                .CreateCoreWebView2Controller(Win32Hwnd(parent_hwnd as _), &controller_handler)
        };
        if let Err(error) = result {
            report_gtk_webview_initialization_failure(
                &state,
                &detail_label,
                "Windows GTK WebView2 controller creation failed",
                error,
            );
        }
    }

    fn report_gtk_webview_initialization_failure(
        state: &Rc<RefCell<WebRuntimeState>>,
        detail_label: &gtk::Label,
        context: &str,
        error: WindowsError,
    ) {
        report_gtk_webview_initialization_message(
            state,
            detail_label,
            &format!("{context}: {error}"),
        );
    }

    fn report_gtk_webview_initialization_io_failure(
        state: &Rc<RefCell<WebRuntimeState>>,
        detail_label: &gtk::Label,
        context: &str,
        error: std::io::Error,
    ) {
        report_gtk_webview_initialization_message(
            state,
            detail_label,
            &format!("{context}: {error}"),
        );
    }

    fn report_gtk_webview_initialization_timeout(
        state: &Rc<RefCell<WebRuntimeState>>,
        detail_label: &gtk::Label,
        context: &str,
        timeout_seconds: u64,
    ) {
        report_gtk_webview_initialization_message(
            state,
            detail_label,
            &format!("{context} did not complete after {timeout_seconds}s"),
        );
    }

    fn report_gtk_webview_initialization_message(
        state: &Rc<RefCell<WebRuntimeState>>,
        detail_label: &gtk::Label,
        message: &str,
    ) {
        let (shutdown, current_url) = {
            let state = state.borrow();
            (state.shutdown, state.current_url.clone())
        };
        if shutdown {
            logging::info(format!(
                "Windows GTK WebView2 initialization message ignored after shutdown: {message}"
            ));
            return;
        }
        detail_label.set_text(&web_runtime_detail(
            &format!(
                "Recoverable WebView2 initialization error: {message}. Open Externally remains available"
            ),
            &current_url,
            None,
        ));
        logging::error(message);
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_gtk_webview_initialization(
        environment: ICoreWebView2Environment,
        controller: ICoreWebView2Controller,
        host: &gtk::Widget,
        state: &Rc<RefCell<WebRuntimeState>>,
        runtime_status: &str,
        title: &gtk::Label,
        detail_label: &gtk::Label,
        context_popover: &gtk::Popover,
    ) {
        if state.borrow().shutdown {
            return;
        }

        let webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => webview,
            Err(error) => {
                report_gtk_webview_initialization_failure(
                    state,
                    detail_label,
                    "Windows GTK WebView2 controller access failed",
                    error,
                );
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
