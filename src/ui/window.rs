use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread_local;
use std::time::{Duration, Instant};

use adw::prelude::*;
use glib::value::ToValue;
use gtk::{gdk, gio, glib, pango};

use crate::extension::RuntimeOptions;
use crate::gtk_shell;
use crate::logging;
use crate::model::assets::RestoreLaunchMode;
use crate::model::preset::{ApplicationDensity, WorkspacePreset};
use crate::services::session_restore::{
    flatten_window_sessions, session_for_restore_mode, shell_only_session,
};
use crate::storage::asset_store::AssetStore;
use crate::storage::preference_store::{AppPreferences, PreferenceStore};
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
use crate::terminal::session::clamp_terminal_zoom_steps;
use crate::tray::TrayController;
use crate::ui::app_chrome::{
    build_app_header_chrome, build_main_titlebar_actions, build_window_shell,
    sync_workspace_fullscreen_chrome,
};
use crate::ui::appearance::{
    apply_optional_window_density, apply_theme_mode, resolved_theme_uses_dark_palette,
    window_uses_dark_theme,
};
use crate::ui::icons::{self, name as icon_name};
use crate::ui::{
    about_dialog, assets_manager, command_palette, companion_dialog, context_menu, dialog_smoke,
    launch_screen, settings_dialog, tab_rename_dialog,
    title_chrome::{TitleTabChrome, apply_title_tab_state, build_title_tab_chrome},
    workspace_view,
};
use crate::voice::audio::AudioCapture;
use crate::voice::engine::{self, VoiceEngineEvent};
use crate::voice::linux_global_hotkey::{LinuxGlobalHotkeyEvent, LinuxGlobalHotkeyHandle};
use crate::voice::pack::{self, VoicePackHealth};
use crate::voice::{ParakeetTranscriber, VoiceActivationMode, VoiceEngineMode, VoicePackStatus};

type SelectTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type TabActionHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type TabPredicateHandle = Rc<RefCell<Option<Box<dyn Fn(usize) -> bool>>>>;
type RenameTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, Option<String>)>>>>;
type ReorderTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, usize)>>>>;
type ShowWorkspaceHandle = Rc<RefCell<Option<Box<dyn Fn(usize, WorkspacePreset, PathBuf)>>>>;
type VoidHandle = Rc<RefCell<Option<Box<dyn Fn()>>>>;
type ShortcutControllerHandle = Rc<RefCell<Option<gtk::ShortcutController>>>;
type VoiceKeyControllerHandle = Rc<RefCell<Option<gtk::EventControllerKey>>>;
type TabStripControllerHandle = Rc<RefCell<TabStripController>>;
type WorkspaceLayoutTargetHandle = Rc<RefCell<Option<WorkspaceLayoutTarget>>>;
type AttachWorkspaceTabHandle = Rc<dyn Fn(WorkspaceTab)>;

const DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT: &str = "F11";
const DEFAULT_WORKSPACE_DENSITY_SHORTCUT: &str = "<Ctrl><Shift>D";
const DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT: &str = "<Ctrl>plus";
const DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT: &str = "<Ctrl>minus";
const DEFAULT_COMMAND_PALETTE_SHORTCUT: &str = "<Ctrl><Shift>P";
const VOICE_AUDIO_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

static NEXT_LINUX_WINDOW_ID: AtomicUsize = AtomicUsize::new(1);
static LINUX_SESSION_REGISTRY: OnceLock<Mutex<LinuxSessionRegistry>> = OnceLock::new();

thread_local! {
    static LINUX_MAIN_ATTACH_TARGETS: RefCell<Vec<LinuxMainAttachTarget>> = const { RefCell::new(Vec::new()) };
}

fn shortcut_display_label(
    _window: &adw::ApplicationWindow,
    accelerator: &str,
    fallback: &str,
) -> String {
    let trigger = gtk::ShortcutTrigger::parse_string(accelerator.trim())
        .or_else(|| gtk::ShortcutTrigger::parse_string(fallback))
        .expect("default shortcut trigger should parse");
    if let Some(display) = gdk::Display::default() {
        trigger.to_label(&display).to_string()
    } else {
        accelerator.trim().to_string()
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

#[derive(Clone)]
struct WorkspaceTab {
    id: usize,
    default_title: String,
    custom_title: Option<String>,
    subtitle: String,
    page_shell: gtk::Box,
    content: TabContent,
    workspace_root: Option<PathBuf>,
}

#[derive(Clone)]
enum TabContent {
    LaunchDeck,
    Workspace(Box<WorkspaceState>),
}

#[derive(Clone)]
struct WorkspaceState {
    preset: WorkspacePreset,
    assets: crate::model::assets::WorkspaceAssets,
    runtime: workspace_view::WorkspaceRuntime,
    terminal_zoom_steps: i32,
    layout_target: WorkspaceLayoutTargetHandle,
}

#[derive(Clone)]
struct SessionPersistence {
    window_id: usize,
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: Rc<Cell<usize>>,
    session_store: Rc<SessionStore>,
    suppression_depth: Rc<Cell<usize>>,
    pending_save: Rc<Cell<bool>>,
}

struct SessionSaveGuard {
    suppression_depth: Rc<Cell<usize>>,
}

impl Drop for SessionSaveGuard {
    fn drop(&mut self) {
        self.suppression_depth
            .set(self.suppression_depth.get().saturating_sub(1));
    }
}

impl SessionPersistence {
    fn new(
        window_id: usize,
        tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
        active_tab_id: Rc<Cell<usize>>,
        session_store: Rc<SessionStore>,
    ) -> Self {
        Self {
            window_id,
            tabs,
            active_tab_id,
            session_store,
            suppression_depth: Rc::new(Cell::new(0)),
            pending_save: Rc::new(Cell::new(false)),
        }
    }

    fn suppress(&self) -> SessionSaveGuard {
        self.suppression_depth
            .set(self.suppression_depth.get().saturating_add(1));
        SessionSaveGuard {
            suppression_depth: self.suppression_depth.clone(),
        }
    }

    fn save_now(&self, reason: &str) {
        self.pending_save.set(false);
        if self.suppression_depth.get() > 0 {
            logging::info(format!(
                "deferred workspace session save while suppressed reason='{}'",
                reason
            ));
            return;
        }

        logging::info(format!("saving workspace session state reason='{reason}'"));
        save_application_window_session_state(
            self.window_id,
            &self.tabs,
            self.active_tab_id.get(),
            &self.session_store,
        );
    }

    fn save_soon(&self, reason: &'static str) {
        if self.suppression_depth.get() > 0 || self.pending_save.replace(true) {
            return;
        }

        let persistence = self.clone();
        glib::idle_add_local_once(move || {
            persistence.save_now(reason);
        });
    }
}

#[derive(Clone)]
struct VoiceHud {
    revealer: gtk::Revealer,
    status_label: gtk::Label,
    transcript_label: gtk::Label,
}

impl VoiceHud {
    fn new() -> Self {
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .can_target(false)
            .build();
        let shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(18)
            .css_classes(["voice-hud"])
            .build();
        let status_label = gtk::Label::builder()
            .label("Voice ready")
            .halign(gtk::Align::Start)
            .css_classes(["voice-hud-status"])
            .build();
        let transcript_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::Start)
            .ellipsize(pango::EllipsizeMode::End)
            .max_width_chars(72)
            .css_classes(["voice-hud-transcript"])
            .build();
        shell.append(&status_label);
        shell.append(&transcript_label);
        revealer.set_child(Some(&shell));
        Self {
            revealer,
            status_label,
            transcript_label,
        }
    }

    fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    fn show(&self, status: &str, transcript: Option<&str>) {
        self.status_label.set_label(status);
        self.transcript_label.set_label(transcript.unwrap_or(""));
        self.transcript_label
            .set_visible(transcript.is_some_and(|value| !value.trim().is_empty()));
        self.revealer.set_reveal_child(true);
    }

    fn hide_later(&self) {
        let revealer = self.revealer.clone();
        glib::timeout_add_seconds_local_once(3, move || {
            revealer.set_reveal_child(false);
        });
    }
}

#[derive(Debug)]
enum VoiceUiEvent {
    WarmRequested,
    WarmReady(u64),
    WarmFailed { generation: u64, message: String },
    ListeningStarted,
    ListeningCancelled(String),
    Final(String),
    Partial(String),
    Status(String),
    Error(String),
    HotkeyPressed,
    HotkeyReleased,
    Toast(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum VoiceWarmState {
    #[default]
    Cold,
    Warming,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceHotkeyWarmGate {
    StartCapture,
    WaitForWarm,
    RequestWarm,
    ReportFailure,
}

fn voice_hotkey_warm_gate(state: VoiceWarmState) -> VoiceHotkeyWarmGate {
    match state {
        VoiceWarmState::Ready => VoiceHotkeyWarmGate::StartCapture,
        VoiceWarmState::Warming => VoiceHotkeyWarmGate::WaitForWarm,
        VoiceWarmState::Cold => VoiceHotkeyWarmGate::RequestWarm,
        VoiceWarmState::Failed => VoiceHotkeyWarmGate::ReportFailure,
    }
}

fn apply_voice_listening_started(
    voice_starting: &Cell<bool>,
    voice_listening: &Cell<bool>,
    voice_stopping: &Cell<bool>,
) {
    voice_starting.set(false);
    voice_listening.set(!voice_stopping.get());
}

enum VoiceTranscriberCommand {
    Prepare {
        manifest: pack::VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
        warm_generation: u64,
        ui_tx: mpsc::Sender<VoiceUiEvent>,
    },
    Start {
        manifest: pack::VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
        microphone_id: Option<String>,
        ui_tx: mpsc::Sender<VoiceUiEvent>,
    },
    Flush {
        ui_tx: mpsc::Sender<VoiceUiEvent>,
    },
    Stop {
        ui_tx: mpsc::Sender<VoiceUiEvent>,
    },
    Reset,
    Shutdown,
}

#[derive(Clone)]
struct VoiceTranscriberHandle {
    tx: mpsc::Sender<VoiceTranscriberCommand>,
}

impl VoiceTranscriberHandle {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel::<VoiceTranscriberCommand>();
        std::thread::spawn(move || run_voice_transcriber_worker(rx));
        Self { tx }
    }

    fn prepare(
        &self,
        manifest: pack::VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
        warm_generation: u64,
        ui_tx: &mpsc::Sender<VoiceUiEvent>,
    ) {
        let _ = self.tx.send(VoiceTranscriberCommand::Prepare {
            manifest,
            health,
            engine_mode,
            warm_generation,
            ui_tx: ui_tx.clone(),
        });
    }

    fn start_capture(
        &self,
        manifest: pack::VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
        microphone_id: Option<String>,
        ui_tx: &mpsc::Sender<VoiceUiEvent>,
    ) {
        let _ = self.tx.send(VoiceTranscriberCommand::Start {
            manifest,
            health,
            engine_mode,
            microphone_id,
            ui_tx: ui_tx.clone(),
        });
    }

    fn flush(&self, ui_tx: &mpsc::Sender<VoiceUiEvent>) {
        let _ = self.tx.send(VoiceTranscriberCommand::Flush {
            ui_tx: ui_tx.clone(),
        });
    }

    fn stop(&self, ui_tx: &mpsc::Sender<VoiceUiEvent>) {
        let _ = self.tx.send(VoiceTranscriberCommand::Stop {
            ui_tx: ui_tx.clone(),
        });
    }

    fn reset(&self) {
        let _ = self.tx.send(VoiceTranscriberCommand::Reset);
    }

    fn shutdown(&self) {
        let _ = self.tx.send(VoiceTranscriberCommand::Shutdown);
    }
}

fn run_voice_transcriber_worker(rx: mpsc::Receiver<VoiceTranscriberCommand>) {
    let mut transcriber = None::<ParakeetTranscriber>;
    let mut current_engine_mode = None::<VoiceEngineMode>;
    let mut model_warmed = false;
    let mut first_partial_started_at = None::<Instant>;

    for command in rx {
        match command {
            VoiceTranscriberCommand::Prepare {
                manifest,
                health,
                engine_mode,
                warm_generation,
                ui_tx,
            } => {
                match ensure_voice_helper(
                    &mut transcriber,
                    &mut current_engine_mode,
                    &mut model_warmed,
                    manifest,
                    health,
                    engine_mode,
                ) {
                    Ok(()) => match warm_voice_model(transcriber.as_mut()) {
                        Ok(()) => {
                            model_warmed = true;
                            let _ = ui_tx.send(VoiceUiEvent::WarmReady(warm_generation));
                        }
                        Err(message) => {
                            if let Some(transcriber) = transcriber.take() {
                                let _ = transcriber.shutdown();
                            }
                            current_engine_mode = None;
                            model_warmed = false;
                            let _ = ui_tx.send(VoiceUiEvent::WarmFailed {
                                generation: warm_generation,
                                message,
                            });
                        }
                    },
                    Err(message) => {
                        current_engine_mode = None;
                        model_warmed = false;
                        let _ = ui_tx.send(VoiceUiEvent::WarmFailed {
                            generation: warm_generation,
                            message,
                        });
                    }
                }
            }
            VoiceTranscriberCommand::Start {
                manifest,
                health,
                engine_mode,
                microphone_id,
                ui_tx,
            } => {
                match ensure_voice_helper(
                    &mut transcriber,
                    &mut current_engine_mode,
                    &mut model_warmed,
                    manifest,
                    health,
                    engine_mode,
                )
                .and_then(|_| {
                    if !model_warmed {
                        let _ = ui_tx.send(VoiceUiEvent::ListeningCancelled(
                            "Voice model is preparing; try again shortly.".into(),
                        ));
                        return Ok(());
                    }
                    transcriber
                        .as_mut()
                        .ok_or_else(|| "voice transcriber unavailable".to_string())?
                        .start_capture(microphone_id.as_deref())
                        .map_err(|error| format!("{error:?}"))
                }) {
                    Ok(()) => {
                        if model_warmed {
                            first_partial_started_at = Some(Instant::now());
                            let _ = ui_tx.send(VoiceUiEvent::ListeningStarted);
                            let _ = ui_tx.send(VoiceUiEvent::Status("Listening…".into()));
                        }
                    }
                    Err(message) => {
                        let _ = ui_tx.send(VoiceUiEvent::Error(message));
                    }
                }
            }
            VoiceTranscriberCommand::Flush { ui_tx } => {
                let Some(transcriber) = transcriber.as_mut() else {
                    continue;
                };
                let flushed_at = Instant::now();
                match transcriber.flush_captured_audio() {
                    Ok(Some(partial)) => {
                        logging::info(format!(
                            "voice audio flush returned partial elapsed_ms={}",
                            flushed_at.elapsed().as_millis()
                        ));
                        if let Some(started) = first_partial_started_at.take() {
                            let elapsed_ms = started.elapsed().as_millis();
                            let _ = ui_tx.send(VoiceUiEvent::Status(format!(
                                "First partial in {elapsed_ms}ms"
                            )));
                        }
                        let _ = ui_tx.send(VoiceUiEvent::Partial(partial));
                    }
                    Ok(None) => {
                        logging::info(format!(
                            "voice audio flush buffered without partial elapsed_ms={}",
                            flushed_at.elapsed().as_millis()
                        ));
                    }
                    Err(error) => {
                        let _ = ui_tx.send(VoiceUiEvent::Error(format!("{error:?}")));
                    }
                }
            }
            VoiceTranscriberCommand::Stop { ui_tx } => {
                first_partial_started_at = None;
                let Some(transcriber) = transcriber.as_mut() else {
                    continue;
                };
                let released_at = Instant::now();
                let partial_tx = ui_tx.clone();
                logging::info("voice capture finalization started");
                let result = transcriber.stop_capture_and_transcribe_with_partials(|partial| {
                    let _ = partial_tx.send(VoiceUiEvent::Partial(partial));
                });
                match result {
                    Ok(text) => {
                        let elapsed_ms = released_at.elapsed().as_millis();
                        logging::info(format!(
                            "voice capture finalized text_len={} elapsed_ms={elapsed_ms}",
                            text.len()
                        ));
                        let _ = ui_tx.send(VoiceUiEvent::Status(format!(
                            "Final after release in {elapsed_ms}ms"
                        )));
                        let _ = ui_tx.send(VoiceUiEvent::Final(text));
                    }
                    Err(error) => {
                        let elapsed_ms = released_at.elapsed().as_millis();
                        logging::error(format!(
                            "voice capture finalization failed after {elapsed_ms}ms: {error:?}"
                        ));
                        let _ = ui_tx.send(VoiceUiEvent::Error(format!("{error:?}")));
                    }
                }
            }
            VoiceTranscriberCommand::Reset => {
                if let Some(transcriber) = transcriber.take() {
                    let _ = transcriber.shutdown();
                }
                current_engine_mode = None;
                model_warmed = false;
                first_partial_started_at = None;
            }
            VoiceTranscriberCommand::Shutdown => {
                if let Some(transcriber) = transcriber.take() {
                    let _ = transcriber.shutdown();
                }
                break;
            }
        }
    }
}

fn ensure_voice_helper(
    transcriber: &mut Option<ParakeetTranscriber>,
    current_engine_mode: &mut Option<VoiceEngineMode>,
    model_warmed: &mut bool,
    manifest: pack::VoicePackManifest,
    health: VoicePackHealth,
    engine_mode: VoiceEngineMode,
) -> Result<(), String> {
    if transcriber.is_some() && *current_engine_mode == Some(engine_mode) {
        return Ok(());
    }
    if let Some(transcriber) = transcriber.take() {
        let _ = transcriber.shutdown();
    }
    *model_warmed = false;
    let launched = ParakeetTranscriber::launch(&manifest, health, engine_mode)
        .map_err(|error| format!("{error:?}"))?;
    *transcriber = Some(launched);
    *current_engine_mode = Some(engine_mode);
    Ok(())
}

fn warm_voice_model(transcriber: Option<&mut ParakeetTranscriber>) -> Result<(), String> {
    let Some(transcriber) = transcriber else {
        return Err("voice transcriber unavailable".into());
    };
    let warm_started = Instant::now();
    transcriber
        .warm_up()
        .map_err(|error| format!("{error:?}"))?;
    let capabilities = transcriber
        .capabilities()
        .map_err(|error| format!("{error:?}"))?;
    let elapsed_ms = warm_started.elapsed().as_millis();
    logging::info(format!(
        "Voice model ready in {elapsed_ms}ms ({}, streaming={})",
        capabilities.device, capabilities.streaming
    ));
    Ok(())
}

fn refresh_builtin_voice_pack_assets_for_runtime(root: &Path) -> Result<(), String> {
    match pack::refresh_builtin_parakeet_pack_assets(root) {
        Ok(Some(manifest)) => {
            logging::info(format!(
                "refreshed bundled NVIDIA Parakeet voice pack assets id={} version={}",
                manifest.id, manifest.version
            ));
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(format!("{error:?}")),
    }
}

enum VoiceGlobalHotkeyRegistration {
    Active {
        shortcut: String,
        #[allow(dead_code)]
        handle: LinuxGlobalHotkeyHandle,
    },
    Unavailable {
        shortcut: String,
        last_attempt: Instant,
    },
}

impl VoiceGlobalHotkeyRegistration {
    fn shortcut(&self) -> &str {
        match self {
            Self::Active { shortcut, .. } | Self::Unavailable { shortcut, .. } => shortcut,
        }
    }

    fn unavailable_retry_pending(&self) -> bool {
        match self {
            Self::Unavailable { last_attempt, .. } => {
                last_attempt.elapsed() < Duration::from_secs(30)
            }
            Self::Active { .. } => false,
        }
    }
}

#[derive(Clone)]
struct LaunchTabContext {
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    window: adw::ApplicationWindow,
    preference_store: Rc<PreferenceStore>,
    preset_store: Rc<PresetStore>,
    asset_store: Rc<AssetStore>,
    show_workspace_handle: ShowWorkspaceHandle,
    close_tab_handle: TabActionHandle,
    refresh_launch_tabs: VoidHandle,
}

struct RestoreSessionContext {
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    next_tab_id: Rc<Cell<usize>>,
    tab_view: adw::TabView,
    select_tab: SelectTabHandle,
    active_tab_id: Rc<Cell<usize>>,
    forced_tab_closes: Rc<RefCell<HashSet<usize>>>,
    suppress_empty_replacement: Rc<Cell<bool>>,
    asset_store: Rc<AssetStore>,
    preference_store: Rc<PreferenceStore>,
    session_persistence: SessionPersistence,
}

impl Clone for RestoreSessionContext {
    fn clone(&self) -> Self {
        Self {
            tabs: self.tabs.clone(),
            next_tab_id: self.next_tab_id.clone(),
            tab_view: self.tab_view.clone(),
            select_tab: self.select_tab.clone(),
            active_tab_id: self.active_tab_id.clone(),
            forced_tab_closes: self.forced_tab_closes.clone(),
            suppress_empty_replacement: self.suppress_empty_replacement.clone(),
            asset_store: self.asset_store.clone(),
            preference_store: self.preference_store.clone(),
            session_persistence: self.session_persistence.clone(),
        }
    }
}

#[derive(Default)]
struct LinuxSessionRegistry {
    windows: BTreeMap<usize, SavedSession>,
    active_window_id: Option<usize>,
}

struct DetachPayload {
    origin_window_id: usize,
    tab: WorkspaceTab,
    saved_tab: SavedTab,
}

#[derive(Clone)]
struct WorkspaceLayoutTarget {
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    tab_id: usize,
}

#[derive(Clone)]
struct LinuxMainAttachTarget {
    window_id: usize,
    window: glib::WeakRef<adw::ApplicationWindow>,
    attach_workspace_tab: AttachWorkspaceTabHandle,
}

#[derive(Clone)]
struct TabStripItem {
    tab_id: usize,
    shell: gtk::Box,
    chrome: TitleTabChrome,
}

struct TabStripDragState {
    dragged_id: usize,
    origin_index: usize,
    preview_index: usize,
}

struct TabStripController {
    tabs_box: gtk::Box,
    items: Vec<TabStripItem>,
    order: Vec<usize>,
    drag_state: Option<TabStripDragState>,
    select_tab: SelectTabHandle,
    close_tab: TabActionHandle,
    request_tab_rename: TabActionHandle,
    detach_tab: TabActionHandle,
    can_detach_tab: TabPredicateHandle,
}

#[allow(clippy::too_many_arguments)]
pub fn present(
    app: &adw::Application,
    preference_store: PreferenceStore,
    preset_store: PresetStore,
    asset_store: AssetStore,
    session_store: SessionStore,
    saved_session: Option<SavedSession>,
    startup_warning: Option<String>,
    tray_controller: TrayController,
    options: RuntimeOptions,
) {
    present_with_initial_workspace(
        app,
        preference_store,
        preset_store,
        asset_store,
        session_store,
        saved_session,
        startup_warning,
        tray_controller,
        options,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn present_with_initial_workspace(
    app: &adw::Application,
    preference_store: PreferenceStore,
    preset_store: PresetStore,
    asset_store: AssetStore,
    session_store: SessionStore,
    saved_session: Option<SavedSession>,
    startup_warning: Option<String>,
    tray_controller: TrayController,
    options: RuntimeOptions,
    initial_workspace_tab: Option<WorkspaceTab>,
) {
    let preference_store = Rc::new(preference_store);
    let preset_store = Rc::new(preset_store);
    let asset_store = Rc::new(asset_store);
    let session_store = Rc::new(session_store);

    let app_header = build_app_header_chrome();
    let header = app_header.header;
    let tab_view = adw::TabView::builder().hexpand(true).vexpand(true).build();
    let title = app_header.title;

    let voice_hud = VoiceHud::new();
    let workspace_overlay = gtk::Overlay::new();
    workspace_overlay.set_child(Some(&tab_view));
    workspace_overlay.add_overlay(&voice_hud.widget());

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&workspace_overlay));

    let close_to_background_notice = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .reveal_child(false)
        .build();
    let close_to_background_notice_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(18)
        .margin_end(18)
        .build();
    close_to_background_notice_row.add_css_class("card");
    close_to_background_notice_row.append(
        &gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .pixel_size(18)
            .valign(gtk::Align::Center)
            .build(),
    );
    close_to_background_notice_row.append(
        &gtk::Label::builder()
            .label("Close-to-background is enabled, but no system tray watcher is available. Closing the window will quit TerminalTiler normally.")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let close_to_background_notice_button = icons::labeled_button(
        "Open Settings",
        icon_name::SETTINGS,
        &["pill-button", "suggested-action"],
    );
    close_to_background_notice_button.set_valign(gtk::Align::Center);
    close_to_background_notice_row.append(&close_to_background_notice_button);
    close_to_background_notice.set_child(Some(&close_to_background_notice_row));

    let window_shell = build_window_shell();
    window_shell.append(&header);
    window_shell.append(&close_to_background_notice);
    window_shell.append(&toast_overlay);

    let window_id = NEXT_LINUX_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(&options.product.app_title)
        .default_width(gtk_shell::DEFAULT_WINDOW_WIDTH)
        .default_height(gtk_shell::DEFAULT_WINDOW_HEIGHT)
        .resizable(true)
        .content(&window_shell)
        .build();
    window.add_css_class("window-shell");

    let titlebar_actions = build_main_titlebar_actions(&header, options.companion.is_some());
    let back_button = titlebar_actions.back_button;
    let fullscreen_button = titlebar_actions.fullscreen_button;
    let settings_button = titlebar_actions.settings_button;
    let companion_button = titlebar_actions.companion_button;
    let assets_button = titlebar_actions.assets_button;

    let tabs = Rc::new(RefCell::new(Vec::<WorkspaceTab>::new()));
    let next_tab_id = Rc::new(Cell::new(1usize));
    let active_tab_id = Rc::new(Cell::new(0usize));
    let select_tab: SelectTabHandle = Rc::new(RefCell::new(None));
    let close_tab: TabActionHandle = Rc::new(RefCell::new(None));
    let request_tab_rename: TabActionHandle = Rc::new(RefCell::new(None));
    let detach_tab: TabActionHandle = Rc::new(RefCell::new(None));
    let can_detach_tab: TabPredicateHandle = Rc::new(RefCell::new(None));
    let apply_tab_rename: RenameTabHandle = Rc::new(RefCell::new(None));
    let reorder_tab: ReorderTabHandle = Rc::new(RefCell::new(None));
    let show_workspace_in_tab: ShowWorkspaceHandle = Rc::new(RefCell::new(None));
    let refresh_launch_tabs: VoidHandle = Rc::new(RefCell::new(None));
    let add_workspace_tab: VoidHandle = Rc::new(RefCell::new(None));
    let forced_tab_closes = Rc::new(RefCell::new(HashSet::<usize>::new()));
    let suppress_empty_replacement = Rc::new(Cell::new(false));
    let current_shortcuts = preference_store.load();
    let current_fullscreen_shortcut = Rc::new(RefCell::new(
        current_shortcuts.workspace_fullscreen_shortcut.clone(),
    ));
    let current_density_shortcut = Rc::new(RefCell::new(
        current_shortcuts.workspace_density_shortcut.clone(),
    ));
    let current_close_to_background = Rc::new(Cell::new(current_shortcuts.close_to_background));
    let current_zoom_in_shortcut = Rc::new(RefCell::new(
        current_shortcuts.workspace_zoom_in_shortcut.clone(),
    ));
    let current_zoom_out_shortcut = Rc::new(RefCell::new(
        current_shortcuts.workspace_zoom_out_shortcut.clone(),
    ));
    let current_command_palette_shortcut = Rc::new(RefCell::new(
        current_shortcuts.command_palette_shortcut.clone(),
    ));
    let voice_key_controller: VoiceKeyControllerHandle = Rc::new(RefCell::new(None));
    let voice_transcriber = Rc::new(VoiceTranscriberHandle::start());
    let voice_listening = Rc::new(Cell::new(false));
    let voice_starting = Rc::new(Cell::new(false));
    let voice_stopping = Rc::new(Cell::new(false));
    let voice_local_key_pressed = Rc::new(Cell::new(false));
    let voice_warm_state = Rc::new(Cell::new(VoiceWarmState::Cold));
    let voice_warm_generation = Rc::new(Cell::new(0_u64));
    let voice_warm_error = Rc::new(RefCell::new(None::<String>));
    let voice_global_hotkey = Rc::new(RefCell::new(None::<VoiceGlobalHotkeyRegistration>));
    let (voice_event_tx, voice_event_rx) = mpsc::channel::<VoiceUiEvent>();
    let quit_requested = Rc::new(Cell::new(false));
    let force_quit_requested = Rc::new(Cell::new(false));
    let session_persistence = SessionPersistence::new(
        window_id,
        tabs.clone(),
        active_tab_id.clone(),
        session_store.clone(),
    );
    let startup_restore_suppression = Rc::new(RefCell::new(
        saved_session
            .as_ref()
            .map(|_| session_persistence.suppress()),
    ));
    let tab_strip_controller = create_tab_strip_controller(
        &title.tabs_box,
        &title.root,
        select_tab.clone(),
        close_tab.clone(),
        request_tab_rename.clone(),
        detach_tab.clone(),
        can_detach_tab.clone(),
        reorder_tab.clone(),
    );
    let refresh_tab_strip: Rc<dyn Fn()> = {
        let controller = tab_strip_controller.clone();
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        Rc::new(move || {
            let tabs = tabs.borrow();
            sync_tab_strip(&controller, &tabs, active_tab_id.get());
        })
    };
    let fullscreen_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let density_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let zoom_in_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let zoom_out_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let command_palette_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let sync_close_to_background_notice: Rc<dyn Fn()> = {
        let close_to_background_notice = close_to_background_notice.clone();
        let current_close_to_background = current_close_to_background.clone();
        let tray_controller = tray_controller.clone();
        Rc::new(move || {
            close_to_background_notice.set_reveal_child(
                current_close_to_background.get() && !tray_controller.is_available(),
            );
        })
    };

    {
        let sync_close_to_background_notice = sync_close_to_background_notice.clone();
        sync_close_to_background_notice();
        glib::timeout_add_seconds_local(1, move || {
            sync_close_to_background_notice();
            glib::ControlFlow::Continue
        });
    }

    {
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let voice_hud = voice_hud.clone();
        let toast_overlay = toast_overlay.clone();
        let preference_store = preference_store.clone();
        let voice_transcriber = voice_transcriber.clone();
        let voice_listening = voice_listening.clone();
        let voice_starting = voice_starting.clone();
        let voice_stopping = voice_stopping.clone();
        let voice_warm_state = voice_warm_state.clone();
        let voice_warm_generation = voice_warm_generation.clone();
        let voice_warm_error = voice_warm_error.clone();
        let voice_event_tx_for_handler = voice_event_tx.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            while let Ok(event) = voice_event_rx.try_recv() {
                match event {
                    VoiceUiEvent::WarmRequested => {
                        warm_voice_engine_if_ready(
                            &preference_store,
                            &voice_transcriber,
                            &voice_event_tx_for_handler,
                            &voice_warm_state,
                            &voice_warm_generation,
                            &voice_warm_error,
                        );
                    }
                    VoiceUiEvent::WarmReady(generation) => {
                        if generation != voice_warm_generation.get() {
                            continue;
                        }
                        voice_warm_state.set(VoiceWarmState::Ready);
                        voice_warm_error.borrow_mut().take();
                    }
                    VoiceUiEvent::WarmFailed {
                        generation,
                        message,
                    } => {
                        if generation != voice_warm_generation.get() {
                            continue;
                        }
                        voice_warm_state.set(VoiceWarmState::Failed);
                        voice_warm_error.replace(Some(message.clone()));
                        logging::error(format!("voice model warm-up failed: {message}"));
                    }
                    VoiceUiEvent::ListeningStarted => {
                        apply_voice_listening_started(
                            &voice_starting,
                            &voice_listening,
                            &voice_stopping,
                        );
                    }
                    VoiceUiEvent::ListeningCancelled(message) => {
                        voice_starting.set(false);
                        voice_listening.set(false);
                        voice_stopping.set(false);
                        voice_hud.show(&message, None);
                    }
                    VoiceUiEvent::Final(text) => {
                        voice_starting.set(false);
                        voice_listening.set(false);
                        voice_stopping.set(false);
                        if text.trim().is_empty() {
                            voice_hud.show("No speech detected", None);
                            voice_hud.hide_later();
                            continue;
                        }
                        let inserted = active_workspace_runtime(&tabs, active_tab_id.get())
                            .map(|runtime| runtime.send_text_to_focused_terminal(&text))
                            .unwrap_or(false);
                        if inserted {
                            voice_hud.show("Voice inserted", Some(&text));
                            voice_hud.hide_later();
                        } else {
                            voice_hud.show("No focused terminal target", Some(&text));
                            show_toast(
                                &toast_overlay,
                                "Voice text was not inserted: no focused terminal pane",
                            );
                        }
                    }
                    VoiceUiEvent::Error(message) => {
                        voice_starting.set(false);
                        voice_listening.set(false);
                        voice_stopping.set(false);
                        logging::error(format!("voice transcription failed: {message}"));
                        voice_hud.show("Voice error", Some(&message));
                        show_toast(&toast_overlay, "Voice transcription failed");
                    }
                    VoiceUiEvent::Partial(text) => {
                        voice_hud.show("Voice partial", Some(&text));
                    }
                    VoiceUiEvent::Status(message) => {
                        if voice_stopping.get() && message == "Listening…" {
                            continue;
                        }
                        voice_hud.show(&message, None);
                    }
                    VoiceUiEvent::HotkeyPressed => {
                        let voice = preference_store.load().voice;
                        logging::info(format!(
                            "voice hotkey pressed enabled={} mode={} listening={} starting={} stopping={} warm={:?}",
                            voice.enabled,
                            voice.activation_mode.label(),
                            voice_listening.get(),
                            voice_starting.get(),
                            voice_stopping.get(),
                            voice_warm_state.get(),
                        ));
                        if !voice.enabled {
                            continue;
                        }
                        match voice.activation_mode {
                            VoiceActivationMode::Toggle if voice_listening.get() => {
                                stop_voice_capture(
                                    &voice_transcriber,
                                    &voice_listening,
                                    &voice_stopping,
                                    &voice_hud,
                                    &voice_event_tx_for_handler,
                                );
                            }
                            VoiceActivationMode::Toggle | VoiceActivationMode::PushToTalk => {
                                if !voice_listening.get()
                                    && !voice_starting.get()
                                    && !voice_stopping.get()
                                {
                                    start_voice_capture(
                                        &preference_store,
                                        &tabs,
                                        active_tab_id.get(),
                                        &voice_hud,
                                        &toast_overlay,
                                        &voice_transcriber,
                                        &voice_listening,
                                        &voice_starting,
                                        &voice_stopping,
                                        &voice_warm_state,
                                        &voice_warm_generation,
                                        &voice_warm_error,
                                        &voice_event_tx_for_handler,
                                    );
                                } else {
                                    logging::info(format!(
                                        "voice hotkey press ignored while busy listening={} starting={} stopping={}",
                                        voice_listening.get(),
                                        voice_starting.get(),
                                        voice_stopping.get(),
                                    ));
                                }
                            }
                        }
                    }
                    VoiceUiEvent::HotkeyReleased => {
                        let voice = preference_store.load().voice;
                        logging::info(format!(
                            "voice hotkey released enabled={} mode={} listening={} starting={} stopping={}",
                            voice.enabled,
                            voice.activation_mode.label(),
                            voice_listening.get(),
                            voice_starting.get(),
                            voice_stopping.get(),
                        ));
                        if voice.enabled
                            && voice.activation_mode == VoiceActivationMode::PushToTalk
                            && voice_starting.replace(false)
                            && !voice_listening.get()
                        {
                            finish_pending_voice_capture(
                                &voice_transcriber,
                                &voice_stopping,
                                &voice_hud,
                                &voice_event_tx_for_handler,
                            );
                        } else if voice.enabled
                            && voice.activation_mode == VoiceActivationMode::PushToTalk
                        {
                            stop_voice_capture(
                                &voice_transcriber,
                                &voice_listening,
                                &voice_stopping,
                                &voice_hud,
                                &voice_event_tx_for_handler,
                            );
                        }
                    }
                    VoiceUiEvent::Toast(message) => {
                        show_toast(&toast_overlay, &message);
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    install_voice_hotkey_controller(
        &window,
        &voice_key_controller,
        preference_store.clone(),
        tabs.clone(),
        active_tab_id.clone(),
        voice_hud.clone(),
        toast_overlay.clone(),
        voice_transcriber.clone(),
        voice_listening.clone(),
        voice_starting.clone(),
        voice_stopping.clone(),
        voice_local_key_pressed.clone(),
        voice_warm_state.clone(),
        voice_warm_generation.clone(),
        voice_warm_error.clone(),
        voice_event_tx.clone(),
    );

    {
        let preference_store = preference_store.clone();
        let voice_global_hotkey = voice_global_hotkey.clone();
        let voice_event_tx = voice_event_tx.clone();
        sync_linux_voice_global_hotkey(
            &voice_global_hotkey,
            &preference_store.load().voice,
            &voice_event_tx,
        );
        glib::timeout_add_seconds_local(2, {
            let preference_store = preference_store.clone();
            let voice_global_hotkey = voice_global_hotkey.clone();
            let voice_event_tx = voice_event_tx.clone();
            move || {
                sync_linux_voice_global_hotkey(
                    &voice_global_hotkey,
                    &preference_store.load().voice,
                    &voice_event_tx,
                );
                glib::ControlFlow::Continue
            }
        });
    }

    {
        let voice_transcriber = voice_transcriber.clone();
        let voice_listening = voice_listening.clone();
        let voice_event_tx = voice_event_tx.clone();
        glib::timeout_add_local(VOICE_AUDIO_FLUSH_INTERVAL, move || {
            if !voice_listening.get() {
                return glib::ControlFlow::Continue;
            }
            voice_transcriber.flush(&voice_event_tx);
            glib::ControlFlow::Continue
        });
    }

    {
        let title_root_for_select = title.root.clone();
        let tab_view_for_select = tab_view.clone();
        let header_for_select = header.clone();
        let window_for_select = window.clone();
        let back_for_select = back_button.clone();
        let fullscreen_for_select = fullscreen_button.clone();
        let tabs_for_select = tabs.clone();
        let tabs_for_sync = tabs.clone();
        let active_for_select = active_tab_id.clone();
        let preference_store_for_select = preference_store.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let refresh_tab_strip_for_select = refresh_tab_strip.clone();
        let session_persistence_for_select = session_persistence.clone();
        let sync_selected_tab: Rc<dyn Fn(usize)> = Rc::new(move |tab_id| {
            note_linux_main_attach_target_active(window_id);
            let (is_workspace, workspace_profile) = {
                let tabs = tabs_for_sync.borrow();
                let active = tabs
                    .iter()
                    .find(|tab| tab.id == tab_id)
                    .cloned()
                    .expect("active workspace tab should exist");
                match active.content {
                    TabContent::LaunchDeck => (false, None),
                    TabContent::Workspace(workspace) => (
                        true,
                        Some((
                            workspace.preset,
                            workspace.runtime,
                            workspace.terminal_zoom_steps,
                        )),
                    ),
                }
            };

            active_for_select.set(tab_id);

            if let Some((preset, runtime, terminal_zoom_steps)) = workspace_profile.as_ref() {
                apply_shell_profile(&header_for_select, &window_for_select, preset);
                runtime.apply_appearance(
                    window_uses_dark_theme(&window_for_select),
                    preset.density,
                    *terminal_zoom_steps,
                );
            } else {
                apply_launch_profile(
                    &header_for_select,
                    &window_for_select,
                    &preference_store_for_select.load(),
                );
            }
            back_for_select.set_visible(is_workspace);
            sync_fullscreen_chrome(
                &window_for_select,
                title_root_for_select.upcast_ref(),
                &fullscreen_for_select,
                is_workspace,
                current_fullscreen_shortcut.borrow().as_str(),
            );
            refresh_tab_strip_for_select();
            session_persistence_for_select.save_soon("active workspace tab changed");
        });
        {
            let sync_selected_tab = sync_selected_tab.clone();
            *select_tab.borrow_mut() = Some(Box::new(move |tab_id| {
                let page = {
                    let tabs = tabs_for_select.borrow();
                    tab_page_for_id(&tab_view_for_select, &tabs, tab_id)
                };
                let Some(page) = page else {
                    return;
                };
                let selected_page = tab_view_for_select.selected_page();
                if selected_page.as_ref() != Some(&page) {
                    tab_view_for_select.set_selected_page(&page);
                }
                sync_selected_tab(tab_id);
            }));
        }
        {
            let tabs_for_notify = tabs.clone();
            let select_handle = select_tab.clone();
            tab_view.connect_selected_page_notify(move |view| {
                let Some(page) = view.selected_page() else {
                    return;
                };
                let tab_id = {
                    let tabs = tabs_for_notify.borrow();
                    tab_id_for_page(&tabs, &page)
                };
                if let Some(tab_id) = tab_id
                    && let Some(select) = select_handle.borrow().as_ref()
                {
                    select(tab_id);
                }
            });
        }
    }

    {
        let tabs_for_rename = tabs.clone();
        let tab_view_for_rename = tab_view.clone();
        let active_for_rename = active_tab_id.clone();
        let select_for_rename = select_tab.clone();
        let refresh_tab_strip_for_rename = refresh_tab_strip.clone();
        let session_persistence_for_rename = session_persistence.clone();

        *apply_tab_rename.borrow_mut() = Some(Box::new(move |tab_id, requested_title| {
            let requested_title = requested_title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);

            let resolved_title = {
                let mut tabs = tabs_for_rename.borrow_mut();
                let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };

                tab.custom_title = requested_title;
                tab_display_title(tab)
            };
            {
                let tabs = tabs_for_rename.borrow();
                if let Some(tab) = tabs.iter().find(|tab| tab.id == tab_id) {
                    sync_tab_page_metadata(&tab_view_for_rename, tab);
                }
            }
            refresh_tab_strip_for_rename();

            logging::info(format!(
                "workspace tab {} renamed to '{}'",
                tab_id, resolved_title
            ));

            let target_id = active_for_rename.get();
            if target_id != 0
                && let Some(select) = select_for_rename.borrow().as_ref()
            {
                select(target_id);
            }
            session_persistence_for_rename.save_soon("workspace tab renamed");
        }));
    }

    {
        let window_for_rename = window.clone();
        let tabs_for_rename = tabs.clone();
        let apply_rename_handle = apply_tab_rename.clone();

        *request_tab_rename.borrow_mut() = Some(Box::new(move |tab_id| {
            let Some(current_title) = tabs_for_rename
                .borrow()
                .iter()
                .find(|tab| tab.id == tab_id)
                .map(tab_display_title)
            else {
                return;
            };

            let apply_rename_handle = apply_rename_handle.clone();
            tab_rename_dialog::present(
                &window_for_rename,
                &current_title,
                move |requested_title| {
                    if let Some(rename) = apply_rename_handle.borrow().as_ref() {
                        rename(tab_id, requested_title);
                    }
                },
            );
        }));
    }

    {
        let tabs_for_reorder = tabs.clone();
        let tab_view_for_reorder = tab_view.clone();
        *reorder_tab.borrow_mut() = Some(Box::new(move |tab_id, position| {
            let page = {
                let tabs = tabs_for_reorder.borrow();
                tab_page_for_id(&tab_view_for_reorder, &tabs, tab_id)
            };
            let Some(page) = page else {
                return;
            };
            let _ = tab_view_for_reorder.reorder_page(&page, position as i32);
        }));
    }

    {
        let tabs_for_reorder = tabs.clone();
        let active_for_reorder = active_tab_id.clone();
        let select_for_reorder = select_tab.clone();
        let refresh_tab_strip_for_reorder = refresh_tab_strip.clone();
        let session_persistence_for_reorder = session_persistence.clone();
        tab_view.connect_page_reordered(move |_, page, position| {
            let moved_id = {
                let tabs = tabs_for_reorder.borrow();
                tab_id_for_page(&tabs, page)
            };
            let Some(moved_id) = moved_id else {
                return;
            };

            let moved = {
                let mut tabs = tabs_for_reorder.borrow_mut();
                move_tab_to_position(&mut tabs, moved_id, position.max(0) as usize)
            };
            if !moved {
                return;
            }

            logging::info(format!(
                "reordered workspace tab {} to position {}",
                moved_id, position
            ));

            let active_id = active_for_reorder.get();
            if active_id != 0
                && let Some(select) = select_for_reorder.borrow().as_ref()
            {
                select(active_id);
            }
            refresh_tab_strip_for_reorder();
            session_persistence_for_reorder.save_soon("workspace tabs reordered");
        });
    }

    {
        let tabs_for_detach_check = tabs.clone();
        *can_detach_tab.borrow_mut() = Some(Box::new(move |tab_id| {
            tabs_for_detach_check
                .borrow()
                .iter()
                .find(|tab| tab.id == tab_id)
                .map(|tab| matches!(tab.content, TabContent::Workspace(_)))
                .unwrap_or(false)
        }));
    }

    {
        let tabs_for_workspace = tabs.clone();
        let tab_view_for_workspace = tab_view.clone();
        let select_for_workspace = select_tab.clone();
        let refresh_tab_strip_for_workspace = refresh_tab_strip.clone();
        let asset_store = asset_store.clone();
        let preference_store_for_workspace = preference_store.clone();
        let session_persistence_for_workspace = session_persistence.clone();

        *show_workspace_in_tab.borrow_mut() =
            Some(Box::new(move |tab_id, preset, workspace_root| {
                let terminal_zoom_steps = 0;
                let layout_target = make_workspace_layout_target(&tabs_for_workspace, tab_id);
                let assets = asset_store
                    .load_assets_for_workspace_root(&workspace_root)
                    .assets;
                let built_workspace = workspace_view::build_with_layout_change_handler(
                    &preset,
                    &workspace_root,
                    &assets,
                    resolved_theme_uses_dark_palette(preset.theme),
                    terminal_zoom_steps,
                    preference_store_for_workspace.load().max_reconnect_attempts,
                    {
                        let layout_target = layout_target.clone();
                        let session_persistence = session_persistence_for_workspace.clone();
                        Rc::new(move |next_layout| {
                            apply_workspace_layout_change(&layout_target, next_layout);
                            session_persistence.save_soon("workspace layout changed");
                        })
                    },
                );
                let (page_shell, previous_runtime) = {
                    let mut tabs = tabs_for_workspace.borrow_mut();
                    let tab = tabs
                        .iter_mut()
                        .find(|tab| tab.id == tab_id)
                        .expect("workspace tab should exist");
                    let previous_runtime = match &tab.content {
                        TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
                        TabContent::LaunchDeck => None,
                    };
                    tab.subtitle = workspace_root.display().to_string();
                    tab.content = TabContent::Workspace(Box::new(WorkspaceState {
                        preset: preset.clone(),
                        assets: assets.clone(),
                        runtime: built_workspace.runtime.clone(),
                        terminal_zoom_steps,
                        layout_target: layout_target.clone(),
                    }));
                    tab.workspace_root = Some(workspace_root.clone());
                    (tab.page_shell.clone(), previous_runtime)
                };

                if let Some(runtime) = previous_runtime {
                    runtime.terminate_all("replacing workspace view");
                }

                replace_tab_page_content(&page_shell, &built_workspace.widget);
                {
                    let tabs = tabs_for_workspace.borrow();
                    if let Some(tab) = tabs.iter().find(|tab| tab.id == tab_id) {
                        sync_tab_page_metadata(&tab_view_for_workspace, tab);
                    }
                }
                refresh_tab_strip_for_workspace();

                logging::info(format!(
                    "workspace tab {} launched preset='{}' root='{}'",
                    tab_id,
                    preset.name,
                    workspace_root.display()
                ));

                if let Some(select) = select_for_workspace.borrow().as_ref() {
                    select(tab_id);
                }
                session_persistence_for_workspace.save_now("workspace tab launched");
            }));
    }

    {
        let tabs_for_refresh = tabs.clone();
        let window_for_refresh = window.clone();
        let preference_store = preference_store.clone();
        let preset_store = preset_store.clone();
        let asset_store = asset_store.clone();
        let show_workspace_handle = show_workspace_in_tab.clone();
        let close_tab_for_refresh = close_tab.clone();
        let refresh_handle = refresh_launch_tabs.clone();
        let active_for_refresh = active_tab_id.clone();
        let select_for_refresh = select_tab.clone();

        *refresh_launch_tabs.borrow_mut() = Some(Box::new(move || {
            let launch_tab_ids = tabs_for_refresh
                .borrow()
                .iter()
                .filter(|tab| matches!(tab.content, TabContent::LaunchDeck))
                .map(|tab| tab.id)
                .collect::<Vec<_>>();

            for tab_id in launch_tab_ids {
                rebuild_launch_tab(
                    tab_id,
                    &LaunchTabContext {
                        tabs: tabs_for_refresh.clone(),
                        window: window_for_refresh.clone(),
                        preference_store: preference_store.clone(),
                        preset_store: preset_store.clone(),
                        asset_store: asset_store.clone(),
                        show_workspace_handle: show_workspace_handle.clone(),
                        close_tab_handle: close_tab_for_refresh.clone(),
                        refresh_launch_tabs: refresh_handle.clone(),
                    },
                );
            }

            let active_id = active_for_refresh.get();
            if active_id != 0
                && let Some(select) = select_for_refresh.borrow().as_ref()
            {
                select(active_id);
            }
        }));
    }

    {
        let tabs_for_close = tabs.clone();
        let active_for_close = active_tab_id.clone();
        let select_for_close = select_tab.clone();
        let add_for_close = add_workspace_tab.clone();
        let window_for_close = window.clone();
        let forced_tab_closes_for_signal = forced_tab_closes.clone();
        let suppress_empty_replacement_for_signal = suppress_empty_replacement.clone();
        let session_persistence_for_close = session_persistence.clone();
        tab_view.connect_close_page(move |view, page| {
            let tab_id = {
                let tabs = tabs_for_close.borrow();
                tab_id_for_page(&tabs, page)
            };
            let Some(tab_id) = tab_id else {
                view.close_page_finish(page, true);
                return glib::Propagation::Stop;
            };

            let is_workspace = {
                let tabs = tabs_for_close.borrow();
                tabs.iter()
                    .find(|tab| tab.id == tab_id)
                    .map(|tab| matches!(tab.content, TabContent::Workspace(_)))
                    .unwrap_or(false)
            };
            let force_close = forced_tab_closes_for_signal.borrow_mut().remove(&tab_id);

            if is_workspace && !force_close {
                let view = view.clone();
                let page = page.clone();
                let tabs = tabs_for_close.clone();
                let active_tab_id = active_for_close.clone();
                let select_tab = select_for_close.clone();
                let add_workspace_tab = add_for_close.clone();
                let suppress_empty_replacement = suppress_empty_replacement_for_signal.clone();
                let session_persistence = session_persistence_for_close.clone();
                confirm_tab_close(
                    &window_for_close,
                    "Close Workspace?",
                    "Running terminal sessions in this workspace will be terminated.",
                    "Close",
                    move |confirmed| {
                        if confirmed {
                            finish_tab_close(
                                &view,
                                &page,
                                tab_id,
                                &tabs,
                                &active_tab_id,
                                &select_tab,
                                &add_workspace_tab,
                                &suppress_empty_replacement,
                                &session_persistence,
                            );
                        } else {
                            view.close_page_finish(&page, false);
                        }
                    },
                );
                return glib::Propagation::Stop;
            }

            finish_tab_close(
                view,
                page,
                tab_id,
                &tabs_for_close,
                &active_for_close,
                &select_for_close,
                &add_for_close,
                &suppress_empty_replacement_for_signal,
                &session_persistence_for_close,
            );
            glib::Propagation::Stop
        });
    }

    {
        let tabs_for_close = tabs.clone();
        let tab_view_for_close = tab_view.clone();
        *close_tab.borrow_mut() = Some(Box::new(move |tab_id| {
            let page = {
                let tabs = tabs_for_close.borrow();
                tab_page_for_id(&tab_view_for_close, &tabs, tab_id)
            };
            if let Some(page) = page {
                tab_view_for_close.close_page(&page);
            }
        }));
    }

    {
        let app_for_detach = app.clone();
        let tabs_for_detach = tabs.clone();
        let active_for_detach = active_tab_id.clone();
        let tab_view_for_detach = tab_view.clone();
        let select_for_detach = select_tab.clone();
        let add_for_detach = add_workspace_tab.clone();
        let refresh_for_detach = refresh_tab_strip.clone();
        let preference_store_for_detach = preference_store.clone();
        let preset_store_for_detach = preset_store.clone();
        let asset_store_for_detach = asset_store.clone();
        let session_store_for_detach = session_store.clone();
        let tray_controller_for_detach = tray_controller.clone();
        let options_for_detach = options.clone();
        let toast_overlay_for_detach = toast_overlay.clone();

        *detach_tab.borrow_mut() = Some(Box::new(move |tab_id| {
            if let Some(select) = select_for_detach.borrow().as_ref() {
                select(tab_id);
            }
            let Some(payload) = detach_workspace_tab(
                window_id,
                &app_for_detach,
                &tab_view_for_detach,
                &tabs_for_detach,
                &active_for_detach,
                &select_for_detach,
                &add_for_detach,
                refresh_for_detach.as_ref(),
                &preference_store_for_detach,
                &session_store_for_detach,
                tab_id,
            ) else {
                return;
            };
            show_toast(
                &toast_overlay_for_detach,
                "Workspace detached to a new window",
            );
            present_detached_workspace_window(
                &app_for_detach,
                payload,
                &preference_store_for_detach,
                &preset_store_for_detach,
                &asset_store_for_detach,
                &session_store_for_detach,
                &tray_controller_for_detach,
                options_for_detach.clone(),
            );
        }));
    }

    {
        let tabs_for_toggle = tabs.clone();
        let active_for_toggle = active_tab_id.clone();
        let window_for_toggle = window.clone();
        fullscreen_button.connect_clicked(move |_| {
            toggle_workspace_fullscreen(
                &window_for_toggle,
                &tabs_for_toggle,
                active_for_toggle.get(),
            );
        });
    }

    install_workspace_fullscreen_shortcut(
        &window,
        &fullscreen_shortcut_controller,
        &tabs,
        &active_tab_id,
        current_fullscreen_shortcut.borrow().as_str(),
    );

    install_workspace_density_shortcut(
        &window,
        &density_shortcut_controller,
        &tabs,
        &active_tab_id,
        &session_persistence,
        current_density_shortcut.borrow().as_str(),
    );

    install_workspace_zoom_in_shortcut(
        &window,
        &zoom_in_shortcut_controller,
        &tabs,
        &active_tab_id,
        &session_persistence,
        current_zoom_in_shortcut.borrow().as_str(),
    );

    install_workspace_zoom_out_shortcut(
        &window,
        &zoom_out_shortcut_controller,
        &tabs,
        &active_tab_id,
        &session_persistence,
        current_zoom_out_shortcut.borrow().as_str(),
    );

    {
        let window_for_notify = window.clone();
        let title_root_for_notify = title.root.clone();
        let fullscreen_for_notify = fullscreen_button.clone();
        let tabs_for_notify = tabs.clone();
        let active_for_notify = active_tab_id.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        window.connect_fullscreened_notify(move |window| {
            let is_workspace = active_tab_is_workspace(&tabs_for_notify, active_for_notify.get());
            sync_fullscreen_chrome(
                &window_for_notify,
                title_root_for_notify.upcast_ref(),
                &fullscreen_for_notify,
                is_workspace,
                current_fullscreen_shortcut.borrow().as_str(),
            );
            if !is_workspace && window.is_fullscreen() {
                window.set_fullscreened(false);
            }
        });
    }

    {
        let tabs_for_add = tabs.clone();
        let next_tab_id = next_tab_id.clone();
        let tab_view_for_add = tab_view.clone();
        let window_for_add = window.clone();
        let preference_store = preference_store.clone();
        let preset_store = preset_store.clone();
        let asset_store = asset_store.clone();
        let show_workspace_handle = show_workspace_in_tab.clone();
        let close_tab_for_add = close_tab.clone();
        let refresh_handle = refresh_launch_tabs.clone();
        let select_for_add = select_tab.clone();
        let refresh_tab_strip_for_add = refresh_tab_strip.clone();

        *add_workspace_tab.borrow_mut() = Some(Box::new(move || {
            let tab_id = next_tab_id.get();
            next_tab_id.set(tab_id + 1);

            let launch_title = format!("Workspace {}", tab_id);
            let page_shell = build_tab_page_shell();
            tabs_for_add.borrow_mut().push(WorkspaceTab {
                id: tab_id,
                default_title: launch_title,
                custom_title: None,
                subtitle: "Launch deck".into(),
                page_shell: page_shell.clone(),
                content: TabContent::LaunchDeck,
                workspace_root: None,
            });
            let tab = {
                let tabs = tabs_for_add.borrow();
                tabs.iter()
                    .find(|tab| tab.id == tab_id)
                    .cloned()
                    .expect("new launch tab should exist")
            };
            tab_view_for_add.append(&page_shell);
            sync_tab_page_metadata(&tab_view_for_add, &tab);
            refresh_tab_strip_for_add();

            rebuild_launch_tab(
                tab_id,
                &LaunchTabContext {
                    tabs: tabs_for_add.clone(),
                    window: window_for_add.clone(),
                    preference_store: preference_store.clone(),
                    preset_store: preset_store.clone(),
                    asset_store: asset_store.clone(),
                    show_workspace_handle: show_workspace_handle.clone(),
                    close_tab_handle: close_tab_for_add.clone(),
                    refresh_launch_tabs: refresh_handle.clone(),
                },
            );

            logging::info(format!("created workspace launch tab {}", tab_id));

            if let Some(select) = select_for_add.borrow().as_ref() {
                select(tab_id);
            }
        }));
    }

    {
        let add_for_button = add_workspace_tab.clone();
        title.add_button.connect_clicked(move |_| {
            if let Some(add_tab) = add_for_button.borrow().as_ref() {
                add_tab();
            }
        });
    }

    let open_settings_dialog: Rc<dyn Fn()> = {
        let window_for_settings = window.clone();
        let preference_store_for_settings = preference_store.clone();
        let preset_store_for_settings = preset_store.clone();
        let refresh_for_settings = refresh_launch_tabs.clone();
        let toast_overlay_for_settings = toast_overlay.clone();
        let tabs_for_settings = tabs.clone();
        let active_for_settings = active_tab_id.clone();
        let title_root_for_settings = title.root.clone();
        let fullscreen_button_for_settings = fullscreen_button.clone();
        let fullscreen_shortcut_controller = fullscreen_shortcut_controller.clone();
        let density_shortcut_controller = density_shortcut_controller.clone();
        let zoom_in_shortcut_controller = zoom_in_shortcut_controller.clone();
        let zoom_out_shortcut_controller = zoom_out_shortcut_controller.clone();
        let command_palette_shortcut_controller = command_palette_shortcut_controller.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let current_command_palette_shortcut = current_command_palette_shortcut.clone();
        let sync_close_to_background_notice = sync_close_to_background_notice.clone();
        let tray_controller = tray_controller.clone();
        let options_for_settings = options.clone();
        let voice_transcriber_for_settings = voice_transcriber.clone();
        let voice_event_tx_for_settings = voice_event_tx.clone();
        let voice_warm_state_for_settings = voice_warm_state.clone();
        let voice_warm_generation_for_settings = voice_warm_generation.clone();
        let voice_warm_error_for_settings = voice_warm_error.clone();
        let session_persistence_for_settings = session_persistence.clone();

        Rc::new(move || {
            let preferences = preference_store_for_settings.load();
            settings_dialog::present(
                &window_for_settings,
                settings_dialog::SettingsDialogInput {
                    default_theme: preferences.default_theme,
                    default_density: preferences.default_density,
                    close_to_background: preferences.close_to_background,
                    workspace_fullscreen_shortcut: preferences.workspace_fullscreen_shortcut,
                    workspace_density_shortcut: preferences.workspace_density_shortcut,
                    workspace_zoom_in_shortcut: preferences.workspace_zoom_in_shortcut,
                    workspace_zoom_out_shortcut: preferences.workspace_zoom_out_shortcut,
                    command_palette_shortcut: preferences.command_palette_shortcut,
                    settings_dialog_width: preferences.settings_dialog_width,
                    settings_dialog_height: preferences.settings_dialog_height,
                    max_reconnect_attempts: preferences.max_reconnect_attempts,
                    voice: preferences.voice.clone(),
                    microphone_devices: AudioCapture::enumerate_microphones().unwrap_or_default(),
                    product_display_name: options_for_settings.product.display_name.clone(),
                    settings_title: options_for_settings.product.settings_title.clone(),
                    settings_summary: options_for_settings.product.settings_summary.clone(),
                },
                settings_dialog::SettingsDialogActions {
                    on_theme_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let refresh_handle = refresh_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        move |theme| {
                            preference_store.save_default_theme(theme);
                            logging::info(format!(
                                "updated application settings default_theme={}",
                                theme.label()
                            ));
                            if let Some(refresh) = refresh_handle.borrow().as_ref() {
                                refresh();
                            }
                            show_toast(
                                &toast_overlay,
                                &format!("Default theme set to {}", theme.label()),
                            );
                        }
                    }),
                    on_density_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let refresh_handle = refresh_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        move |density| {
                            preference_store.save_default_density(density);
                            logging::info(format!(
                                "updated application settings default_density={}",
                                density.label()
                            ));
                            if let Some(refresh) = refresh_handle.borrow().as_ref() {
                                refresh();
                            }
                            show_toast(
                                &toast_overlay,
                                &format!("Default density set to {}", density.label()),
                            );
                        }
                    }),
                    on_close_to_background_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let current_close_to_background = current_close_to_background.clone();
                        let sync_close_to_background_notice =
                            sync_close_to_background_notice.clone();
                        let tray_controller = tray_controller.clone();
                        move |close_to_background| {
                            preference_store.save_close_to_background(close_to_background);
                            current_close_to_background.set(close_to_background);
                            sync_close_to_background_notice();
                            logging::info(format!(
                                "updated application settings close_to_background={}",
                                close_to_background
                            ));
                            show_toast(
                                &toast_overlay,
                                if close_to_background {
                                    if tray_controller.is_available() {
                                        "Close button now hides TerminalTiler to the background"
                                    } else {
                                        "Close-to-background is enabled, but no tray watcher is available right now. Closing will still quit normally"
                                    }
                                } else {
                                    "Close button now quits TerminalTiler"
                                },
                            );
                        }
                    }),
                    on_fullscreen_shortcut_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let tabs = tabs_for_settings.clone();
                        let active_tab_id = active_for_settings.clone();
                        let title_root = title_root_for_settings.clone();
                        let fullscreen_button = fullscreen_button_for_settings.clone();
                        let window = window_for_settings.clone();
                        let controller_handle = fullscreen_shortcut_controller.clone();
                        let current_shortcut = current_fullscreen_shortcut.clone();
                        move |shortcut| {
                            preference_store.save_workspace_fullscreen_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_fullscreen_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
                                &shortcut,
                            );
                            sync_fullscreen_chrome(
                                &window,
                                title_root.upcast_ref(),
                                &fullscreen_button,
                                active_tab_is_workspace(&tabs, active_tab_id.get()),
                                current_shortcut.borrow().as_str(),
                            );
                            logging::info(format!(
                                "updated application settings workspace_fullscreen_shortcut={}",
                                shortcut
                            ));
                            show_toast(
                                &toast_overlay,
                                &format!(
                                    "Fullscreen shortcut set to {}",
                                    shortcut_display_label(
                                        &window,
                                        &shortcut,
                                        DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT,
                                    )
                                ),
                            );
                        }
                    }),
                    on_density_shortcut_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let tabs = tabs_for_settings.clone();
                        let active_tab_id = active_for_settings.clone();
                        let window = window_for_settings.clone();
                        let controller_handle = density_shortcut_controller.clone();
                        let current_shortcut = current_density_shortcut.clone();
                        let session_persistence = session_persistence_for_settings.clone();
                        move |shortcut| {
                            preference_store.save_workspace_density_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_density_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &shortcut,
                            );
                            logging::info(format!(
                                "updated application settings workspace_density_shortcut={}",
                                shortcut
                            ));
                            show_toast(
                                &toast_overlay,
                                &format!(
                                    "Density shortcut set to {}",
                                    shortcut_display_label(
                                        &window,
                                        &shortcut,
                                        DEFAULT_WORKSPACE_DENSITY_SHORTCUT,
                                    )
                                ),
                            );
                        }
                    }),
                    on_zoom_in_shortcut_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let tabs = tabs_for_settings.clone();
                        let active_tab_id = active_for_settings.clone();
                        let window = window_for_settings.clone();
                        let controller_handle = zoom_in_shortcut_controller.clone();
                        let current_shortcut = current_zoom_in_shortcut.clone();
                        let session_persistence = session_persistence_for_settings.clone();
                        move |shortcut| {
                            preference_store.save_workspace_zoom_in_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_zoom_in_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &shortcut,
                            );
                            logging::info(format!(
                                "updated application settings workspace_zoom_in_shortcut={}",
                                shortcut
                            ));
                            show_toast(
                                &toast_overlay,
                                &format!(
                                    "Zoom in shortcut set to {}",
                                    shortcut_display_label(
                                        &window,
                                        &shortcut,
                                        DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT,
                                    )
                                ),
                            );
                        }
                    }),
                    on_zoom_out_shortcut_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let tabs = tabs_for_settings.clone();
                        let active_tab_id = active_for_settings.clone();
                        let window = window_for_settings.clone();
                        let controller_handle = zoom_out_shortcut_controller.clone();
                        let current_shortcut = current_zoom_out_shortcut.clone();
                        let session_persistence = session_persistence_for_settings.clone();
                        move |shortcut| {
                            preference_store.save_workspace_zoom_out_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_zoom_out_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &shortcut,
                            );
                            logging::info(format!(
                                "updated application settings workspace_zoom_out_shortcut={}",
                                shortcut
                            ));
                            show_toast(
                                &toast_overlay,
                                &format!(
                                    "Zoom out shortcut set to {}",
                                    shortcut_display_label(
                                        &window,
                                        &shortcut,
                                        DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT,
                                    )
                                ),
                            );
                        }
                    }),
                    on_command_palette_shortcut_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let window = window_for_settings.clone();
                        let controller_handle = command_palette_shortcut_controller.clone();
                        let current_shortcut = current_command_palette_shortcut.clone();
                        move |shortcut| {
                            preference_store.save_command_palette_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_command_palette_shortcut(
                                &window,
                                &controller_handle,
                                &shortcut,
                                Rc::new({
                                    let window = window.clone();
                                    move || {
                                        gio::prelude::ActionGroupExt::activate_action(
                                            &window,
                                            "win.open-command-palette",
                                            None,
                                        );
                                    }
                                }),
                            );
                            logging::info(format!(
                                "updated application settings command_palette_shortcut={}",
                                shortcut
                            ));
                            show_toast(
                                &toast_overlay,
                                &format!(
                                    "Command palette shortcut set to {}",
                                    shortcut_display_label(
                                        &window,
                                        &shortcut,
                                        DEFAULT_COMMAND_PALETTE_SHORTCUT,
                                    )
                                ),
                            );
                        }
                    }),
                    on_max_reconnect_attempts_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        move |attempts| {
                            preference_store.save_max_reconnect_attempts(attempts);
                            logging::info(format!(
                                "updated application settings max_reconnect_attempts={}",
                                attempts
                            ));
                        }
                    }),
                    on_voice_preferences_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let voice_transcriber = voice_transcriber_for_settings.clone();
                        let voice_event_tx = voice_event_tx_for_settings.clone();
                        let voice_warm_state = voice_warm_state_for_settings.clone();
                        let voice_warm_generation = voice_warm_generation_for_settings.clone();
                        let voice_warm_error = voice_warm_error_for_settings.clone();
                        move |voice| {
                            let previous_voice = preference_store.load().voice;
                            preference_store.save_voice_preferences(voice.clone());
                            if !voice.enabled || previous_voice.engine_mode != voice.engine_mode {
                                voice_transcriber.reset();
                                reset_voice_warm_tracking(
                                    &voice_warm_state,
                                    &voice_warm_generation,
                                    &voice_warm_error,
                                );
                            }
                            if voice.enabled {
                                warm_voice_engine_if_ready(
                                    &preference_store,
                                    &voice_transcriber,
                                    &voice_event_tx,
                                    &voice_warm_state,
                                    &voice_warm_generation,
                                    &voice_warm_error,
                                );
                            }
                            logging::info(format!(
                                "updated application settings voice enabled={} mode={} engine={} global_hotkey={}",
                                voice.enabled,
                                voice.activation_mode.label(),
                                voice.engine_mode.label(),
                                voice.prefer_global_hotkey
                            ));
                            show_toast(
                                &toast_overlay,
                                if voice.enabled {
                                    "Voice input settings updated"
                                } else {
                                    "Voice input disabled"
                                },
                            );
                        }
                    }),
                    voice_pack_status_provider: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        move || preference_store.load().voice.pack_status
                    }),
                    on_voice_pack_install_requested: Rc::new({
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let preference_store = preference_store_for_settings.as_ref().clone();
                        let voice_event_tx = voice_event_tx_for_settings.clone();
                        let voice_transcriber = voice_transcriber_for_settings.clone();
                        let voice_warm_state = voice_warm_state_for_settings.clone();
                        let voice_warm_generation = voice_warm_generation_for_settings.clone();
                        let voice_warm_error = voice_warm_error_for_settings.clone();
                        move || {
                            voice_transcriber.reset();
                            reset_voice_warm_tracking(
                                &voice_warm_state,
                                &voice_warm_generation,
                                &voice_warm_error,
                            );
                            let Some(root) = pack::default_voice_pack_dir() else {
                                show_toast(
                                    &toast_overlay,
                                    "Could not resolve application data directory",
                                );
                                return;
                            };
                            show_toast(&toast_overlay, "Installing NVIDIA Parakeet voice pack…");
                            let mut preferences = preference_store.load();
                            preferences.voice.pack_status =
                                VoicePackStatus::Downloading { percent: 1 };
                            preference_store.save(&preferences);
                            let preference_store = preference_store.clone();
                            let voice_event_tx = voice_event_tx.clone();
                            std::thread::spawn(move || {
                                match pack::install_builtin_parakeet_pack(&root) {
                                    Ok(manifest) => {
                                        save_voice_pack_download_progress(&preference_store, 40);
                                        match pack::prepare_python_environment_with_progress(
                                            &root,
                                            &manifest,
                                            |percent| {
                                                save_voice_pack_download_progress(
                                                    &preference_store,
                                                    percent,
                                                );
                                            },
                                        ) {
                                            Ok(_) => {
                                                let engine_mode =
                                                    preference_store.load().voice.engine_mode;
                                                save_voice_pack_download_progress(
                                                    &preference_store,
                                                    80,
                                                );
                                                match pack::health_check(&root, &manifest) {
                                                    health @ VoicePackHealth::Ready { .. } => {
                                                        let (progress_stop, progress_thread) =
                                                            start_voice_pack_progress_heartbeat(
                                                                preference_store.clone(),
                                                                81,
                                                                96,
                                                            );
                                                        let health_event =
                                                            engine::run_voice_engine_health_check(
                                                                &manifest,
                                                                health,
                                                                engine_mode,
                                                            );
                                                        progress_stop
                                                            .store(true, Ordering::Relaxed);
                                                        let _ = progress_thread.join();
                                                        match health_event {
                                                            Ok(VoiceEngineEvent::Health {
                                                                ok: true,
                                                                detail,
                                                            }) => {
                                                                let mut preferences =
                                                                    preference_store.load();
                                                                preferences.voice.pack_status =
                                                                    VoicePackStatus::Installed {
                                                                        version: manifest
                                                                            .version
                                                                            .clone(),
                                                                    };
                                                                preference_store.save(&preferences);
                                                                logging::info(format!(
                                                                    "installed bundled NVIDIA Parakeet voice pack id={} version={} root={} health={}",
                                                                    manifest.id,
                                                                    manifest.version,
                                                                    root.display(),
                                                                    detail
                                                                ));
                                                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                                                    "NVIDIA Parakeet voice pack installed; warming model in the background".into(),
                                                                ));
                                                                let _ = voice_event_tx.send(
                                                                    VoiceUiEvent::WarmRequested,
                                                                );
                                                            }
                                                            Ok(VoiceEngineEvent::Health {
                                                                detail,
                                                                ..
                                                            })
                                                            | Ok(VoiceEngineEvent::Error(detail)) =>
                                                            {
                                                                let mut preferences =
                                                                    preference_store.load();
                                                                preferences.voice.pack_status =
                                                                    VoicePackStatus::Error {
                                                                        message: detail.clone(),
                                                                    };
                                                                preference_store.save(&preferences);
                                                                logging::error(format!(
                                                                    "NVIDIA Parakeet voice pack installed but runtime health failed: {detail}"
                                                                ));
                                                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                                                    "Voice pack installed, but Parakeet verification failed".into(),
                                                                ));
                                                            }
                                                            Ok(other) => {
                                                                let mut preferences =
                                                                    preference_store.load();
                                                                preferences.voice.pack_status =
                                                                    VoicePackStatus::Error {
                                                                        message: format!(
                                                                            "inconclusive health check: {other:?}"
                                                                        ),
                                                                    };
                                                                preference_store.save(&preferences);
                                                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                                                    "Voice pack installed, but health check was inconclusive".into(),
                                                                ));
                                                            }
                                                            Err(error) => {
                                                                let mut preferences =
                                                                    preference_store.load();
                                                                preferences.voice.pack_status =
                                                                    VoicePackStatus::Error {
                                                                        message: error.to_string(),
                                                                    };
                                                                preference_store.save(&preferences);
                                                                logging::error(format!(
                                                                    "failed to verify NVIDIA Parakeet voice pack: {error}"
                                                                ));
                                                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                                                    "Voice pack installed, but verification failed".into(),
                                                                ));
                                                            }
                                                        }
                                                    }
                                                    VoicePackHealth::Missing
                                                    | VoicePackHealth::Broken(_) => {
                                                        let mut preferences =
                                                            preference_store.load();
                                                        preferences.voice.pack_status =
                                                            VoicePackStatus::Error {
                                                                message: "voice pack files are incomplete after install".into(),
                                                            };
                                                        preference_store.save(&preferences);
                                                        let _ = voice_event_tx
                                                            .send(VoiceUiEvent::Toast(
                                                            "Voice pack installation is incomplete"
                                                                .into(),
                                                        ));
                                                    }
                                                }
                                            }
                                            Err(error) => {
                                                let mut preferences = preference_store.load();
                                                preferences.voice.pack_status =
                                                    VoicePackStatus::Error {
                                                        message: error.to_string(),
                                                    };
                                                preference_store.save(&preferences);
                                                logging::error(format!(
                                                    "failed to prepare NVIDIA Parakeet Python environment: {error:?}"
                                                ));
                                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                                    "Voice pack installed, but Python dependencies failed".into(),
                                                ));
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        let mut preferences = preference_store.load();
                                        preferences.voice.pack_status = VoicePackStatus::Error {
                                            message: error.to_string(),
                                        };
                                        preference_store.save(&preferences);
                                        logging::error(format!(
                                            "failed to install bundled NVIDIA Parakeet voice pack: {error:?}"
                                        ));
                                        let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                            "Failed to install NVIDIA Parakeet voice pack".into(),
                                        ));
                                    }
                                }
                            });
                        }
                    }),
                    on_voice_pack_delete_requested: Rc::new({
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let preference_store = preference_store_for_settings.as_ref().clone();
                        let voice_event_tx = voice_event_tx_for_settings.clone();
                        let voice_transcriber = voice_transcriber_for_settings.clone();
                        let voice_warm_state = voice_warm_state_for_settings.clone();
                        let voice_warm_generation = voice_warm_generation_for_settings.clone();
                        let voice_warm_error = voice_warm_error_for_settings.clone();
                        move || {
                            voice_transcriber.reset();
                            reset_voice_warm_tracking(
                                &voice_warm_state,
                                &voice_warm_generation,
                                &voice_warm_error,
                            );
                            let manifest = pack::builtin_parakeet_manifest();
                            let Some(root) = pack::default_voice_pack_dir() else {
                                show_toast(
                                    &toast_overlay,
                                    "Could not resolve application data directory",
                                );
                                return;
                            };
                            show_toast(&toast_overlay, "Deleting NVIDIA Parakeet voice pack…");
                            let preference_store = preference_store.clone();
                            let voice_event_tx = voice_event_tx.clone();
                            std::thread::spawn(move || match pack::delete_pack(&root, &manifest) {
                                Ok(_) => {
                                    let mut preferences = preference_store.load();
                                    preferences.voice.pack_status = VoicePackStatus::NotInstalled;
                                    preference_store.save(&preferences);
                                    logging::info(format!(
                                        "deleted NVIDIA Parakeet voice pack id={} version={} root={}",
                                        manifest.id,
                                        manifest.version,
                                        root.display()
                                    ));
                                    let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                        "NVIDIA Parakeet voice pack deleted".into(),
                                    ));
                                }
                                Err(error) => {
                                    logging::error(format!(
                                        "failed to delete NVIDIA Parakeet voice pack: {error:?}"
                                    ));
                                    let _ = voice_event_tx.send(VoiceUiEvent::Toast(
                                        "Failed to delete NVIDIA Parakeet voice pack".into(),
                                    ));
                                }
                            });
                        }
                    }),
                    on_voice_pack_health_check_requested: Rc::new({
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let preference_store = preference_store_for_settings.as_ref().clone();
                        let voice_event_tx = voice_event_tx_for_settings.clone();
                        move || {
                            let manifest = pack::builtin_parakeet_manifest();
                            let Some(root) = pack::default_voice_pack_dir() else {
                                show_toast(
                                    &toast_overlay,
                                    "Could not resolve application data directory",
                                );
                                return;
                            };
                            show_toast(&toast_overlay, "Checking NVIDIA Parakeet runtime…");
                            let preference_store = preference_store.clone();
                            let voice_event_tx = voice_event_tx.clone();
                            std::thread::spawn(move || {
                                let toast = match refresh_builtin_voice_pack_assets_for_runtime(
                                    &root,
                                ) {
                                    Ok(()) => match pack::health_check(&root, &manifest) {
                                        health @ VoicePackHealth::Ready { .. } => {
                                            let engine_mode =
                                                preference_store.load().voice.engine_mode;
                                            match engine::run_voice_engine_health_check(
                                                &manifest,
                                                health,
                                                engine_mode,
                                            ) {
                                                Ok(VoiceEngineEvent::Health { ok, detail })
                                                    if ok =>
                                                {
                                                    logging::info(format!(
                                                        "NVIDIA Parakeet runtime health check passed id={} version={} root={} detail={}",
                                                        manifest.id,
                                                        manifest.version,
                                                        root.display(),
                                                        detail
                                                    ));
                                                    "NVIDIA Parakeet runtime is healthy".to_string()
                                                }
                                                Ok(VoiceEngineEvent::Health { detail, .. })
                                                | Ok(VoiceEngineEvent::Error(detail)) => {
                                                    logging::error(format!(
                                                        "NVIDIA Parakeet runtime health check failed: {detail}"
                                                    ));
                                                    "NVIDIA Parakeet runtime dependencies are missing"
                                                        .to_string()
                                                }
                                                Ok(other) => {
                                                    logging::error(format!(
                                                        "unexpected NVIDIA Parakeet health event: {other:?}"
                                                    ));
                                                    "NVIDIA Parakeet health check was inconclusive"
                                                        .to_string()
                                                }
                                                Err(error) => {
                                                    logging::error(format!(
                                                        "failed to run NVIDIA Parakeet runtime health check: {error}"
                                                    ));
                                                    "Failed to run NVIDIA Parakeet health check"
                                                        .to_string()
                                                }
                                            }
                                        }
                                        VoicePackHealth::Missing => {
                                            "NVIDIA Parakeet voice pack is not installed"
                                                .to_string()
                                        }
                                        VoicePackHealth::Broken(message) => {
                                            logging::error(format!(
                                                "NVIDIA Parakeet voice pack health check failed: {message}"
                                            ));
                                            "NVIDIA Parakeet voice pack is incomplete".to_string()
                                        }
                                    },
                                    Err(detail) => {
                                        logging::error(format!(
                                            "NVIDIA Parakeet voice pack refresh failed before health check: {detail}"
                                        ));
                                        "NVIDIA Parakeet voice pack refresh failed".to_string()
                                    }
                                };
                                let _ = voice_event_tx.send(VoiceUiEvent::Toast(toast));
                            });
                        }
                    }),
                    on_open_logs_folder: Rc::new({
                        let toast_overlay = toast_overlay_for_settings.clone();
                        move || match logging::ensure_log_directory() {
                            Ok(path) => {
                                let uri = gio::File::for_path(&path).uri();
                                match gio::AppInfo::launch_default_for_uri(
                                    uri.as_str(),
                                    None::<&gio::AppLaunchContext>,
                                ) {
                                    Ok(()) => {
                                        logging::info(format!(
                                            "opened application logs folder {}",
                                            path.display()
                                        ));
                                        show_toast(&toast_overlay, "Opened logs folder");
                                    }
                                    Err(error) => {
                                        logging::error(format!(
                                            "failed to open application logs folder '{}': {}",
                                            path.display(),
                                            error
                                        ));
                                        show_toast(&toast_overlay, "Failed to open logs folder");
                                    }
                                }
                            }
                            Err(error) => {
                                logging::error(format!(
                                    "failed to prepare application logs folder: {}",
                                    error
                                ));
                                show_toast(&toast_overlay, "Could not resolve logs folder");
                            }
                        }
                    }),
                    on_reset_defaults: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        let refresh_handle = refresh_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        let tabs = tabs_for_settings.clone();
                        let active_tab_id = active_for_settings.clone();
                        let title_root = title_root_for_settings.clone();
                        let fullscreen_button = fullscreen_button_for_settings.clone();
                        let window = window_for_settings.clone();
                        let fullscreen_controller = fullscreen_shortcut_controller.clone();
                        let density_controller = density_shortcut_controller.clone();
                        let zoom_in_controller = zoom_in_shortcut_controller.clone();
                        let zoom_out_controller = zoom_out_shortcut_controller.clone();
                        let command_palette_controller =
                            command_palette_shortcut_controller.clone();
                        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
                        let current_density_shortcut = current_density_shortcut.clone();
                        let current_close_to_background = current_close_to_background.clone();
                        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
                        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
                        let current_command_palette_shortcut =
                            current_command_palette_shortcut.clone();
                        let sync_close_to_background_notice =
                            sync_close_to_background_notice.clone();
                        let session_persistence = session_persistence_for_settings.clone();
                        move || {
                            let defaults = AppPreferences::default();
                            preference_store.save(&defaults);
                            current_fullscreen_shortcut
                                .replace(defaults.workspace_fullscreen_shortcut.clone());
                            current_density_shortcut
                                .replace(defaults.workspace_density_shortcut.clone());
                            current_close_to_background.set(defaults.close_to_background);
                            sync_close_to_background_notice();
                            current_zoom_in_shortcut
                                .replace(defaults.workspace_zoom_in_shortcut.clone());
                            current_zoom_out_shortcut
                                .replace(defaults.workspace_zoom_out_shortcut.clone());
                            current_command_palette_shortcut
                                .replace(defaults.command_palette_shortcut.clone());
                            install_workspace_fullscreen_shortcut(
                                &window,
                                &fullscreen_controller,
                                &tabs,
                                &active_tab_id,
                                &defaults.workspace_fullscreen_shortcut,
                            );
                            install_workspace_density_shortcut(
                                &window,
                                &density_controller,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &defaults.workspace_density_shortcut,
                            );
                            install_workspace_zoom_in_shortcut(
                                &window,
                                &zoom_in_controller,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &defaults.workspace_zoom_in_shortcut,
                            );
                            install_workspace_zoom_out_shortcut(
                                &window,
                                &zoom_out_controller,
                                &tabs,
                                &active_tab_id,
                                &session_persistence,
                                &defaults.workspace_zoom_out_shortcut,
                            );
                            install_command_palette_shortcut(
                                &window,
                                &command_palette_controller,
                                &defaults.command_palette_shortcut,
                                Rc::new({
                                    let window = window.clone();
                                    move || {
                                        gio::prelude::ActionGroupExt::activate_action(
                                            &window,
                                            "win.open-command-palette",
                                            None,
                                        );
                                    }
                                }),
                            );
                            sync_fullscreen_chrome(
                                &window,
                                title_root.upcast_ref(),
                                &fullscreen_button,
                                active_tab_is_workspace(&tabs, active_tab_id.get()),
                                current_fullscreen_shortcut.borrow().as_str(),
                            );
                            logging::info("reset application settings to defaults");
                            if let Some(refresh) = refresh_handle.borrow().as_ref() {
                                refresh();
                            }
                            show_toast(&toast_overlay, "Application defaults reset");
                        }
                    }),
                    on_reset_builtin_presets: Rc::new({
                        let preset_store = preset_store_for_settings.clone();
                        let refresh_handle = refresh_for_settings.clone();
                        let toast_overlay = toast_overlay_for_settings.clone();
                        move || match preset_store.reset_builtin_presets() {
                            Ok(()) => {
                                logging::info("reset builtin saved presets to factory defaults");
                                if let Some(refresh) = refresh_handle.borrow().as_ref() {
                                    refresh();
                                }
                                show_toast(&toast_overlay, "Default saved presets restored");
                            }
                            Err(error) => {
                                logging::error(format!(
                                    "failed to reset builtin saved presets: {}",
                                    error
                                ));
                                show_toast(
                                    &toast_overlay,
                                    "Failed to restore default saved presets",
                                );
                            }
                        }
                    }),
                    on_size_changed: Rc::new({
                        let preference_store = preference_store_for_settings.clone();
                        move |width, height| {
                            preference_store.save_settings_dialog_size(width, height);
                        }
                    }),
                },
            );
        })
    };

    {
        let open_settings_dialog = open_settings_dialog.clone();
        settings_button.connect_clicked(move |_| open_settings_dialog());
    }

    let open_assets_manager: Rc<dyn Fn()> = {
        let window = window.clone();
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let asset_store = asset_store.clone();
        let refresh_launch_tabs = refresh_launch_tabs.clone();
        Rc::new(move || {
            let workspace_root = tabs
                .borrow()
                .iter()
                .find(|tab| tab.id == active_tab_id.get())
                .and_then(|tab| tab.workspace_root.clone())
                .or_else(|| std::env::current_dir().ok());
            let tabs_for_saved = tabs.clone();
            let asset_store_for_saved = asset_store.clone();
            let refresh_launch_tabs = refresh_launch_tabs.clone();
            assets_manager::present(
                &window,
                asset_store.clone(),
                workspace_root,
                Rc::new(move || {
                    {
                        let mut tabs = tabs_for_saved.borrow_mut();
                        for tab in tabs.iter_mut() {
                            let TabContent::Workspace(workspace) = &mut tab.content else {
                                continue;
                            };
                            let Some(workspace_root) = tab.workspace_root.as_ref() else {
                                continue;
                            };
                            let assets = asset_store_for_saved
                                .load_assets_for_workspace_root(workspace_root)
                                .assets;
                            workspace.assets = assets.clone();
                            workspace.runtime.update_assets(assets);
                        }
                    }
                    if let Some(refresh) = refresh_launch_tabs.borrow().as_ref() {
                        refresh();
                    }
                }),
            );
        })
    };

    {
        let open_assets_manager = open_assets_manager.clone();
        assets_button.connect_clicked(move |_| open_assets_manager());
    }

    let open_companion_dialog: Option<Rc<dyn Fn()>> = options.companion.as_ref().map(|companion| {
        let window = window.clone();
        let companion = companion.clone();
        Rc::new(move || companion_dialog::present(&window, companion.clone())) as Rc<dyn Fn()>
    });

    if let (Some(button), Some(open_companion_dialog)) =
        (companion_button.as_ref(), open_companion_dialog.as_ref())
    {
        let open_companion_dialog = open_companion_dialog.clone();
        button.connect_clicked(move |_| open_companion_dialog());
    }

    let open_command_palette: Rc<dyn Fn()> = {
        let window = window.clone();
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let add_workspace_tab = add_workspace_tab.clone();
        let select_tab = select_tab.clone();
        let request_tab_rename = request_tab_rename.clone();
        let open_settings_dialog = open_settings_dialog.clone();
        let open_assets_manager = open_assets_manager.clone();
        let open_companion_dialog = open_companion_dialog.clone();
        let open_about_dialog: Rc<dyn Fn()> = {
            let window = window.clone();
            Rc::new({
                let options = options.clone();
                move || about_dialog::present(&window, &options.product)
            })
        };
        Rc::new(move || {
            let snapshot = tabs.borrow().clone();
            let active_id = active_tab_id.get();
            let mut actions = command_palette::app_actions(command_palette::AppActionCallbacks {
                open_settings: Rc::new({
                    let open_settings_dialog = open_settings_dialog.clone();
                    move || open_settings_dialog()
                }),
                open_assets_manager: Rc::new({
                    let open_assets_manager = open_assets_manager.clone();
                    move || open_assets_manager()
                }),
                open_about: Rc::new({
                    let open_about_dialog = open_about_dialog.clone();
                    move || open_about_dialog()
                }),
                new_tab: Rc::new({
                    let add_workspace_tab = add_workspace_tab.clone();
                    move || {
                        if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
                            add_tab();
                        }
                    }
                }),
                open_companion: open_companion_dialog.as_ref().map(|open_companion_dialog| {
                    Rc::new({
                        let open_companion_dialog = open_companion_dialog.clone();
                        move || open_companion_dialog()
                    }) as Rc<dyn Fn()>
                }),
            });

            for tab in &snapshot {
                let tab_id = tab.id;
                let title = tab_display_title(tab);
                let subtitle = tab.subtitle.clone();
                actions.push(command_palette::PaletteAction {
                    title: format!("Switch to {title}"),
                    subtitle,
                    on_activate: Rc::new({
                        let select_tab = select_tab.clone();
                        move || {
                            if let Some(select) = select_tab.borrow().as_ref() {
                                select(tab_id);
                            }
                        }
                    }),
                });
            }

            if let Some(active_tab) = snapshot.iter().find(|tab| tab.id == active_id) {
                actions.extend(command_palette::active_tab_actions(Rc::new({
                    let request_tab_rename = request_tab_rename.clone();
                    move || {
                        if let Some(rename) = request_tab_rename.borrow().as_ref() {
                            rename(active_id);
                        }
                    }
                })));

                if let TabContent::Workspace(workspace) = &active_tab.content {
                    let runbooks =
                        workspace
                            .assets
                            .runbooks
                            .iter()
                            .filter(|runbook| runbook.variables.is_empty())
                            .map(|runbook| {
                                let runbook_for_action = runbook.clone();
                                let runbook_for_callback = runbook.clone();
                                let runtime = workspace.runtime.clone();
                                command_palette::RunbookAction {
                                    runbook: runbook_for_action,
                                    on_activate: Rc::new(move || {
                                        if let Ok(resolved) =
                                            crate::services::runbooks::resolve_runbook(
                                                &runbook_for_callback,
                                                &HashMap::new(),
                                                &runtime.tile_specs(),
                                            )
                                        {
                                            runtime.run_runbook(&resolved);
                                        }
                                    }),
                                }
                            })
                            .collect();

                    actions.extend(command_palette::workspace_actions(
                        command_palette::WorkspaceActionCallbacks {
                            focus_next_alert: Rc::new({
                                let runtime_for_alert_focus = workspace.runtime.clone();
                                move || {
                                    let alert_store = runtime_for_alert_focus.alert_store();
                                    if let Some(alert) = alert_store
                                        .snapshot()
                                        .into_iter()
                                        .find(|alert| alert.unread && alert.pane_id.is_some())
                                    {
                                        if let Some(pane_id) = alert.pane_id {
                                            runtime_for_alert_focus.focus_tile(&pane_id);
                                        }
                                        alert_store.mark_read(alert.id);
                                    }
                                }
                            }),
                            add_web_tile: Rc::new({
                                let runtime_for_add_web_tile = workspace.runtime.clone();
                                move || {
                                    let _ = runtime_for_add_web_tile.add_web_tile();
                                }
                            }),
                            runbooks,
                        },
                    ));
                }
            }

            command_palette::present(&window, actions);
        })
    };

    {
        let open_settings_dialog = open_settings_dialog.clone();
        close_to_background_notice_button.connect_clicked(move |_| open_settings_dialog());
    }

    {
        let open_settings_dialog = open_settings_dialog.clone();
        let action = gio::SimpleAction::new("open-settings", None);
        action.connect_activate(move |_, _| open_settings_dialog());
        window.add_action(&action);
    }

    {
        let open_assets_manager = open_assets_manager.clone();
        let action = gio::SimpleAction::new("open-assets", None);
        action.connect_activate(move |_, _| open_assets_manager());
        window.add_action(&action);
    }

    {
        let open_command_palette = open_command_palette.clone();
        let action = gio::SimpleAction::new("open-command-palette", None);
        action.connect_activate(move |_, _| open_command_palette());
        window.add_action(&action);
    }

    {
        let window_for_quit_action = window.clone();
        let tabs_for_quit_action = tabs.clone();
        let active_for_quit_action = active_tab_id.clone();
        let session_store_for_quit_action = session_store.clone();
        let tray_controller = tray_controller.clone();
        let quit_requested = quit_requested.clone();
        let force_quit_requested = force_quit_requested.clone();
        let action = gio::SimpleAction::new("quit-app", None);
        action.connect_activate(move |_, _| {
            tray_controller.set_window_hidden(false);
            if has_active_workspace_processes(&tabs_for_quit_action) {
                let window = window_for_quit_action.clone();
                let tabs = tabs_for_quit_action.clone();
                let session_store = session_store_for_quit_action.clone();
                let active_tab_id = active_for_quit_action.clone();
                let force_quit_requested = force_quit_requested.clone();
                confirm_destructive_action(
                    &window_for_quit_action,
                    "Quit Application?",
                    "One or more terminal sessions are still running. Quitting TerminalTiler now will close the application immediately even if those processes are still active.",
                    "Quit Application",
                    move || {
                        force_quit_requested.set(true);
                        force_quit_application(
                            window_id,
                            &window,
                            &tabs,
                            active_tab_id.get(),
                            &session_store,
                        );
                    },
                );
                return;
            }

            quit_requested.set(true);
            window_for_quit_action.close();
        });
        window.add_action(&action);
    }

    install_command_palette_shortcut(
        &window,
        &command_palette_shortcut_controller,
        current_command_palette_shortcut.borrow().as_str(),
        open_command_palette.clone(),
    );

    let attach_workspace_tab = {
        let tab_view_for_attach = tab_view.clone();
        let tabs_for_attach = tabs.clone();
        let next_tab_id_for_attach = next_tab_id.clone();
        let active_for_attach = active_tab_id.clone();
        let select_for_attach = select_tab.clone();
        let refresh_for_attach = refresh_tab_strip.clone();
        let session_store_for_attach = session_store.clone();
        Rc::new(move |tab: WorkspaceTab| {
            attach_workspace_tab_to_main_window(
                window_id,
                &tab_view_for_attach,
                &tabs_for_attach,
                &next_tab_id_for_attach,
                &active_for_attach,
                &select_for_attach,
                refresh_for_attach.as_ref(),
                &session_store_for_attach,
                tab,
            );
        })
    };
    register_linux_main_attach_target(window_id, &window, attach_workspace_tab.clone());

    if let Some(tab) = initial_workspace_tab {
        attach_workspace_tab(tab);
    } else if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
        add_tab();
    }

    let tabs_for_back = tabs.clone();
    let window_for_back = window.clone();
    let preference_store_for_back = preference_store.clone();
    let preset_store_for_back = preset_store.clone();
    let asset_store_for_back = asset_store.clone();
    let show_workspace_for_back = show_workspace_in_tab.clone();
    let close_tab_for_back = close_tab.clone();
    let refresh_for_back = refresh_launch_tabs.clone();
    let select_for_back = select_tab.clone();
    let active_for_back = active_tab_id.clone();
    let session_persistence_for_back = session_persistence.clone();
    back_button.connect_clicked(move |_| {
        let tab_id = active_for_back.get();
        if tab_id == 0 {
            return;
        }

        let is_workspace = {
            let tabs = tabs_for_back.borrow();
            tabs.iter()
                .find(|tab| tab.id == tab_id)
                .map(|tab| matches!(tab.content, TabContent::Workspace(_)))
                .unwrap_or(false)
        };

        let do_return = {
            let tabs_for_back = tabs_for_back.clone();
            let window_for_back = window_for_back.clone();
            let preference_store_for_back = preference_store_for_back.clone();
            let preset_store_for_back = preset_store_for_back.clone();
            let asset_store_for_back = asset_store_for_back.clone();
            let show_workspace_for_back = show_workspace_for_back.clone();
            let close_tab_for_back = close_tab_for_back.clone();
            let refresh_for_back = refresh_for_back.clone();
            let select_for_back = select_for_back.clone();
            let session_persistence_for_back = session_persistence_for_back.clone();

            move || {
                let runtime = {
                    let mut tabs = tabs_for_back.borrow_mut();
                    let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                        return;
                    };
                    let runtime = match &tab.content {
                        TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
                        TabContent::LaunchDeck => None,
                    };
                    tab.subtitle = "Launch deck".into();
                    tab.content = TabContent::LaunchDeck;
                    tab.workspace_root = None;
                    runtime
                };

                logging::info(format!("returning workspace tab {} to launch deck", tab_id));

                if let Some(runtime) = runtime {
                    runtime.terminate_all("returning workspace tab to templates");
                }
                rebuild_launch_tab(
                    tab_id,
                    &LaunchTabContext {
                        tabs: tabs_for_back.clone(),
                        window: window_for_back.clone(),
                        preference_store: preference_store_for_back.clone(),
                        preset_store: preset_store_for_back.clone(),
                        asset_store: asset_store_for_back.clone(),
                        show_workspace_handle: show_workspace_for_back.clone(),
                        close_tab_handle: close_tab_for_back.clone(),
                        refresh_launch_tabs: refresh_for_back.clone(),
                    },
                );

                if let Some(select) = select_for_back.borrow().as_ref() {
                    select(tab_id);
                }
                session_persistence_for_back.save_now("workspace tab returned to launch deck");
            }
        };

        if is_workspace {
            confirm_destructive_action(
                &window_for_back,
                "Return to Templates?",
                "Running terminal sessions in this workspace will be terminated.",
                "Return",
                do_return,
            );
        } else {
            do_return();
        }
    });

    {
        let tabs_for_save = tabs.clone();
        let active_for_save = active_tab_id.clone();
        let session_store = session_store.clone();
        let session_persistence_for_window_close = session_persistence.clone();
        let current_close_to_background = current_close_to_background.clone();
        let quit_requested = quit_requested.clone();
        let force_quit_requested = force_quit_requested.clone();
        let tray_controller = tray_controller.clone();
        let voice_transcriber = voice_transcriber.clone();
        window.connect_close_request(move |window| {
            if force_quit_requested.replace(false) {
                voice_transcriber.shutdown();
                unregister_linux_main_attach_target(window_id);
                return glib::Propagation::Proceed;
            }

            if !quit_requested.replace(false)
                && current_close_to_background.get()
                && tray_controller.is_available()
            {
                logging::info("hiding application window to background");
                tray_controller.set_window_hidden(true);
                window.set_visible(false);
                return glib::Propagation::Stop;
            }

            if has_active_workspace_processes(&tabs_for_save) {
                let window = window.clone();
                let confirm_window = window.clone();
                let tabs = tabs_for_save.clone();
                let session_store = session_store.clone();
                let active_tab_id = active_for_save.clone();
                let force_quit_requested = force_quit_requested.clone();
                confirm_destructive_action(
                    &confirm_window,
                    "Quit Application?",
                    "One or more terminal sessions are still running. Quitting TerminalTiler now will close the application immediately even if those processes are still active.",
                    "Quit Application",
                    move || {
                        force_quit_requested.set(true);
                        force_quit_application(
                            window_id,
                            &window,
                            &tabs,
                            active_tab_id.get(),
                            &session_store,
                        );
                    },
                );
                return glib::Propagation::Stop;
            }

            tray_controller.set_window_hidden(false);
            voice_transcriber.shutdown();
            let runtimes = workspace_runtimes(&tabs_for_save);
            session_persistence_for_window_close.save_now("closing application window");
            unregister_linux_main_attach_target(window_id);

            for runtime in runtimes {
                runtime.terminate_all("closing application window");
            }
            glib::Propagation::Proceed
        });
    }

    window.present();

    warm_voice_engine_if_ready(
        &preference_store,
        &voice_transcriber,
        &voice_event_tx,
        &voice_warm_state,
        &voice_warm_generation,
        &voice_warm_error,
    );

    if dialog_smoke::is_enabled() {
        dialog_smoke::start(&window);
        return;
    }

    if let Some(saved_session) = saved_session {
        let resume_session = saved_session.clone();
        let tabs_for_restore = tabs.clone();
        let next_tab_id_for_restore = next_tab_id.clone();
        let tab_view_for_restore = tab_view.clone();
        let select_for_restore = select_tab.clone();
        let active_for_restore = active_tab_id.clone();
        let session_store_for_restore = session_store.clone();
        let window_for_restore = window.clone();
        let warning = startup_warning.clone();
        let restore_mode = preference_store.load().default_restore_mode;
        let session_persistence_for_restore = session_persistence.clone();
        let startup_restore_suppression_for_restore = startup_restore_suppression.clone();

        glib::idle_add_local_once(move || {
            let restore_context = RestoreSessionContext {
                tabs: tabs_for_restore.clone(),
                next_tab_id: next_tab_id_for_restore.clone(),
                tab_view: tab_view_for_restore.clone(),
                select_tab: select_for_restore.clone(),
                active_tab_id: active_for_restore.clone(),
                forced_tab_closes: forced_tab_closes.clone(),
                suppress_empty_replacement: suppress_empty_replacement.clone(),
                asset_store: asset_store.clone(),
                preference_store: preference_store.clone(),
                session_persistence: session_persistence_for_restore.clone(),
            };
            match restore_mode {
                RestoreLaunchMode::Prompt => prompt_session_resume(
                    &window_for_restore,
                    &saved_session,
                    warning.as_deref(),
                    {
                        let restore_context = restore_context.clone();
                        let resume_session = resume_session.clone();
                        let startup_restore_suppression =
                            startup_restore_suppression_for_restore.clone();
                        move || {
                            startup_restore_suppression.borrow_mut().take();
                            restore_saved_session(&restore_context, resume_session.clone(), true);
                        }
                    },
                    {
                        let restore_context = restore_context.clone();
                        let shell_session = shell_only_session(&resume_session);
                        let startup_restore_suppression =
                            startup_restore_suppression_for_restore.clone();
                        move || {
                            startup_restore_suppression.borrow_mut().take();
                            restore_saved_session(&restore_context, shell_session.clone(), true);
                        }
                    },
                    move || {
                        startup_restore_suppression_for_restore.borrow_mut().take();
                        session_store_for_restore.clear();
                    },
                ),
                RestoreLaunchMode::RerunStartupCommands => {
                    startup_restore_suppression_for_restore.borrow_mut().take();
                    if let Some(session) = session_for_restore_mode(
                        &resume_session,
                        RestoreLaunchMode::RerunStartupCommands,
                    ) {
                        restore_saved_session(&restore_context, session, true);
                    }
                }
                RestoreLaunchMode::ShellOnly => {
                    startup_restore_suppression_for_restore.borrow_mut().take();
                    if let Some(session) =
                        session_for_restore_mode(&resume_session, RestoreLaunchMode::ShellOnly)
                    {
                        restore_saved_session(&restore_context, session, true);
                    }
                }
            }
        });
    } else if let Some(startup_warning) = startup_warning {
        let window_for_notice = window.clone();
        glib::idle_add_local_once(move || {
            show_startup_notice(
                &window_for_notice,
                "Session Restore Warning",
                &startup_warning,
            );
        });
    }
}

fn tab_display_title(tab: &WorkspaceTab) -> String {
    tab.custom_title
        .clone()
        .unwrap_or_else(|| match &tab.content {
            TabContent::LaunchDeck => tab.default_title.clone(),
            TabContent::Workspace(workspace) => workspace.preset.name.clone(),
        })
}

fn make_workspace_layout_target(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    tab_id: usize,
) -> WorkspaceLayoutTargetHandle {
    Rc::new(RefCell::new(Some(WorkspaceLayoutTarget {
        tabs: tabs.clone(),
        tab_id,
    })))
}

fn update_workspace_layout_target(
    target: &WorkspaceLayoutTargetHandle,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    tab_id: usize,
) {
    *target.borrow_mut() = Some(WorkspaceLayoutTarget {
        tabs: tabs.clone(),
        tab_id,
    });
}

fn clear_workspace_layout_target(target: &WorkspaceLayoutTargetHandle) {
    *target.borrow_mut() = None;
}

fn apply_workspace_layout_change(
    target: &WorkspaceLayoutTargetHandle,
    next_layout: crate::model::layout::LayoutNode,
) {
    let Some(target) = target.borrow().clone() else {
        return;
    };
    let mut tabs = target.tabs.borrow_mut();
    let Some(tab) = tabs.iter_mut().find(|tab| tab.id == target.tab_id) else {
        return;
    };
    if let TabContent::Workspace(workspace) = &mut tab.content {
        workspace.preset.layout = next_layout;
    }
}

fn rebind_workspace_tab_layout(tab: &WorkspaceTab, tabs: &Rc<RefCell<Vec<WorkspaceTab>>>) {
    if let TabContent::Workspace(workspace) = &tab.content {
        update_workspace_layout_target(&workspace.layout_target, tabs, tab.id);
    }
}

fn clear_workspace_tab_layout_binding(tab: &WorkspaceTab) {
    if let TabContent::Workspace(workspace) = &tab.content {
        clear_workspace_layout_target(&workspace.layout_target);
    }
}

fn move_item_to_position<T>(items: &mut Vec<T>, from_index: usize, position: usize) -> bool {
    if from_index >= items.len() {
        return false;
    }
    let item = items.remove(from_index);
    let insert_index = position.min(items.len());
    items.insert(insert_index, item);
    from_index != insert_index
}

fn move_tab_to_position(tabs: &mut Vec<WorkspaceTab>, moved_id: usize, position: usize) -> bool {
    let Some(from_index) = tabs.iter().position(|tab| tab.id == moved_id) else {
        return false;
    };
    move_item_to_position(tabs, from_index, position)
}

fn build_tab_page_shell() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .build()
}

fn replace_tab_page_content(page_shell: &gtk::Box, widget: &gtk::Widget) {
    while let Some(child) = page_shell.first_child() {
        page_shell.remove(&child);
    }
    page_shell.append(widget);
}

fn tab_page_for_id(
    tab_view: &adw::TabView,
    tabs: &[WorkspaceTab],
    tab_id: usize,
) -> Option<adw::TabPage> {
    tabs.iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab_view.page(&tab.page_shell))
}

fn tab_id_for_page(tabs: &[WorkspaceTab], page: &adw::TabPage) -> Option<usize> {
    let page_child = page.child();
    tabs.iter()
        .find(|tab| tab.page_shell.clone().upcast::<gtk::Widget>() == page_child)
        .map(|tab| tab.id)
}

fn sync_tab_page_metadata(tab_view: &adw::TabView, tab: &WorkspaceTab) {
    let page = tab_view.page(&tab.page_shell);
    let icon = gio::ThemedIcon::new("utilities-terminal-symbolic");
    page.set_title(&tab_display_title(tab));
    page.set_tooltip(&tab.subtitle);
    page.set_icon(Some(&icon));
}

fn suppress_native_tab_drag_icon(source: &gtk::DragSource) {
    let empty_icon = gdk::Paintable::new_empty(1, 1);
    source.set_icon(Some(&empty_icon), 0, 0);
}

fn preview_index_for_pointer(slots: &[(f64, f64)], x: f64) -> usize {
    for (index, (start, width)) in slots.iter().enumerate() {
        if x < *start + (*width / 2.0) {
            return index;
        }
    }
    slots.len()
}

impl TabStripController {
    fn new(
        tabs_box: gtk::Box,
        select_tab: SelectTabHandle,
        close_tab: TabActionHandle,
        request_tab_rename: TabActionHandle,
        detach_tab: TabActionHandle,
        can_detach_tab: TabPredicateHandle,
    ) -> Self {
        Self {
            tabs_box,
            items: Vec::new(),
            order: Vec::new(),
            drag_state: None,
            select_tab,
            close_tab,
            request_tab_rename,
            detach_tab,
            can_detach_tab,
        }
    }

    fn sync(
        &mut self,
        controller: &TabStripControllerHandle,
        tabs: &[WorkspaceTab],
        active_tab_id: usize,
    ) {
        self.order = tabs.iter().map(|tab| tab.id).collect();

        let stale_ids = self
            .items
            .iter()
            .filter(|item| !self.order.contains(&item.tab_id))
            .map(|item| item.tab_id)
            .collect::<Vec<_>>();
        for stale_id in stale_ids {
            if let Some(index) = self.items.iter().position(|item| item.tab_id == stale_id) {
                let item = self.items.remove(index);
                self.tabs_box.remove(&item.shell);
            }
        }

        for tab in tabs {
            if self.find_item(tab.id).is_none() {
                let item = self.build_item(controller, tab.id);
                self.tabs_box.append(&item.shell);
                self.items.push(item);
            }
        }

        for tab in tabs {
            if let Some(item) = self.find_item(tab.id) {
                let title = tab_display_title(tab);
                apply_title_tab_state(
                    &item.chrome,
                    &title,
                    &tab.subtitle,
                    tab.id == active_tab_id,
                    true,
                );
            }
        }

        if let Some(drag_state) = self.drag_state.as_ref()
            && !self.order.contains(&drag_state.dragged_id)
        {
            self.clear_drag_state();
        }

        if self.drag_state.is_none() {
            self.reorder_shells_to_model_order();
        }
    }

    fn build_item(&self, controller: &TabStripControllerHandle, tab_id: usize) -> TabStripItem {
        let chrome = build_title_tab_chrome();
        let shell = chrome.shell.clone();
        let select_button = chrome.select_button.clone();
        let close_button = chrome.close_button.clone();

        let select_handle = self.select_tab.clone();
        select_button.connect_clicked(move |_| {
            if let Some(select) = select_handle.borrow().as_ref() {
                select(tab_id);
            }
        });

        let rename_handle = self.request_tab_rename.clone();
        let rename_click = gtk::GestureClick::builder()
            .button(1)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        rename_click.connect_pressed(move |gesture, n_press, _, _| {
            if n_press != 2 {
                return;
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(rename) = rename_handle.borrow().as_ref() {
                rename(tab_id);
            }
        });
        select_button.add_controller(rename_click);

        close_button.set_focus_on_click(false);
        let close_handle = self.close_tab.clone();
        close_button.connect_clicked(move |_| {
            if let Some(close) = close_handle.borrow().as_ref() {
                close(tab_id);
            }
        });

        let middle_close = gtk::GestureClick::builder()
            .button(2)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let close_handle = self.close_tab.clone();
        middle_close.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(close) = close_handle.borrow().as_ref() {
                close(tab_id);
            }
        });
        shell.add_controller(middle_close);

        let popover = context_menu::popover(&shell);
        let menu = context_menu::menu_box();
        let detach_button = context_menu::action_button("Detach", None);
        {
            let detach_handle = self.detach_tab.clone();
            let popover = popover.clone();
            detach_button.connect_clicked(move |_| {
                popover.popdown();
                if let Some(detach) = detach_handle.borrow().as_ref() {
                    detach(tab_id);
                }
            });
        }
        menu.append(&detach_button);
        popover.set_child(Some(&menu));

        let right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let select_handle = self.select_tab.clone();
            let can_detach_handle = self.can_detach_tab.clone();
            let popover = popover.clone();
            right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                if let Some(select) = select_handle.borrow().as_ref() {
                    select(tab_id);
                }
                let can_detach = can_detach_handle
                    .borrow()
                    .as_ref()
                    .map(|can_detach| can_detach(tab_id))
                    .unwrap_or(false);
                if can_detach {
                    context_menu::popup_at(&popover, x, y);
                }
            });
        }
        shell.add_controller(right_click);

        let drag_source = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .button(1)
            .build();
        drag_source.connect_prepare(move |source, _, _| {
            suppress_native_tab_drag_icon(source);
            Some(gdk::ContentProvider::for_value(&(tab_id as u32).to_value()))
        });
        let controller_for_begin = controller.clone();
        drag_source.connect_drag_begin(move |_, _| {
            controller_for_begin.borrow_mut().begin_drag(tab_id);
        });
        let controller_for_cancel = controller.clone();
        drag_source.connect_drag_cancel(move |_, _, _| {
            controller_for_cancel.borrow_mut().cancel_drag(tab_id);
            false
        });
        let controller_for_end = controller.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            controller_for_end.borrow_mut().finish_drag(tab_id);
        });
        select_button.add_controller(drag_source);

        TabStripItem {
            tab_id,
            shell,
            chrome,
        }
    }

    fn find_item(&self, tab_id: usize) -> Option<TabStripItem> {
        self.items
            .iter()
            .find(|item| item.tab_id == tab_id)
            .cloned()
    }

    fn reorder_shells_to_model_order(&self) {
        let mut previous: Option<gtk::Widget> = None;
        for tab_id in &self.order {
            let Some(item) = self.find_item(*tab_id) else {
                continue;
            };
            let sibling = previous.as_ref();
            self.tabs_box.reorder_child_after(&item.shell, sibling);
            previous = Some(item.shell.clone().upcast());
        }
    }

    fn begin_drag(&mut self, tab_id: usize) {
        if self.drag_state.is_some() {
            return;
        }

        let Some(item) = self.find_item(tab_id) else {
            return;
        };
        let Some(origin_index) = self.order.iter().position(|id| *id == tab_id) else {
            return;
        };

        item.shell.add_css_class("is-lifted-source");
        item.shell.add_css_class("is-preview-slot");
        self.reorder_widget_for_preview(&item.shell.clone().upcast(), origin_index, tab_id);

        self.drag_state = Some(TabStripDragState {
            dragged_id: tab_id,
            origin_index,
            preview_index: origin_index,
        });
    }

    fn reorder_widget_for_preview(
        &self,
        widget: &gtk::Widget,
        preview_index: usize,
        dragged_id: usize,
    ) {
        let previous = if preview_index == 0 {
            None
        } else {
            self.order
                .iter()
                .copied()
                .filter(|tab_id| *tab_id != dragged_id)
                .nth(preview_index - 1)
                .and_then(|tab_id| self.find_item(tab_id))
                .map(|item| item.shell.upcast::<gtk::Widget>())
        };
        self.tabs_box.reorder_child_after(widget, previous.as_ref());
    }

    fn update_preview_for_x(&mut self, x: f64) -> bool {
        let Some((dragged_id, current_preview_index)) = self
            .drag_state
            .as_ref()
            .map(|state| (state.dragged_id, state.preview_index))
        else {
            return false;
        };

        let slots = self
            .order
            .iter()
            .copied()
            .filter(|tab_id| *tab_id != dragged_id)
            .filter_map(|tab_id| self.find_item(tab_id))
            .map(|item| {
                let allocation = item.shell.allocation();
                (f64::from(allocation.x()), f64::from(allocation.width()))
            })
            .collect::<Vec<_>>();

        let preview_index = preview_index_for_pointer(&slots, x);
        if preview_index == current_preview_index {
            return false;
        }

        if let Some(drag_state) = self.drag_state.as_mut() {
            drag_state.preview_index = preview_index;
        }
        if let Some(item) = self.find_item(dragged_id) {
            self.reorder_widget_for_preview(
                &item.shell.clone().upcast(),
                preview_index,
                dragged_id,
            );
        }
        true
    }

    fn update_preview_from_widget(&mut self, widget: &gtk::Widget, x: f64, y: f64) -> bool {
        let strip_x = widget
            .translate_coordinates(&self.tabs_box, x, y)
            .map(|(strip_x, _)| strip_x)
            .unwrap_or(x);
        self.update_preview_for_x(strip_x)
    }

    fn prepare_drop(&mut self, value: &glib::Value, x: f64) -> Result<Option<(usize, usize)>, ()> {
        let Ok(moved_id) = value.get::<u32>() else {
            return Err(());
        };
        let moved_id = moved_id as usize;
        let Some(drag_state) = self.drag_state.as_ref() else {
            return Err(());
        };
        if moved_id != drag_state.dragged_id {
            return Err(());
        }

        self.update_preview_for_x(x);
        let (origin_index, preview_index) = match self.drag_state.as_ref() {
            Some(state) => (state.origin_index, state.preview_index),
            None => return Err(()),
        };

        self.clear_drag_state();

        if preview_index != origin_index {
            Ok(Some((moved_id, preview_index)))
        } else {
            Ok(None)
        }
    }

    fn prepare_drop_from_widget(
        &mut self,
        value: &glib::Value,
        widget: &gtk::Widget,
        x: f64,
        y: f64,
    ) -> Result<Option<(usize, usize)>, ()> {
        let strip_x = widget
            .translate_coordinates(&self.tabs_box, x, y)
            .map(|(strip_x, _)| strip_x)
            .unwrap_or(x);
        self.prepare_drop(value, strip_x)
    }

    fn cancel_drag(&mut self, tab_id: usize) {
        if self
            .drag_state
            .as_ref()
            .map(|state| state.dragged_id == tab_id)
            .unwrap_or(false)
        {
            self.clear_drag_state();
        }
    }

    fn finish_drag(&mut self, tab_id: usize) {
        self.cancel_drag(tab_id);
    }

    fn clear_drag_state(&mut self) {
        if let Some(drag_state) = self.drag_state.take()
            && let Some(item) = self.find_item(drag_state.dragged_id)
        {
            item.shell.remove_css_class("is-lifted-source");
            item.shell.remove_css_class("is-preview-slot");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_tab_strip_controller(
    tabs_box: &gtk::Box,
    drop_surface: &gtk::Box,
    select_tab: SelectTabHandle,
    close_tab: TabActionHandle,
    request_tab_rename: TabActionHandle,
    detach_tab: TabActionHandle,
    can_detach_tab: TabPredicateHandle,
    reorder_tab: ReorderTabHandle,
) -> TabStripControllerHandle {
    let controller = Rc::new(RefCell::new(TabStripController::new(
        tabs_box.clone(),
        select_tab,
        close_tab,
        request_tab_rename,
        detach_tab,
        can_detach_tab,
    )));

    let drop_target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    drop_target.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let controller_for_enter = controller.clone();
        drop_target.connect_enter(move |target, x, y| {
            let Some(widget) = target.widget() else {
                return gdk::DragAction::empty();
            };
            let mut controller = controller_for_enter.borrow_mut();
            if controller.drag_state.is_none() {
                return gdk::DragAction::empty();
            }
            controller.update_preview_from_widget(&widget, x, y);
            gdk::DragAction::MOVE
        });
    }
    {
        let controller_for_motion = controller.clone();
        drop_target.connect_motion(move |target, x, y| {
            let Some(widget) = target.widget() else {
                return gdk::DragAction::empty();
            };
            let mut controller = controller_for_motion.borrow_mut();
            if controller.drag_state.is_none() {
                return gdk::DragAction::empty();
            }
            controller.update_preview_from_widget(&widget, x, y);
            gdk::DragAction::MOVE
        });
    }
    {
        let controller_for_drop = controller.clone();
        let reorder_handle = reorder_tab.clone();
        drop_target.connect_drop(move |target, value, x, y| {
            let Some(widget) = target.widget() else {
                return false;
            };
            let drop_result = {
                let mut controller = controller_for_drop.borrow_mut();
                controller.prepare_drop_from_widget(value, &widget, x, y)
            };
            match drop_result {
                Ok(Some((moved_id, preview_index))) => {
                    if let Some(reorder) = reorder_handle.borrow().as_ref() {
                        reorder(moved_id, preview_index);
                    }
                    true
                }
                Ok(None) => true,
                Err(()) => false,
            }
        });
    }
    drop_surface.add_controller(drop_target);

    controller
}

fn sync_tab_strip(
    controller: &TabStripControllerHandle,
    tabs: &[WorkspaceTab],
    active_tab_id: usize,
) {
    controller
        .borrow_mut()
        .sync(controller, tabs, active_tab_id);
}

fn register_linux_main_attach_target(
    window_id: usize,
    window: &adw::ApplicationWindow,
    attach_workspace_tab: AttachWorkspaceTabHandle,
) {
    let weak_window = window.downgrade();
    LINUX_MAIN_ATTACH_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.window_id != window_id && target.window.upgrade().is_some());
        targets.push(LinuxMainAttachTarget {
            window_id,
            window: weak_window,
            attach_workspace_tab,
        });
    });
}

fn unregister_linux_main_attach_target(window_id: usize) {
    LINUX_MAIN_ATTACH_TARGETS.with(|targets| {
        targets
            .borrow_mut()
            .retain(|target| target.window_id != window_id && target.window.upgrade().is_some());
    });
}

fn note_linux_main_attach_target_active(window_id: usize) {
    LINUX_MAIN_ATTACH_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        let Some(index) = targets
            .iter()
            .position(|target| target.window_id == window_id)
        else {
            targets.retain(|target| target.window.upgrade().is_some());
            return;
        };
        let target = targets.remove(index);
        targets.retain(|target| target.window.upgrade().is_some());
        targets.push(target);
    });
}

fn linux_main_attach_target(preferred_window_id: Option<usize>) -> Option<LinuxMainAttachTarget> {
    LINUX_MAIN_ATTACH_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.window.upgrade().is_some());
        if let Some(preferred_window_id) = preferred_window_id
            && let Some(target) = targets
                .iter()
                .find(|target| target.window_id == preferred_window_id)
                .cloned()
        {
            return Some(target);
        }
        targets.last().cloned()
    })
}

#[allow(clippy::too_many_arguments)]
fn attach_workspace_tab_to_main_window(
    window_id: usize,
    tab_view: &adw::TabView,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    next_tab_id: &Rc<Cell<usize>>,
    active_tab_id: &Rc<Cell<usize>>,
    select_tab: &SelectTabHandle,
    refresh_tab_strip: &dyn Fn(),
    session_store: &SessionStore,
    mut tab: WorkspaceTab,
) {
    let page_shell = tab.page_shell.clone();
    let runtime = match &tab.content {
        TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
        TabContent::LaunchDeck => None,
    };
    if let Some(parent) = page_shell.parent()
        && let Ok(parent_box) = parent.downcast::<gtk::Box>()
    {
        parent_box.remove(&page_shell);
    }

    {
        let tabs_ref = tabs.borrow();
        if tab.id == 0 || tabs_ref.iter().any(|existing| existing.id == tab.id) {
            tab.id = tabs_ref
                .iter()
                .map(|existing| existing.id)
                .max()
                .unwrap_or(0)
                + 1;
        }
    }

    let tab_id = tab.id;
    if next_tab_id.get() <= tab_id {
        next_tab_id.set(tab_id + 1);
    }
    rebind_workspace_tab_layout(&tab, tabs);
    tabs.borrow_mut().push(tab);
    tab_view.append(&page_shell);
    {
        let tabs = tabs.borrow();
        if let Some(tab) = tabs.iter().find(|tab| tab.id == tab_id) {
            sync_tab_page_metadata(tab_view, tab);
        }
    }
    refresh_tab_strip();
    if let Some(select) = select_tab.borrow().as_ref() {
        select(tab_id);
    } else {
        active_tab_id.set(tab_id);
    }
    if let Some(runtime) = runtime {
        runtime.reflow_layout();
    }
    note_linux_main_attach_target_active(window_id);
    save_application_window_session_state(window_id, tabs, active_tab_id.get(), session_store);
    logging::info(format!(
        "reattached workspace tab {} to window {}",
        tab_id, window_id
    ));
}

fn rebuild_launch_tab(tab_id: usize, context: &LaunchTabContext) {
    let page_shell = context
        .tabs
        .borrow()
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.page_shell.clone())
        .expect("launch tab should exist");

    let load_outcome = context.preset_store.load_presets_with_status();
    let asset_outcome = std::env::current_dir()
        .ok()
        .map(|root| context.asset_store.load_assets_for_workspace_root(&root))
        .unwrap_or_else(|| context.asset_store.load_assets_with_status());
    let presets = load_outcome.presets;
    let preferences = context.preference_store.load();
    let preset_store = context.preset_store.as_ref().clone();
    let window = context.window.clone();
    let show_workspace_handle = context.show_workspace_handle.clone();
    let close_tab_handle = context.close_tab_handle.clone();
    let refresh_handle = context.refresh_launch_tabs.clone();

    let theme_preview_window = window.clone();
    let density_preview_window = window.clone();

    let launch_surface = launch_screen::build(
        launch_screen::LaunchScreenInput {
            load_warning: combine_warnings(load_outcome.warning, asset_outcome.warning),
            presets,
            assets: asset_outcome.assets,
            default_theme: preferences.default_theme,
            default_density: preferences.default_density,
            default_restore_mode: preferences.default_restore_mode,
            preset_store,
        },
        launch_screen::LaunchScreenActions {
            on_theme_preview: Rc::new(move |theme| {
                apply_theme_mode(&theme_preview_window, theme);
            }),
            on_density_preview: Rc::new({
                move |density| {
                    apply_optional_window_density(&density_preview_window, Some(density));
                }
            }),
            on_launch: Rc::new(move |preset, workspace_root| {
                if let Some(show_workspace) = show_workspace_handle.borrow().as_ref() {
                    show_workspace(tab_id, preset, workspace_root);
                }
            }),
            on_cancel: Rc::new({
                let close_tab_handle = close_tab_handle.clone();
                move || {
                    if let Some(close) = close_tab_handle.borrow().as_ref() {
                        close(tab_id);
                    }
                }
            }),
            on_presets_changed: Rc::new(move || {
                let refresh_for_idle = refresh_handle.clone();
                glib::idle_add_local_once(move || {
                    if let Some(refresh) = refresh_for_idle.borrow().as_ref() {
                        refresh();
                    }
                });
            }),
        },
    );

    replace_tab_page_content(&page_shell, &launch_surface);
}

fn clear_all_tabs(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    tab_view: &adw::TabView,
    active_tab_id: &Cell<usize>,
    forced_tab_closes: &Rc<RefCell<HashSet<usize>>>,
    suppress_empty_replacement: &Cell<bool>,
) {
    let tab_ids = tabs.borrow().iter().map(|tab| tab.id).collect::<Vec<_>>();
    active_tab_id.set(0);
    suppress_empty_replacement.set(true);
    for tab_id in tab_ids {
        let page = {
            let tabs = tabs.borrow();
            tab_page_for_id(tab_view, &tabs, tab_id)
        };
        if let Some(page) = page {
            forced_tab_closes.borrow_mut().insert(tab_id);
            tab_view.close_page(&page);
        }
    }
    suppress_empty_replacement.set(false);
}

fn restore_saved_session(
    context: &RestoreSessionContext,
    saved_session: SavedSession,
    replace_existing: bool,
) {
    let save_guard = context.session_persistence.suppress();
    if replace_existing {
        clear_all_tabs(
            &context.tabs,
            &context.tab_view,
            &context.active_tab_id,
            &context.forced_tab_closes,
            &context.suppress_empty_replacement,
        );
    }

    let mut restored_ids = Vec::with_capacity(saved_session.tabs.len());
    for saved_tab in saved_session.tabs {
        let tab_id = context.next_tab_id.get();
        context.next_tab_id.set(tab_id + 1);

        let workspace_root = saved_tab.workspace_root;
        let preset = saved_tab.preset;
        let terminal_zoom_steps =
            clamp_terminal_zoom_steps(preset.density, saved_tab.terminal_zoom_steps);
        let layout_target = make_workspace_layout_target(&context.tabs, tab_id);
        let assets = context
            .asset_store
            .load_assets_for_workspace_root(&workspace_root)
            .assets;

        let built_workspace = workspace_view::build_with_layout_change_handler(
            &preset,
            &workspace_root,
            &assets,
            resolved_theme_uses_dark_palette(preset.theme),
            terminal_zoom_steps,
            context.preference_store.load().max_reconnect_attempts,
            {
                let layout_target = layout_target.clone();
                let session_persistence = context.session_persistence.clone();
                Rc::new(move |next_layout| {
                    apply_workspace_layout_change(&layout_target, next_layout);
                    session_persistence.save_soon("workspace layout changed");
                })
            },
        );
        let page_shell = build_tab_page_shell();
        replace_tab_page_content(&page_shell, &built_workspace.widget);
        context.tabs.borrow_mut().push(WorkspaceTab {
            id: tab_id,
            default_title: format!("Workspace {}", tab_id),
            custom_title: saved_tab.custom_title,
            subtitle: workspace_root.display().to_string(),
            page_shell: page_shell.clone(),
            content: TabContent::Workspace(Box::new(WorkspaceState {
                preset: preset.clone(),
                assets: assets.clone(),
                runtime: built_workspace.runtime.clone(),
                terminal_zoom_steps,
                layout_target: layout_target.clone(),
            })),
            workspace_root: Some(workspace_root.clone()),
        });
        let tab = context
            .tabs
            .borrow()
            .iter()
            .find(|tab| tab.id == tab_id)
            .cloned()
            .expect("restored workspace tab should exist");
        context.tab_view.append(&page_shell);
        sync_tab_page_metadata(&context.tab_view, &tab);
        logging::info(format!(
            "restored workspace tab {} preset='{}' root='{}'",
            tab_id,
            preset.name,
            workspace_root.display()
        ));
        restored_ids.push(tab_id);
    }

    let restored_active_id = restored_ids
        .get(saved_session.active_tab_index)
        .copied()
        .or_else(|| restored_ids.first().copied());

    if let Some(active_id) = restored_active_id
        && let Some(select) = context.select_tab.borrow().as_ref()
    {
        select(active_id);
    }
    drop(save_guard);
    context
        .session_persistence
        .save_now("saved workspace session restored");
}

fn apply_shell_profile(
    header: &adw::HeaderBar,
    window: &adw::ApplicationWindow,
    preset: &WorkspacePreset,
) {
    configure_window_controls(header);

    logging::info(format!(
        "applying shell profile preset='{}' theme={} density={}",
        preset.name,
        preset.theme.label(),
        preset.density.label()
    ));

    apply_theme_mode(window, preset.theme);

    apply_optional_window_density(window, Some(preset.density));
}

fn apply_launch_profile(
    header: &adw::HeaderBar,
    window: &adw::ApplicationWindow,
    preferences: &AppPreferences,
) {
    configure_window_controls(header);
    logging::info(format!(
        "applying launch profile theme={} density={}",
        preferences.default_theme.label(),
        preferences.default_density.label()
    ));
    apply_theme_mode(window, preferences.default_theme);
    apply_optional_window_density(window, Some(preferences.default_density));
}

fn active_tab_is_workspace(tabs: &Rc<RefCell<Vec<WorkspaceTab>>>, active_tab_id: usize) -> bool {
    tabs.borrow()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .map(|tab| matches!(tab.content, TabContent::Workspace(_)))
        .unwrap_or(false)
}

fn toggle_workspace_fullscreen(
    window: &adw::ApplicationWindow,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
) {
    if !active_tab_is_workspace(tabs, active_tab_id) {
        return;
    }

    window.set_fullscreened(!window.is_fullscreen());
}

fn cycle_active_workspace_density(
    window: &adw::ApplicationWindow,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
) -> Option<ApplicationDensity> {
    let (workspace_name, next_density, terminal_zoom_steps, runtime) = {
        let mut tabs = tabs.borrow_mut();
        let tab = tabs.iter_mut().find(|tab| tab.id == active_tab_id)?;
        let workspace = match &mut tab.content {
            TabContent::Workspace(workspace) => workspace,
            TabContent::LaunchDeck => return None,
        };
        let next_density = workspace.preset.density.next();
        workspace.terminal_zoom_steps =
            clamp_terminal_zoom_steps(next_density, workspace.terminal_zoom_steps);
        workspace.preset.density = next_density;
        (
            workspace.preset.name.clone(),
            next_density,
            workspace.terminal_zoom_steps,
            workspace.runtime.clone(),
        )
    };

    runtime.apply_appearance(
        window_uses_dark_theme(window),
        next_density,
        terminal_zoom_steps,
    );
    apply_optional_window_density(window, Some(next_density));
    logging::info(format!(
        "cycled workspace density preset='{}' density={}",
        workspace_name,
        next_density.label()
    ));
    Some(next_density)
}

fn adjust_active_workspace_zoom(
    window: &adw::ApplicationWindow,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    delta: i32,
) -> Option<i32> {
    let (workspace_name, density, terminal_zoom_steps, runtime) = {
        let mut tabs = tabs.borrow_mut();
        let tab = tabs.iter_mut().find(|tab| tab.id == active_tab_id)?;
        let workspace = match &mut tab.content {
            TabContent::Workspace(workspace) => workspace,
            TabContent::LaunchDeck => return None,
        };
        let next_zoom_steps = clamp_terminal_zoom_steps(
            workspace.preset.density,
            workspace.terminal_zoom_steps + delta,
        );
        if next_zoom_steps == workspace.terminal_zoom_steps {
            return None;
        }
        workspace.terminal_zoom_steps = next_zoom_steps;
        (
            workspace.preset.name.clone(),
            workspace.preset.density,
            workspace.terminal_zoom_steps,
            workspace.runtime.clone(),
        )
    };

    runtime.apply_appearance(window_uses_dark_theme(window), density, terminal_zoom_steps);
    logging::info(format!(
        "adjusted workspace terminal zoom preset='{}' zoom_steps={}",
        workspace_name, terminal_zoom_steps
    ));
    Some(terminal_zoom_steps)
}

fn active_workspace_runtime(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
) -> Option<workspace_view::WorkspaceRuntime> {
    tabs.borrow()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .and_then(|tab| match &tab.content {
            TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
            TabContent::LaunchDeck => None,
        })
}

fn sync_linux_voice_global_hotkey(
    registration: &Rc<RefCell<Option<VoiceGlobalHotkeyRegistration>>>,
    voice: &crate::voice::VoicePreferences,
    voice_event_tx: &mpsc::Sender<VoiceUiEvent>,
) {
    if !should_register_linux_voice_global_hotkey(voice) {
        registration.borrow_mut().take();
        return;
    }

    {
        let current = registration.borrow();
        if let Some(current) = current.as_ref()
            && current.shortcut() == voice.hotkey
        {
            if current.unavailable_retry_pending() {
                return;
            }
            if matches!(current, VoiceGlobalHotkeyRegistration::Active { .. }) {
                return;
            }
        }
    }

    registration.borrow_mut().take();
    let (global_tx, global_rx) = mpsc::channel::<LinuxGlobalHotkeyEvent>();
    match LinuxGlobalHotkeyHandle::start(voice.hotkey.clone(), global_tx) {
        Ok(handle) => {
            let shortcut = voice.hotkey.clone();
            logging::info(format!(
                "registered Linux X11 global voice hotkey {shortcut}"
            ));
            *registration.borrow_mut() =
                Some(VoiceGlobalHotkeyRegistration::Active { shortcut, handle });
            let ui_tx = voice_event_tx.clone();
            std::thread::spawn(move || {
                while let Ok(event) = global_rx.recv() {
                    logging::info(format!("voice global hotkey event={event:?}"));
                    let _ = ui_tx.send(match event {
                        LinuxGlobalHotkeyEvent::Pressed => VoiceUiEvent::HotkeyPressed,
                        LinuxGlobalHotkeyEvent::Released => VoiceUiEvent::HotkeyReleased,
                    });
                }
            });
        }
        Err(error) => {
            logging::error(format!(
                "Linux global voice hotkey unavailable for '{}': {error}",
                voice.hotkey
            ));
            *registration.borrow_mut() = Some(VoiceGlobalHotkeyRegistration::Unavailable {
                shortcut: voice.hotkey.clone(),
                last_attempt: Instant::now(),
            });
        }
    }
}

fn should_register_linux_voice_global_hotkey(voice: &crate::voice::VoicePreferences) -> bool {
    voice.enabled
        && (voice.prefer_global_hotkey || voice.activation_mode == VoiceActivationMode::PushToTalk)
}

#[allow(clippy::too_many_arguments)]
fn install_voice_hotkey_controller(
    window: &adw::ApplicationWindow,
    controller_handle: &VoiceKeyControllerHandle,
    preference_store: Rc<PreferenceStore>,
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: Rc<Cell<usize>>,
    voice_hud: VoiceHud,
    toast_overlay: adw::ToastOverlay,
    voice_transcriber: Rc<VoiceTranscriberHandle>,
    voice_listening: Rc<Cell<bool>>,
    voice_starting: Rc<Cell<bool>>,
    voice_stopping: Rc<Cell<bool>>,
    voice_local_key_pressed: Rc<Cell<bool>>,
    voice_warm_state: Rc<Cell<VoiceWarmState>>,
    voice_warm_generation: Rc<Cell<u64>>,
    voice_warm_error: Rc<RefCell<Option<String>>>,
    voice_event_tx: mpsc::Sender<VoiceUiEvent>,
) {
    if let Some(existing) = controller_handle.borrow_mut().take() {
        window.remove_controller(&existing);
    }

    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);

    {
        let preference_store = preference_store.clone();
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let voice_hud = voice_hud.clone();
        let toast_overlay = toast_overlay.clone();
        let voice_transcriber = voice_transcriber.clone();
        let voice_listening = voice_listening.clone();
        let voice_starting = voice_starting.clone();
        let voice_stopping = voice_stopping.clone();
        let voice_local_key_pressed = voice_local_key_pressed.clone();
        let voice_warm_state = voice_warm_state.clone();
        let voice_warm_generation = voice_warm_generation.clone();
        let voice_warm_error = voice_warm_error.clone();
        let voice_event_tx = voice_event_tx.clone();
        controller.connect_key_pressed(move |_, key, _, state| {
            let preferences = preference_store.load();
            let voice = preferences.voice.clone();
            if !voice_key_event_matches(&voice.hotkey, key, state) {
                return glib::Propagation::Proceed;
            }
            if voice_local_key_pressed.replace(true) {
                logging::info("voice local hotkey press ignored: repeat");
                return glib::Propagation::Stop;
            }
            logging::info("voice local hotkey press matched");

            if !voice.enabled {
                voice_hud.show("Voice disabled", None);
                show_toast(&toast_overlay, "Enable voice input in Settings first");
                return glib::Propagation::Stop;
            }

            match voice.activation_mode {
                VoiceActivationMode::Toggle if voice_listening.get() => {
                    stop_voice_capture(
                        &voice_transcriber,
                        &voice_listening,
                        &voice_stopping,
                        &voice_hud,
                        &voice_event_tx,
                    );
                }
                VoiceActivationMode::Toggle | VoiceActivationMode::PushToTalk => {
                    if !voice_listening.get() && !voice_starting.get() && !voice_stopping.get() {
                        start_voice_capture(
                            &preference_store,
                            &tabs,
                            active_tab_id.get(),
                            &voice_hud,
                            &toast_overlay,
                            &voice_transcriber,
                            &voice_listening,
                            &voice_starting,
                            &voice_stopping,
                            &voice_warm_state,
                            &voice_warm_generation,
                            &voice_warm_error,
                            &voice_event_tx,
                        );
                    }
                }
            }

            glib::Propagation::Stop
        });
    }

    {
        let preference_store = preference_store.clone();
        let voice_hud = voice_hud.clone();
        let voice_transcriber = voice_transcriber.clone();
        let voice_listening = voice_listening.clone();
        let voice_starting = voice_starting.clone();
        let voice_stopping = voice_stopping.clone();
        let voice_local_key_pressed = voice_local_key_pressed.clone();
        let voice_event_tx = voice_event_tx.clone();
        controller.connect_key_released(move |_, key, _, _state| {
            let preferences = preference_store.load();
            let voice = preferences.voice.clone();
            if !voice_key_matches_accelerator_key(&voice.hotkey, key) {
                return;
            }
            voice_local_key_pressed.set(false);
            if voice.activation_mode != VoiceActivationMode::PushToTalk {
                return;
            }
            logging::info("voice local hotkey release matched");
            if voice_starting.replace(false) && !voice_listening.get() {
                finish_pending_voice_capture(
                    &voice_transcriber,
                    &voice_stopping,
                    &voice_hud,
                    &voice_event_tx,
                );
            } else {
                stop_voice_capture(
                    &voice_transcriber,
                    &voice_listening,
                    &voice_stopping,
                    &voice_hud,
                    &voice_event_tx,
                );
            }
        });
    }

    window.add_controller(controller.clone());
    *controller_handle.borrow_mut() = Some(controller);
}

fn save_voice_pack_download_progress(preference_store: &PreferenceStore, percent: u8) {
    let mut preferences = preference_store.load();
    if matches!(
        preferences.voice.pack_status,
        VoicePackStatus::Installed { .. } | VoicePackStatus::Error { .. }
    ) {
        return;
    }
    preferences.voice.pack_status = VoicePackStatus::Downloading {
        percent: percent.clamp(1, 99),
    };
    preference_store.save(&preferences);
}

fn start_voice_pack_progress_heartbeat(
    preference_store: PreferenceStore,
    start_percent: u8,
    end_percent: u8,
) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut percent = start_percent.clamp(1, 99);
        let end_percent = end_percent.clamp(percent, 99);
        save_voice_pack_download_progress(&preference_store, percent);
        while !worker_stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if worker_stop.load(Ordering::Relaxed) {
                break;
            }
            if percent < end_percent {
                percent += 1;
            }
            save_voice_pack_download_progress(&preference_store, percent);
        }
    });
    (stop, handle)
}

fn reset_voice_warm_tracking(
    voice_warm_state: &Rc<Cell<VoiceWarmState>>,
    voice_warm_generation: &Rc<Cell<u64>>,
    voice_warm_error: &Rc<RefCell<Option<String>>>,
) {
    voice_warm_generation.set(voice_warm_generation.get().wrapping_add(1));
    voice_warm_state.set(VoiceWarmState::Cold);
    voice_warm_error.borrow_mut().take();
}

fn reserve_voice_warm_generation(voice_warm_generation: &Rc<Cell<u64>>) -> u64 {
    let generation = voice_warm_generation.get().wrapping_add(1);
    voice_warm_generation.set(generation);
    generation
}

fn warm_voice_engine_if_ready(
    preference_store: &PreferenceStore,
    voice_transcriber: &VoiceTranscriberHandle,
    voice_event_tx: &mpsc::Sender<VoiceUiEvent>,
    voice_warm_state: &Rc<Cell<VoiceWarmState>>,
    voice_warm_generation: &Rc<Cell<u64>>,
    voice_warm_error: &Rc<RefCell<Option<String>>>,
) {
    let voice = preference_store.load().voice;
    if !voice.enabled {
        return;
    }
    if matches!(
        voice_warm_state.get(),
        VoiceWarmState::Warming | VoiceWarmState::Ready
    ) {
        return;
    }
    if !matches!(voice.pack_status, VoicePackStatus::Installed { .. }) {
        return;
    }
    let manifest = pack::builtin_parakeet_manifest();
    let Some(root) = pack::default_voice_pack_dir() else {
        return;
    };
    if let Err(detail) = refresh_builtin_voice_pack_assets_for_runtime(&root) {
        logging::error(format!(
            "voice model warm-up blocked: could not refresh bundled voice pack assets: {detail}"
        ));
        voice_warm_state.set(VoiceWarmState::Failed);
        voice_warm_error.replace(Some(format!("Voice pack refresh failed: {detail}")));
        return;
    }
    let health = pack::health_check(&root, &manifest);
    if matches!(health, VoicePackHealth::Ready { .. }) {
        let generation = reserve_voice_warm_generation(voice_warm_generation);
        voice_warm_state.set(VoiceWarmState::Warming);
        voice_warm_error.borrow_mut().take();
        voice_transcriber.prepare(
            manifest,
            health,
            voice.engine_mode,
            generation,
            voice_event_tx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn start_voice_capture(
    preference_store: &PreferenceStore,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    voice_hud: &VoiceHud,
    toast_overlay: &adw::ToastOverlay,
    voice_transcriber: &Rc<VoiceTranscriberHandle>,
    voice_listening: &Rc<Cell<bool>>,
    voice_starting: &Rc<Cell<bool>>,
    voice_stopping: &Rc<Cell<bool>>,
    voice_warm_state: &Rc<Cell<VoiceWarmState>>,
    voice_warm_generation: &Rc<Cell<u64>>,
    voice_warm_error: &Rc<RefCell<Option<String>>>,
    voice_event_tx: &mpsc::Sender<VoiceUiEvent>,
) {
    let preferences = preference_store.load();
    let voice = preferences.voice.clone();
    logging::info(format!(
        "voice capture start requested enabled={} mode={} listening={} starting={} stopping={} warm={:?} active_tab={active_tab_id}",
        voice.enabled,
        voice.activation_mode.label(),
        voice_listening.get(),
        voice_starting.get(),
        voice_stopping.get(),
        voice_warm_state.get(),
    ));
    let Some(runtime) = active_workspace_runtime(tabs, active_tab_id) else {
        logging::info("voice capture start blocked: no active workspace target");
        voice_hud.show("No workspace target", None);
        show_toast(
            toast_overlay,
            "Open a workspace and focus a terminal pane before dictating",
        );
        return;
    };
    if !runtime.focused_terminal_available() {
        logging::info("voice capture start blocked: no focused terminal target");
        voice_hud.show("No focused terminal target", None);
        show_toast(toast_overlay, "Focus a terminal pane before dictating");
        return;
    }

    let manifest = pack::builtin_parakeet_manifest();
    let Some(root) = pack::default_voice_pack_dir() else {
        logging::error("voice capture start blocked: could not resolve app data directory");
        voice_hud.show(
            "Voice pack error",
            Some("Could not resolve app data directory"),
        );
        return;
    };
    if let Err(detail) = refresh_builtin_voice_pack_assets_for_runtime(&root) {
        logging::error(format!(
            "voice capture start blocked: could not refresh bundled voice pack assets: {detail}"
        ));
        voice_hud.show("Voice pack refresh failed", Some(&detail));
        show_toast(toast_overlay, "Voice pack refresh failed");
        return;
    }
    let health = pack::health_check(&root, &manifest);
    if !matches!(health, VoicePackHealth::Ready { .. }) {
        logging::info("voice capture start blocked: voice pack not ready");
        voice_hud.show(
            "Voice pack not installed",
            Some("Install NVIDIA Parakeet from Settings"),
        );
        show_toast(
            toast_overlay,
            "Install the NVIDIA Parakeet voice pack in Settings first",
        );
        return;
    }

    match voice_hotkey_warm_gate(voice_warm_state.get()) {
        VoiceHotkeyWarmGate::StartCapture => {}
        VoiceHotkeyWarmGate::WaitForWarm => {
            logging::info("voice capture start blocked: voice model is still warming");
            voice_hud.show("Voice model is preparing", Some("Try again shortly"));
            return;
        }
        VoiceHotkeyWarmGate::RequestWarm => {
            logging::info("voice capture start blocked: requesting voice model warm-up");
            warm_voice_engine_if_ready(
                preference_store,
                voice_transcriber,
                voice_event_tx,
                voice_warm_state,
                voice_warm_generation,
                voice_warm_error,
            );
            voice_hud.show("Voice model is preparing", Some("Try again shortly"));
            return;
        }
        VoiceHotkeyWarmGate::ReportFailure => {
            let detail = voice_warm_error
                .borrow()
                .clone()
                .unwrap_or_else(|| "Run a voice runtime health check from Settings".into());
            logging::error(format!(
                "voice capture start blocked: voice model warm-up failed: {detail}"
            ));
            voice_hud.show("Voice model failed to warm", Some(&detail));
            return;
        }
    }

    voice_listening.set(false);
    voice_starting.set(true);
    voice_stopping.set(false);
    voice_hud.show("Starting voice capture…", Some("Preparing microphone"));
    logging::info("voice capture start queued");
    voice_transcriber.start_capture(
        manifest,
        health,
        voice.engine_mode,
        voice.microphone_id,
        voice_event_tx,
    );
}

fn stop_voice_capture(
    voice_transcriber: &Rc<VoiceTranscriberHandle>,
    voice_listening: &Rc<Cell<bool>>,
    voice_stopping: &Rc<Cell<bool>>,
    voice_hud: &VoiceHud,
    voice_event_tx: &mpsc::Sender<VoiceUiEvent>,
) {
    if !voice_listening.replace(false) {
        logging::info("voice capture stop ignored: not listening");
        return;
    }
    voice_stopping.set(true);
    voice_hud.show("Finalizing voice text…", None);
    logging::info("voice capture stop requested");
    voice_transcriber.stop(voice_event_tx);
}

fn finish_pending_voice_capture(
    voice_transcriber: &Rc<VoiceTranscriberHandle>,
    voice_stopping: &Rc<Cell<bool>>,
    voice_hud: &VoiceHud,
    voice_event_tx: &mpsc::Sender<VoiceUiEvent>,
) {
    voice_stopping.set(true);
    voice_hud.show("Finalizing voice text…", None);
    logging::info("voice capture stop requested before listening started");
    voice_transcriber.stop(voice_event_tx);
}

fn voice_key_event_matches(accelerator: &str, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let Some((expected_key, expected_modifiers)) = gtk::accelerator_parse(accelerator) else {
        return false;
    };
    let event_modifiers = state & gtk::accelerator_get_default_mod_mask();
    key == expected_key && event_modifiers == expected_modifiers
}

fn voice_key_matches_accelerator_key(accelerator: &str, key: gdk::Key) -> bool {
    let Some((expected_key, _)) = gtk::accelerator_parse(accelerator) else {
        return false;
    };
    key == expected_key
}

fn install_shortcut_controller<F>(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    shortcut_name: &str,
    accelerators: &[String],
    on_activate: F,
) where
    F: Fn() -> glib::Propagation + 'static,
{
    if let Some(existing) = controller_handle.borrow_mut().take() {
        window.remove_controller(&existing);
    }

    let shortcut_controller = gtk::ShortcutController::new();
    shortcut_controller.set_scope(gtk::ShortcutScope::Global);
    shortcut_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let on_activate = Rc::new(on_activate);
    let mut installed_triggers = Vec::new();
    let mut active_labels = Vec::new();
    for accelerator in accelerators {
        let accelerator = accelerator.trim();
        if accelerator.is_empty() || installed_triggers.iter().any(|item| item == accelerator) {
            continue;
        }
        installed_triggers.push(accelerator.to_string());

        let Some(trigger) = gtk::ShortcutTrigger::parse_string(accelerator) else {
            logging::error(format!(
                "failed to parse {} shortcut accelerator='{}'",
                shortcut_name, accelerator
            ));
            continue;
        };

        active_labels.push(trigger.to_str().to_string());
        let on_activate = on_activate.clone();
        let action = gtk::CallbackAction::new(move |_, _| on_activate());
        shortcut_controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
    }

    if installed_triggers.is_empty() {
        logging::error(format!(
            "failed to install {} shortcut: no valid accelerators",
            shortcut_name,
        ));
        return;
    }

    logging::info(format!(
        "installed {} shortcut requested={:?} active={:?}",
        shortcut_name, installed_triggers, active_labels
    ));
    window.add_controller(shortcut_controller.clone());
    *controller_handle.borrow_mut() = Some(shortcut_controller);
}

fn zoom_in_shortcut_accelerators(shortcut: &str) -> Vec<String> {
    equivalent_shortcut_accelerators(
        shortcut,
        &[
            &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"],
            &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
            &["<Primary>plus", "<Primary>equal", "<Primary>KP_Add"],
            &["<Alt>plus", "<Alt>equal", "<Alt>KP_Add"],
            &["<Ctrl><Alt>plus", "<Ctrl><Alt>equal", "<Ctrl><Alt>KP_Add"],
            &[
                "<Control><Alt>plus",
                "<Control><Alt>equal",
                "<Control><Alt>KP_Add",
            ],
        ],
    )
}

fn zoom_out_shortcut_accelerators(shortcut: &str) -> Vec<String> {
    equivalent_shortcut_accelerators(
        shortcut,
        &[
            &["<Ctrl>minus", "<Ctrl>KP_Subtract"],
            &["<Control>minus", "<Control>KP_Subtract"],
            &["<Primary>minus", "<Primary>KP_Subtract"],
            &["<Alt>minus", "<Alt>KP_Subtract"],
            &["<Ctrl><Alt>minus", "<Ctrl><Alt>KP_Subtract"],
            &["<Control><Alt>minus", "<Control><Alt>KP_Subtract"],
        ],
    )
}

fn command_palette_shortcut_accelerators(shortcut: &str) -> Vec<String> {
    equivalent_shortcut_accelerators(
        shortcut,
        &[
            &["<Ctrl><Shift>P", "<Primary><Shift>P", "<Control><Shift>P"],
            &["<Ctrl>P", "<Primary>P", "<Control>P"],
        ],
    )
}

fn equivalent_shortcut_accelerators(shortcut: &str, families: &[&[&str]]) -> Vec<String> {
    let trimmed = shortcut.trim();
    let mut accelerators = vec![trimmed.to_string()];

    if let Some(family) = families
        .iter()
        .find(|family| family.iter().any(|candidate| candidate == &trimmed))
    {
        accelerators.extend(family.iter().map(|candidate| (*candidate).to_string()));
    }

    accelerators
}

fn install_command_palette_shortcut(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    shortcut: &str,
    open_command_palette: Rc<dyn Fn()>,
) {
    install_shortcut_controller(
        window,
        controller_handle,
        "command_palette",
        &command_palette_shortcut_accelerators(shortcut),
        move || {
            open_command_palette();
            glib::Propagation::Stop
        },
    );
}

fn install_workspace_fullscreen_shortcut(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    shortcut: &str,
) {
    let window_for_shortcut = window.clone();
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_fullscreen",
        &[
            shortcut.trim().to_string(),
            DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT.into(),
        ],
        move || {
            toggle_workspace_fullscreen(
                &window_for_shortcut,
                &tabs_for_shortcut,
                active_for_shortcut.get(),
            );
            glib::Propagation::Stop
        },
    );
}

fn install_workspace_density_shortcut(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    session_persistence: &SessionPersistence,
    shortcut: &str,
) {
    let window_for_shortcut = window.clone();
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    let session_persistence = session_persistence.clone();
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_density",
        &[
            shortcut.trim().to_string(),
            DEFAULT_WORKSPACE_DENSITY_SHORTCUT.into(),
        ],
        move || {
            if cycle_active_workspace_density(
                &window_for_shortcut,
                &tabs_for_shortcut,
                active_for_shortcut.get(),
            )
            .is_some()
            {
                session_persistence.save_soon("workspace density changed");
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        },
    );
}

fn install_workspace_zoom_in_shortcut(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    session_persistence: &SessionPersistence,
    shortcut: &str,
) {
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    let window_for_shortcut = window.clone();
    let session_persistence = session_persistence.clone();
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_zoom_in",
        &zoom_in_shortcut_accelerators(shortcut),
        move || {
            if adjust_active_workspace_zoom(
                &window_for_shortcut,
                &tabs_for_shortcut,
                active_for_shortcut.get(),
                1,
            )
            .is_some()
            {
                session_persistence.save_soon("workspace zoom changed");
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        },
    );
}

fn install_workspace_zoom_out_shortcut(
    window: &adw::ApplicationWindow,
    controller_handle: &ShortcutControllerHandle,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    session_persistence: &SessionPersistence,
    shortcut: &str,
) {
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    let window_for_shortcut = window.clone();
    let session_persistence = session_persistence.clone();
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_zoom_out",
        &zoom_out_shortcut_accelerators(shortcut),
        move || {
            if adjust_active_workspace_zoom(
                &window_for_shortcut,
                &tabs_for_shortcut,
                active_for_shortcut.get(),
                -1,
            )
            .is_some()
            {
                session_persistence.save_soon("workspace zoom changed");
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        },
    );
}

fn sync_fullscreen_chrome(
    window: &adw::ApplicationWindow,
    title_widget: &gtk::Widget,
    fullscreen_button: &gtk::Button,
    is_workspace: bool,
    fullscreen_shortcut: &str,
) {
    let shortcut = shortcut_display_label(
        window,
        fullscreen_shortcut,
        DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT,
    );
    sync_workspace_fullscreen_chrome(
        window,
        title_widget,
        fullscreen_button,
        is_workspace,
        &format!("Enter fullscreen ({shortcut})"),
        &format!("Exit fullscreen ({shortcut})"),
    );
}

fn show_toast(overlay: &adw::ToastOverlay, title: &str) {
    let toast = adw::Toast::new(title);
    toast.set_timeout(2);
    overlay.add_toast(toast);
}

fn configure_window_controls(header: &adw::HeaderBar) {
    header.set_show_start_title_buttons(true);
    header.set_show_end_title_buttons(true);
}

fn collect_session(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
) -> Option<SavedSession> {
    let tabs_ref = tabs.borrow();
    let saved_tabs: Vec<SavedTab> = tabs_ref
        .iter()
        .filter_map(|tab| match &tab.content {
            TabContent::Workspace(workspace) => tab.workspace_root.as_ref().map(|root| SavedTab {
                preset: workspace.preset.clone(),
                workspace_root: root.clone(),
                custom_title: tab.custom_title.clone(),
                terminal_zoom_steps: workspace.terminal_zoom_steps,
            }),
            TabContent::LaunchDeck => None,
        })
        .collect();

    if saved_tabs.is_empty() {
        return None;
    }

    let active_index = tabs_ref
        .iter()
        .filter(|tab| matches!(tab.content, TabContent::Workspace(_)))
        .position(|tab| tab.id == active_tab_id)
        .unwrap_or(0);

    Some(SavedSession {
        tabs: saved_tabs,
        active_tab_index: active_index,
    })
}

fn workspace_runtimes(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
) -> Vec<workspace_view::WorkspaceRuntime> {
    tabs.borrow()
        .iter()
        .filter_map(|tab| match &tab.content {
            TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
            TabContent::LaunchDeck => None,
        })
        .collect()
}

fn saved_tab_for_workspace(tab: &WorkspaceTab) -> Option<SavedTab> {
    let TabContent::Workspace(workspace) = &tab.content else {
        return None;
    };
    tab.workspace_root.as_ref().map(|root| SavedTab {
        preset: workspace.preset.clone(),
        workspace_root: root.clone(),
        custom_title: tab.custom_title.clone(),
        terminal_zoom_steps: workspace.terminal_zoom_steps,
    })
}

fn next_active_index_after_detach(tab_count: usize, detached_index: usize) -> Option<usize> {
    if tab_count <= 1 || detached_index >= tab_count {
        return None;
    }
    Some(detached_index.min(tab_count - 2))
}

#[allow(clippy::too_many_arguments)]
fn detach_workspace_tab(
    origin_window_id: usize,
    _app: &adw::Application,
    tab_view: &adw::TabView,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    select_tab: &SelectTabHandle,
    add_workspace_tab: &VoidHandle,
    refresh_tab_strip: &dyn Fn(),
    _preference_store: &PreferenceStore,
    session_store: &SessionStore,
    tab_id: usize,
) -> Option<DetachPayload> {
    let page = {
        let tabs_ref = tabs.borrow();
        tab_page_for_id(tab_view, &tabs_ref, tab_id)
    }?;

    let (payload, next_active_id, should_create_replacement) = {
        let mut tabs_ref = tabs.borrow_mut();
        let detached_index = tabs_ref.iter().position(|tab| tab.id == tab_id)?;
        let saved_tab = saved_tab_for_workspace(&tabs_ref[detached_index])?;
        let tab = tabs_ref.remove(detached_index);
        clear_workspace_tab_layout_binding(&tab);
        let next_active_id = next_active_index_after_detach(tabs_ref.len() + 1, detached_index)
            .and_then(|index| tabs_ref.get(index).map(|tab| tab.id));
        (
            DetachPayload {
                origin_window_id,
                tab,
                saved_tab,
            },
            next_active_id,
            tabs_ref.is_empty(),
        )
    };

    tab_view.close_page(&page);
    refresh_tab_strip();

    if should_create_replacement {
        active_tab_id.set(0);
        if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
            add_tab();
        }
    } else if let Some(next_active_id) = next_active_id
        && let Some(select) = select_tab.borrow().as_ref()
    {
        select(next_active_id);
    }

    save_application_window_session_state(
        origin_window_id,
        tabs,
        active_tab_id.get(),
        session_store,
    );
    logging::info(format!(
        "detached workspace tab {} preset='{}' root='{}'",
        tab_id,
        payload.saved_tab.preset.name,
        payload.saved_tab.workspace_root.display()
    ));
    Some(payload)
}

#[allow(clippy::too_many_arguments)]
fn present_detached_workspace_window(
    app: &adw::Application,
    payload: DetachPayload,
    preference_store: &PreferenceStore,
    preset_store: &PresetStore,
    asset_store: &AssetStore,
    session_store: &SessionStore,
    tray_controller: &TrayController,
    options: RuntimeOptions,
) {
    let window_id = NEXT_LINUX_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
    let origin_window_id = payload.origin_window_id;
    let tab_id = payload.tab.id;
    let title = tab_display_title(&payload.tab);
    let runtime = match &payload.tab.content {
        TabContent::Workspace(workspace) => workspace.runtime.clone(),
        TabContent::LaunchDeck => return,
    };
    let preset = payload.saved_tab.preset.clone();

    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    header.add_css_class("app-headerbar");
    let title_label = gtk::Label::builder()
        .label(&title)
        .single_line_mode(true)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    let title_shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .build();
    title_shell.append(&title_label);
    header.set_title_widget(Some(&title_shell));
    let fullscreen_button = icons::labeled_button(
        "Fullscreen",
        icon_name::FULLSCREEN,
        &["flat", "titlebar-action-button"],
    );
    header.pack_end(&fullscreen_button);
    let reattach_button = icons::labeled_button(
        "Reattach",
        icon_name::RESTORE,
        &["flat", "titlebar-action-button"],
    );
    reattach_button.set_tooltip_text(Some("Reattach workspace to the main tab strip"));
    header.pack_end(&reattach_button);

    let page_shell = payload.tab.page_shell.clone();
    let window_shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    window_shell.append(&header);
    window_shell.append(&page_shell);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(&title)
        .default_width(gtk_shell::DEFAULT_WINDOW_WIDTH)
        .default_height(gtk_shell::DEFAULT_WINDOW_HEIGHT)
        .resizable(true)
        .content(&window_shell)
        .build();
    window.add_css_class("window-shell");
    apply_shell_profile(&header, &window, &preset);
    runtime.apply_appearance(
        window_uses_dark_theme(&window),
        preset.density,
        payload.saved_tab.terminal_zoom_steps,
    );
    runtime.reflow_layout();

    let detached_tabs = Rc::new(RefCell::new(vec![payload.tab]));
    {
        let tabs = detached_tabs.borrow();
        if let Some(tab) = tabs.first() {
            rebind_workspace_tab_layout(tab, &detached_tabs);
        }
    }
    save_application_window_session_state(window_id, &detached_tabs, tab_id, session_store);

    {
        let window = window.clone();
        let tabs = detached_tabs.clone();
        fullscreen_button.connect_clicked(move |_| {
            toggle_workspace_fullscreen(&window, &tabs, tab_id);
        });
    }

    {
        let session_store = session_store.clone();
        let reattaching = Rc::new(Cell::new(false));
        let reattaching_for_button = reattaching.clone();
        let app_for_reattach = app.clone();
        let window_for_reattach = window.clone();
        let window_shell_for_reattach = window_shell.clone();
        let tabs_for_reattach = detached_tabs.clone();
        let preference_store_for_reattach = preference_store.clone();
        let preset_store_for_reattach = preset_store.clone();
        let asset_store_for_reattach = asset_store.clone();
        let session_store_for_reattach = session_store.clone();
        let tray_controller_for_reattach = tray_controller.clone();
        let options_for_reattach = options.clone();
        let do_reattach = Rc::new(move || {
            let tab = tabs_for_reattach.borrow_mut().pop();
            let Some(tab) = tab else {
                return;
            };
            remove_application_window_session_state(window_id, &session_store_for_reattach);
            if tab.page_shell.parent().is_some() {
                window_shell_for_reattach.remove(&tab.page_shell);
            }
            if let Some(target) = linux_main_attach_target(Some(origin_window_id)) {
                let target_window = target.window.upgrade();
                (target.attach_workspace_tab)(tab);
                if let Some(target_window) = target_window {
                    target_window.present();
                }
            } else {
                present_with_initial_workspace(
                    &app_for_reattach,
                    preference_store_for_reattach.clone(),
                    preset_store_for_reattach.clone(),
                    asset_store_for_reattach.clone(),
                    session_store_for_reattach.clone(),
                    None,
                    None,
                    tray_controller_for_reattach.clone(),
                    options_for_reattach.clone(),
                    Some(tab),
                );
            }
            reattaching_for_button.set(true);
            window_for_reattach.close();
        });

        {
            let do_reattach = do_reattach.clone();
            reattach_button.connect_clicked(move |_| {
                do_reattach();
            });
        }

        let popover = context_menu::popover(&title_shell);
        let menu = context_menu::menu_box();
        let menu_reattach_button = context_menu::action_button("Reattach", None);
        {
            let do_reattach = do_reattach.clone();
            let popover = popover.clone();
            menu_reattach_button.connect_clicked(move |_| {
                popover.popdown();
                do_reattach();
            });
        }
        menu.append(&menu_reattach_button);
        popover.set_child(Some(&menu));
        let right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let popover = popover.clone();
            right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                context_menu::popup_at(&popover, x, y);
            });
        }
        header.add_controller(right_click);

        let title_right_click = gtk::GestureClick::builder()
            .button(3)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        {
            let popover = popover.clone();
            title_right_click.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                context_menu::popup_at(&popover, x, y);
            });
        }
        title_shell.add_controller(title_right_click);

        let force_close = Rc::new(Cell::new(false));
        let force_close_for_confirm = force_close.clone();
        let runtime_for_close = runtime.clone();
        let tabs_for_close = detached_tabs.clone();
        let window_shell_for_close = window_shell.clone();
        window.connect_close_request(move |window| {
            if reattaching.get() {
                return glib::Propagation::Proceed;
            }
            if force_close.replace(false) {
                finalize_detached_workspace_close(
                    window_id,
                    &window_shell_for_close,
                    &tabs_for_close,
                    &runtime_for_close,
                    &session_store,
                );
                return glib::Propagation::Proceed;
            }

            if runtime_for_close.has_active_processes() {
                let window = window.clone();
                let window_for_confirm = window.clone();
                let force_close = force_close_for_confirm.clone();
                confirm_destructive_action(
                    &window,
                    "Close Detached Workspace?",
                    "Running terminal sessions in this detached workspace will be terminated.",
                    "Close",
                    move || {
                        force_close.set(true);
                        window_for_confirm.close();
                    },
                );
                return glib::Propagation::Stop;
            }

            finalize_detached_workspace_close(
                window_id,
                &window_shell_for_close,
                &tabs_for_close,
                &runtime_for_close,
                &session_store,
            );
            glib::Propagation::Proceed
        });
    }

    window.present();
    logging::info(format!(
        "presented detached workspace window {} for tab {}",
        window_id, tab_id
    ));
}

fn finalize_detached_workspace_close(
    window_id: usize,
    window_shell: &gtk::Box,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    runtime: &workspace_view::WorkspaceRuntime,
    session_store: &SessionStore,
) {
    remove_application_window_session_state(window_id, session_store);

    let detached_tabs = tabs.borrow_mut().drain(..).collect::<Vec<_>>();
    for tab in detached_tabs {
        clear_workspace_tab_layout_binding(&tab);
        if let Some(parent) = tab.page_shell.parent()
            && let Ok(parent_box) = parent.downcast::<gtk::Box>()
            && parent_box == *window_shell
        {
            parent_box.remove(&tab.page_shell);
        }
    }

    runtime.terminate_all("closing detached workspace window");
}

#[allow(clippy::too_many_arguments)]
fn finish_tab_close(
    view: &adw::TabView,
    page: &adw::TabPage,
    tab_id: usize,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: &Rc<Cell<usize>>,
    select_tab: &SelectTabHandle,
    add_workspace_tab: &VoidHandle,
    suppress_empty_replacement: &Cell<bool>,
    session_persistence: &SessionPersistence,
) {
    let (runtime, next_active_id, should_create_replacement) = {
        let mut tabs = tabs.borrow_mut();
        let Some(index) = tabs.iter().position(|tab| tab.id == tab_id) else {
            view.close_page_finish(page, false);
            return;
        };

        let removed = tabs.remove(index);
        let runtime = match removed.content {
            TabContent::Workspace(workspace) => Some(workspace.runtime),
            TabContent::LaunchDeck => None,
        };
        let next_active_id = if tabs.is_empty() {
            None
        } else if active_tab_id.get() == tab_id {
            tabs.get(index).or_else(|| tabs.last()).map(|tab| tab.id)
        } else {
            Some(active_tab_id.get())
        };

        (runtime, next_active_id, tabs.is_empty())
    };

    if let Some(runtime) = runtime {
        runtime.terminate_all("closing workspace tab");
    }
    view.close_page_finish(page, true);
    logging::info(format!("closed workspace tab {}", tab_id));

    if should_create_replacement {
        active_tab_id.set(0);
        if !suppress_empty_replacement.get()
            && let Some(add_tab) = add_workspace_tab.borrow().as_ref()
        {
            add_tab();
        }
        session_persistence.save_now("last workspace tab closed");
        return;
    }

    if let Some(next_active_id) = next_active_id
        && let Some(select) = select_tab.borrow().as_ref()
    {
        select(next_active_id);
    }
    session_persistence.save_now("workspace tab closed");
}

fn has_active_workspace_processes(tabs: &Rc<RefCell<Vec<WorkspaceTab>>>) -> bool {
    tabs.borrow().iter().any(|tab| match &tab.content {
        TabContent::Workspace(workspace) => workspace.runtime.has_active_processes(),
        TabContent::LaunchDeck => false,
    })
}

fn linux_session_registry() -> &'static Mutex<LinuxSessionRegistry> {
    LINUX_SESSION_REGISTRY.get_or_init(|| Mutex::new(LinuxSessionRegistry::default()))
}

fn persist_linux_session_registry(registry: &LinuxSessionRegistry, session_store: &SessionStore) {
    let Some(session) = flatten_window_sessions(
        registry
            .windows
            .iter()
            .map(|(window_id, session)| (*window_id, session)),
        registry.active_window_id,
    ) else {
        session_store.clear();
        return;
    };

    session_store.save(&session);
}

fn save_application_window_session_state(
    window_id: usize,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    session_store: &SessionStore,
) {
    let registry_lock = linux_session_registry().lock();
    let Ok(mut registry) = registry_lock else {
        logging::error("Linux session registry lock poisoned while saving");
        return;
    };

    if let Some(session) = collect_session(tabs, active_tab_id) {
        logging::info(format!(
            "saving window {} session with {} workspace tab(s)",
            window_id,
            session.tabs.len()
        ));
        registry.windows.insert(window_id, session);
        registry.active_window_id = Some(window_id);
    } else {
        logging::info(format!(
            "removing window {} from session registry because it has no workspace tabs",
            window_id
        ));
        registry.windows.remove(&window_id);
        if registry.active_window_id == Some(window_id) {
            registry.active_window_id = registry.windows.keys().next().copied();
        }
    }

    persist_linux_session_registry(&registry, session_store);
}

fn remove_application_window_session_state(window_id: usize, session_store: &SessionStore) {
    let registry_lock = linux_session_registry().lock();
    let Ok(mut registry) = registry_lock else {
        logging::error("Linux session registry lock poisoned while removing");
        return;
    };

    registry.windows.remove(&window_id);
    if registry.active_window_id == Some(window_id) {
        registry.active_window_id = registry.windows.keys().next().copied();
    }
    persist_linux_session_registry(&registry, session_store);
}

fn force_quit_application(
    window_id: usize,
    window: &adw::ApplicationWindow,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    session_store: &SessionStore,
) {
    logging::info("force quitting application window");
    save_application_window_session_state(window_id, tabs, active_tab_id, session_store);
    for runtime in workspace_runtimes(tabs) {
        runtime.terminate_all("force quitting application window");
    }

    window.set_visible(false);
    if let Some(app) = window.application()
        && let Ok(app) = app.downcast::<adw::Application>()
    {
        app.quit();
    } else {
        window.close();
    }
}

fn confirm_destructive_action<F>(
    window: &adw::ApplicationWindow,
    heading: &str,
    body: &str,
    confirm_label: &str,
    on_confirm: F,
) where
    F: Fn() + 'static,
{
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(window)
        .heading(heading)
        .body(body)
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", confirm_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dialog, response| {
        if response == "confirm" {
            on_confirm();
        }
        dialog.close();
    });

    dialog.present();
}

fn confirm_tab_close<F>(
    window: &adw::ApplicationWindow,
    heading: &str,
    body: &str,
    confirm_label: &str,
    on_response: F,
) where
    F: Fn(bool) + 'static,
{
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(window)
        .heading(heading)
        .body(body)
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", confirm_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dialog, response| {
        on_response(response == "confirm");
        dialog.close();
    });

    dialog.present();
}

fn prompt_session_resume<F, G, H>(
    window: &adw::ApplicationWindow,
    saved_session: &SavedSession,
    warning: Option<&str>,
    on_resume: F,
    on_resume_shells: G,
    on_start_fresh: H,
) where
    F: Fn() + 'static,
    G: Fn() + 'static,
    H: Fn() + 'static,
{
    let body = if let Some(warning) = warning {
        format!(
            "TerminalTiler found {} saved workspace(s). You can rerun commands, reopen the same layouts as plain shells, or start fresh.\n\n{}",
            saved_session.tabs.len(),
            warning
        )
    } else {
        format!(
            "TerminalTiler found {} saved workspace(s). You can rerun commands, reopen the same layouts as plain shells, or start fresh.",
            saved_session.tabs.len()
        )
    };

    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(window)
        .heading("Resume Previous Session?")
        .body(body)
        .build();

    dialog.add_response("fresh", "Start Fresh");
    dialog.add_response("shells", "Resume As Shells");
    dialog.add_response("resume", "Resume And Rerun");
    dialog.set_response_appearance("resume", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("shells"));
    dialog.set_close_response("fresh");

    dialog.connect_response(None, move |dialog, response| {
        match response {
            "resume" => on_resume(),
            "shells" => on_resume_shells(),
            _ => on_start_fresh(),
        }
        dialog.close();
    });

    dialog.present();
}

fn show_startup_notice(window: &adw::ApplicationWindow, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(window)
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.connect_response(None, move |dialog, _| {
        dialog.close();
    });
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::{
        VOICE_AUDIO_FLUSH_INTERVAL, VoiceHotkeyWarmGate, VoiceWarmState, WorkspaceTab,
        apply_voice_listening_started, move_item_to_position, move_tab_to_position,
        next_active_index_after_detach, preview_index_for_pointer, voice_hotkey_warm_gate,
    };
    use crate::voice::{VoiceActivationMode, VoicePreferences};
    use std::cell::Cell;

    fn tab_ids(tabs: &[usize]) -> Vec<usize> {
        tabs.to_vec()
    }

    #[test]
    fn reorders_tab_before_target() {
        let mut tabs = vec![1, 2, 3];

        let moved = move_item_to_position(&mut tabs, 2, 0);

        assert!(moved);
        assert_eq!(tab_ids(&tabs), vec![3, 1, 2]);
    }

    #[test]
    fn reorders_tab_after_target() {
        let mut tabs = vec![1, 2, 3];

        let moved = move_item_to_position(&mut tabs, 0, 2);

        assert!(moved);
        assert_eq!(tab_ids(&tabs), vec![2, 3, 1]);
    }

    #[test]
    fn ignores_reorder_when_moving_to_same_position() {
        let mut tabs = vec![1, 2, 3];

        let moved = move_item_to_position(&mut tabs, 1, 1);

        assert!(!moved);
        assert_eq!(tab_ids(&tabs), vec![1, 2, 3]);
    }

    #[test]
    fn ignores_reorder_for_unknown_tab() {
        let mut tabs = vec![1, 2, 3];

        let moved = move_item_to_position(&mut tabs, 99, 0);

        assert!(!moved);
        assert_eq!(tab_ids(&tabs), vec![1, 2, 3]);
    }

    #[test]
    fn ignores_reorder_for_unknown_tab_id() {
        let mut tabs: Vec<WorkspaceTab> = Vec::new();

        let moved = move_tab_to_position(&mut tabs, 99, 0);

        assert!(!moved);
    }

    #[test]
    fn voice_audio_flush_cadence_targets_low_latency_chunks() {
        assert_eq!(VOICE_AUDIO_FLUSH_INTERVAL.as_millis(), 250);
    }

    #[test]
    fn voice_hotkey_waits_until_background_warm_is_ready() {
        assert_eq!(
            voice_hotkey_warm_gate(VoiceWarmState::Cold),
            VoiceHotkeyWarmGate::RequestWarm
        );
        assert_eq!(
            voice_hotkey_warm_gate(VoiceWarmState::Warming),
            VoiceHotkeyWarmGate::WaitForWarm
        );
        assert_eq!(
            voice_hotkey_warm_gate(VoiceWarmState::Ready),
            VoiceHotkeyWarmGate::StartCapture
        );
        assert_eq!(
            voice_hotkey_warm_gate(VoiceWarmState::Failed),
            VoiceHotkeyWarmGate::ReportFailure
        );
    }

    #[test]
    fn pending_push_to_talk_release_does_not_reopen_listening_after_start_ack() {
        let voice_starting = Cell::new(true);
        let voice_listening = Cell::new(false);
        let voice_stopping = Cell::new(true);

        apply_voice_listening_started(&voice_starting, &voice_listening, &voice_stopping);

        assert!(!voice_starting.get());
        assert!(!voice_listening.get());
    }

    #[test]
    fn start_ack_marks_voice_listening_when_no_stop_is_pending() {
        let voice_starting = Cell::new(true);
        let voice_listening = Cell::new(false);
        let voice_stopping = Cell::new(false);

        apply_voice_listening_started(&voice_starting, &voice_listening, &voice_stopping);

        assert!(!voice_starting.get());
        assert!(voice_listening.get());
    }

    #[test]
    fn push_to_talk_registers_linux_global_hotkey_for_terminal_focus() {
        let mut voice = VoicePreferences {
            enabled: true,
            activation_mode: VoiceActivationMode::PushToTalk,
            prefer_global_hotkey: false,
            ..VoicePreferences::default()
        };

        assert!(super::should_register_linux_voice_global_hotkey(&voice));

        voice.activation_mode = VoiceActivationMode::Toggle;
        assert!(!super::should_register_linux_voice_global_hotkey(&voice));

        voice.prefer_global_hotkey = true;
        assert!(super::should_register_linux_voice_global_hotkey(&voice));

        voice.enabled = false;
        assert!(!super::should_register_linux_voice_global_hotkey(&voice));
    }

    #[test]
    fn preview_index_is_before_first_tab_when_pointer_is_left_of_first_midpoint() {
        let slots = vec![(0.0, 100.0), (110.0, 100.0)];

        assert_eq!(preview_index_for_pointer(&slots, 20.0), 0);
    }

    #[test]
    fn preview_index_moves_between_tabs_after_crossing_first_midpoint() {
        let slots = vec![(0.0, 100.0), (110.0, 100.0)];

        assert_eq!(preview_index_for_pointer(&slots, 70.0), 1);
    }

    #[test]
    fn preview_index_stays_before_second_tab_on_left_half() {
        let slots = vec![(0.0, 100.0), (110.0, 100.0)];

        assert_eq!(preview_index_for_pointer(&slots, 140.0), 1);
    }

    #[test]
    fn preview_index_is_after_last_tab_when_pointer_is_past_all_midpoints() {
        let slots = vec![(0.0, 100.0), (110.0, 100.0)];

        assert_eq!(preview_index_for_pointer(&slots, 190.0), 2);
    }

    #[test]
    fn detach_next_active_selects_following_tab_when_available() {
        assert_eq!(next_active_index_after_detach(3, 0), Some(0));
        assert_eq!(next_active_index_after_detach(3, 1), Some(1));
    }

    #[test]
    fn detach_next_active_selects_previous_for_last_tab() {
        assert_eq!(next_active_index_after_detach(3, 2), Some(1));
    }

    #[test]
    fn detach_next_active_is_none_for_only_or_unknown_tab() {
        assert_eq!(next_active_index_after_detach(1, 0), None);
        assert_eq!(next_active_index_after_detach(3, 3), None);
    }
}
