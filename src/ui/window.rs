use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use glib::value::ToValue;
use gtk::{gdk, gio, glib, pango};

use crate::logging;
use crate::model::assets::RestoreLaunchMode;
use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
use crate::services::session_restore::{session_for_restore_mode, shell_only_session};
use crate::storage::asset_store::AssetStore;
use crate::storage::preference_store::{AppPreferences, PreferenceStore};
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
use crate::terminal::session::clamp_terminal_zoom_steps;
use crate::tray::TrayController;
use crate::ui::{assets_manager, command_palette, launch_screen, settings_dialog, workspace_view};

type SelectTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type TabActionHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type RenameTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, Option<String>)>>>>;
type ReorderTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, usize)>>>>;
type ShowWorkspaceHandle = Rc<RefCell<Option<Box<dyn Fn(usize, WorkspacePreset, PathBuf)>>>>;
type VoidHandle = Rc<RefCell<Option<Box<dyn Fn()>>>>;
type ShortcutControllerHandle = Rc<RefCell<Option<gtk::ShortcutController>>>;
type TabStripControllerHandle = Rc<RefCell<TabStripController>>;

const DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT: &str = "F11";
const DEFAULT_WORKSPACE_DENSITY_SHORTCUT: &str = "<Ctrl><Shift>D";
const DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT: &str = "<Ctrl>plus";
const DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT: &str = "<Ctrl>minus";
const DEFAULT_COMMAND_PALETTE_SHORTCUT: &str = "<Ctrl><Shift>P";

fn apply_theme_mode(window: &adw::ApplicationWindow, theme: &ThemeMode) {
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

fn apply_window_density(window: &adw::ApplicationWindow, density: Option<ApplicationDensity>) {
    window.remove_css_class("profile-comfortable");
    window.remove_css_class("profile-standard");
    window.remove_css_class("profile-compact");

    if let Some(density) = density {
        window.add_css_class(density.css_class());
    }
}

fn resolved_theme_uses_dark_palette(theme: ThemeMode) -> bool {
    match theme {
        ThemeMode::System => adw::StyleManager::default().is_dark(),
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
    }
}

fn window_uses_dark_theme(window: &adw::ApplicationWindow) -> bool {
    window.has_css_class("theme-dark")
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
        }
    }
}

#[derive(Clone)]
struct TabStripItem {
    tab_id: usize,
    shell: gtk::Box,
    title_label: gtk::Label,
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
) {
    let preference_store = Rc::new(preference_store);
    let preset_store = Rc::new(preset_store);
    let asset_store = Rc::new(asset_store);
    let session_store = Rc::new(session_store);

    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    header.set_centering_policy(adw::CenteringPolicy::Loose);
    header.add_css_class("app-headerbar");

    let tab_view = adw::TabView::builder().hexpand(true).vexpand(true).build();
    let title = TitleChrome::new(&tab_view);
    title.root.add_css_class("app-title-handle");
    header.set_title_widget(Some(&title.root));

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&tab_view));

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
    let close_to_background_notice_button = gtk::Button::with_label("Open Settings");
    close_to_background_notice_button.add_css_class("pill-button");
    close_to_background_notice_button.add_css_class("suggested-action");
    close_to_background_notice_button.set_valign(gtk::Align::Center);
    close_to_background_notice_row.append(&close_to_background_notice_button);
    close_to_background_notice.set_child(Some(&close_to_background_notice_row));

    let window_shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    window_shell.append(&header);
    window_shell.append(&close_to_background_notice);
    window_shell.append(&toast_overlay);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("TerminalTiler")
        .default_width(1280)
        .default_height(680)
        .resizable(true)
        .content(&window_shell)
        .build();
    window.add_css_class("window-shell");

    let back_button = gtk::Button::with_label("Templates");
    back_button.add_css_class("flat");
    back_button.add_css_class("titlebar-action-button");
    back_button.set_visible(false);
    header.pack_start(&back_button);

    let fullscreen_button = gtk::Button::with_label("Fullscreen");
    fullscreen_button.add_css_class("flat");
    fullscreen_button.add_css_class("titlebar-action-button");
    fullscreen_button.set_tooltip_text(Some("Enter fullscreen"));
    fullscreen_button.set_visible(false);
    header.pack_end(&fullscreen_button);

    let settings_button = gtk::Button::from_icon_name("preferences-system-symbolic");
    if let Some(img) = settings_button.first_child() {
        let _ = img.pango_context();
    }
    settings_button.add_css_class("flat");
    settings_button.add_css_class("titlebar-action-button");
    settings_button.add_css_class("titlebar-icon-button");
    settings_button.set_tooltip_text(Some("Application settings"));
    header.pack_end(&settings_button);

    let assets_button = gtk::Button::from_icon_name("folder-saved-search-symbolic");
    if let Some(img) = assets_button.first_child() {
        let _ = img.pango_context();
    }
    assets_button.add_css_class("flat");
    assets_button.add_css_class("titlebar-action-button");
    assets_button.add_css_class("titlebar-icon-button");
    assets_button.set_tooltip_text(Some("Assets manager"));
    header.pack_end(&assets_button);

    let tabs = Rc::new(RefCell::new(Vec::<WorkspaceTab>::new()));
    let next_tab_id = Rc::new(Cell::new(1usize));
    let active_tab_id = Rc::new(Cell::new(0usize));
    let select_tab: SelectTabHandle = Rc::new(RefCell::new(None));
    let close_tab: TabActionHandle = Rc::new(RefCell::new(None));
    let request_tab_rename: TabActionHandle = Rc::new(RefCell::new(None));
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
    let quit_requested = Rc::new(Cell::new(false));
    let force_quit_requested = Rc::new(Cell::new(false));
    let tab_strip_controller = create_tab_strip_controller(
        &title.tabs_box,
        select_tab.clone(),
        close_tab.clone(),
        request_tab_rename.clone(),
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
        let sync_selected_tab: Rc<dyn Fn(usize)> = Rc::new(move |tab_id| {
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
            prompt_tab_rename(&window_for_rename, &current_title, move |requested_title| {
                if let Some(rename) = apply_rename_handle.borrow().as_ref() {
                    rename(tab_id, requested_title);
                }
            });
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
        });
    }

    {
        let tabs_for_workspace = tabs.clone();
        let tab_view_for_workspace = tab_view.clone();
        let select_for_workspace = select_tab.clone();
        let refresh_tab_strip_for_workspace = refresh_tab_strip.clone();
        let asset_store = asset_store.clone();
        let preference_store_for_workspace = preference_store.clone();

        *show_workspace_in_tab.borrow_mut() =
            Some(Box::new(move |tab_id, preset, workspace_root| {
                let terminal_zoom_steps = 0;
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
                        let tabs_for_workspace = tabs_for_workspace.clone();
                        Rc::new(move |next_layout| {
                            let mut tabs = tabs_for_workspace.borrow_mut();
                            let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                                return;
                            };
                            if let TabContent::Workspace(workspace) = &mut tab.content {
                                workspace.preset.layout = next_layout;
                            }
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
        current_density_shortcut.borrow().as_str(),
    );

    install_workspace_zoom_in_shortcut(
        &window,
        &zoom_in_shortcut_controller,
        &tabs,
        &active_tab_id,
        current_zoom_in_shortcut.borrow().as_str(),
    );

    install_workspace_zoom_out_shortcut(
        &window,
        &zoom_out_shortcut_controller,
        &tabs,
        &active_tab_id,
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
                        move |shortcut| {
                            preference_store.save_workspace_density_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_density_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
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
                        move |shortcut| {
                            preference_store.save_workspace_zoom_in_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_zoom_in_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
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
                        move |shortcut| {
                            preference_store.save_workspace_zoom_out_shortcut(&shortcut);
                            current_shortcut.replace(shortcut.clone());
                            install_workspace_zoom_out_shortcut(
                                &window,
                                &controller_handle,
                                &tabs,
                                &active_tab_id,
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
                                &defaults.workspace_density_shortcut,
                            );
                            install_workspace_zoom_in_shortcut(
                                &window,
                                &zoom_in_controller,
                                &tabs,
                                &active_tab_id,
                                &defaults.workspace_zoom_in_shortcut,
                            );
                            install_workspace_zoom_out_shortcut(
                                &window,
                                &zoom_out_controller,
                                &tabs,
                                &active_tab_id,
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

    let open_command_palette: Rc<dyn Fn()> = {
        let window = window.clone();
        let tabs = tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let add_workspace_tab = add_workspace_tab.clone();
        let select_tab = select_tab.clone();
        let request_tab_rename = request_tab_rename.clone();
        let open_settings_dialog = open_settings_dialog.clone();
        let open_assets_manager = open_assets_manager.clone();
        Rc::new(move || {
            let snapshot = tabs.borrow().clone();
            let active_id = active_tab_id.get();
            let mut actions = vec![
                command_palette::PaletteAction {
                    title: "Open Settings".into(),
                    subtitle: "Application preferences and shortcuts.".into(),
                    on_activate: Rc::new({
                        let open_settings_dialog = open_settings_dialog.clone();
                        move || open_settings_dialog()
                    }),
                },
                command_palette::PaletteAction {
                    title: "Open Assets Manager".into(),
                    subtitle: "Edit global or workspace scoped assets.".into(),
                    on_activate: Rc::new({
                        let open_assets_manager = open_assets_manager.clone();
                        move || open_assets_manager()
                    }),
                },
                command_palette::PaletteAction {
                    title: "New Tab".into(),
                    subtitle: "Open a fresh launch deck tab.".into(),
                    on_activate: Rc::new({
                        let add_workspace_tab = add_workspace_tab.clone();
                        move || {
                            if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
                                add_tab();
                            }
                        }
                    }),
                },
            ];

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
                actions.push(command_palette::PaletteAction {
                    title: "Rename Active Tab".into(),
                    subtitle: "Set a custom workspace title.".into(),
                    on_activate: Rc::new({
                        let request_tab_rename = request_tab_rename.clone();
                        move || {
                            if let Some(rename) = request_tab_rename.borrow().as_ref() {
                                rename(active_id);
                            }
                        }
                    }),
                });

                if let TabContent::Workspace(workspace) = &active_tab.content {
                    let runtime_for_alert_focus = workspace.runtime.clone();
                    actions.push(command_palette::PaletteAction {
                        title: "Focus Next Alert".into(),
                        subtitle: "Jump to the next unread workspace alert.".into(),
                        on_activate: Rc::new(move || {
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
                        }),
                    });

                    let runtime_for_add_web_tile = workspace.runtime.clone();
                    actions.push(command_palette::PaletteAction {
                        title: "Add Web Tile".into(),
                        subtitle: "Insert a new browser tile beside the focused pane.".into(),
                        on_activate: Rc::new(move || {
                            let _ = runtime_for_add_web_tile.add_web_tile();
                        }),
                    });

                    for runbook in workspace
                        .assets
                        .runbooks
                        .iter()
                        .filter(|runbook| runbook.variables.is_empty())
                    {
                        let runbook = runbook.clone();
                        let runtime = workspace.runtime.clone();
                        actions.push(command_palette::PaletteAction {
                            title: format!("Run Runbook: {}", runbook.name),
                            subtitle: if runbook.description.trim().is_empty() {
                                runbook.target.label()
                            } else {
                                runbook.description.clone()
                            },
                            on_activate: Rc::new(move || {
                                if let Ok(resolved) = crate::services::runbooks::resolve_runbook(
                                    &runbook,
                                    &HashMap::new(),
                                    &runtime.tile_specs(),
                                ) {
                                    runtime.run_runbook(&resolved);
                                }
                            }),
                        });
                    }
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

    if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
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
        let current_close_to_background = current_close_to_background.clone();
        let quit_requested = quit_requested.clone();
        let force_quit_requested = force_quit_requested.clone();
        let tray_controller = tray_controller.clone();
        window.connect_close_request(move |window| {
            if force_quit_requested.replace(false) {
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
            let runtimes = workspace_runtimes(&tabs_for_save);
            persist_application_session(&tabs_for_save, active_for_save.get(), &session_store);

            for runtime in runtimes {
                runtime.terminate_all("closing application window");
            }
            glib::Propagation::Proceed
        });
    }

    window.present();

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
            };
            match restore_mode {
                RestoreLaunchMode::Prompt => prompt_session_resume(
                    &window_for_restore,
                    &saved_session,
                    warning.as_deref(),
                    {
                        let restore_context = restore_context.clone();
                        let resume_session = resume_session.clone();
                        move || {
                            restore_saved_session(&restore_context, resume_session.clone(), true);
                        }
                    },
                    {
                        let restore_context = restore_context.clone();
                        let shell_session = shell_only_session(&resume_session);
                        move || {
                            restore_saved_session(&restore_context, shell_session.clone(), true);
                        }
                    },
                    move || {
                        session_store_for_restore.clear();
                    },
                ),
                RestoreLaunchMode::RerunStartupCommands => {
                    if let Some(session) = session_for_restore_mode(
                        &resume_session,
                        RestoreLaunchMode::RerunStartupCommands,
                    ) {
                        restore_saved_session(&restore_context, session, true);
                    }
                }
                RestoreLaunchMode::ShellOnly => {
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

fn build_tab_drag_preview(title: &str, is_active: bool) -> gtk::Box {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .build();
    shell.add_css_class("app-tab-shell");
    shell.add_css_class(if is_active {
        "is-active"
    } else {
        "is-inactive"
    });
    shell.add_css_class("app-tab-drag-icon");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    icon.set_valign(gtk::Align::Center);
    icon.add_css_class("app-tab-icon");
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .single_line_mode(true)
        .ellipsize(pango::EllipsizeMode::End)
        .width_chars(14)
        .max_width_chars(14)
        .build();
    label.add_css_class("app-tab-title");
    content.append(&icon);
    content.append(&label);
    shell.append(&content);
    shell
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
    ) -> Self {
        Self {
            tabs_box,
            items: Vec::new(),
            order: Vec::new(),
            drag_state: None,
            select_tab,
            close_tab,
            request_tab_rename,
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
                item.title_label.set_label(&title);
                item.shell.set_tooltip_text(Some(&tab.subtitle));
                item.shell.remove_css_class("is-active");
                item.shell.remove_css_class("is-inactive");
                item.shell.add_css_class(if tab.id == active_tab_id {
                    "is-active"
                } else {
                    "is-inactive"
                });
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
        let shell = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        shell.add_css_class("app-tab-shell");
        shell.add_css_class("is-inactive");

        let select_button = gtk::Button::new();
        select_button.add_css_class("app-tab-select");
        select_button.set_hexpand(true);
        select_button.set_focus_on_click(false);

        let select_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();
        let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
        icon.set_valign(gtk::Align::Center);
        icon.add_css_class("app-tab-icon");
        let title_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .single_line_mode(true)
            .ellipsize(pango::EllipsizeMode::End)
            .width_chars(14)
            .max_width_chars(14)
            .build();
        title_label.add_css_class("app-tab-title");
        select_row.append(&icon);
        select_row.append(&title_label);
        select_button.set_child(Some(&select_row));

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
        shell.append(&select_button);

        let close_button = gtk::Button::from_icon_name("window-close-symbolic");
        if let Some(img) = close_button.first_child() {
            let _ = img.pango_context();
        }
        close_button.add_css_class("app-tab-close");
        close_button.set_focus_on_click(false);
        let close_handle = self.close_tab.clone();
        close_button.connect_clicked(move |_| {
            if let Some(close) = close_handle.borrow().as_ref() {
                close(tab_id);
            }
        });
        shell.append(&close_button);

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

        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&(tab_id as u32).to_value()))
        });
        let controller_for_begin = controller.clone();
        let shell_for_begin = shell.clone();
        let label_for_begin = title_label.clone();
        drag_source.connect_drag_begin(move |_, drag| {
            let title = label_for_begin.text().to_string();
            let is_active = shell_for_begin.has_css_class("is-active");
            controller_for_begin
                .borrow_mut()
                .begin_drag(tab_id, drag, &title, is_active);
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
            title_label,
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

    fn begin_drag(&mut self, tab_id: usize, drag: &gdk::Drag, title: &str, is_active: bool) {
        if self.drag_state.is_some() {
            return;
        }

        let Some(item) = self.find_item(tab_id) else {
            return;
        };
        let Some(origin_index) = self.order.iter().position(|id| *id == tab_id) else {
            return;
        };

        let icon = gtk::DragIcon::for_drag(drag);
        let preview = build_tab_drag_preview(title, is_active);
        icon.set_child(Some(&preview));

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

fn create_tab_strip_controller(
    tabs_box: &gtk::Box,
    select_tab: SelectTabHandle,
    close_tab: TabActionHandle,
    request_tab_rename: TabActionHandle,
    reorder_tab: ReorderTabHandle,
) -> TabStripControllerHandle {
    let controller = Rc::new(RefCell::new(TabStripController::new(
        tabs_box.clone(),
        select_tab,
        close_tab,
        request_tab_rename,
    )));

    let drop_target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    {
        let controller_for_motion = controller.clone();
        drop_target.connect_motion(move |_, x, _| {
            if controller_for_motion.borrow().drag_state.is_none() {
                return gdk::DragAction::empty();
            }
            controller_for_motion.borrow_mut().update_preview_for_x(x);
            gdk::DragAction::MOVE
        });
    }
    {
        let controller_for_drop = controller.clone();
        let reorder_handle = reorder_tab.clone();
        drop_target.connect_drop(move |_, value, x, _| {
            let drop_result = {
                let mut controller = controller_for_drop.borrow_mut();
                controller.prepare_drop(value, x)
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
    tabs_box.add_controller(drop_target);

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
                apply_theme_mode(&theme_preview_window, &theme);
            }),
            on_density_preview: Rc::new({
                move |density| {
                    apply_window_density(&density_preview_window, Some(density));
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
                let tabs = context.tabs.clone();
                Rc::new(move |next_layout| {
                    let mut tabs = tabs.borrow_mut();
                    let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                        return;
                    };
                    if let TabContent::Workspace(workspace) = &mut tab.content {
                        workspace.preset.layout = next_layout;
                    }
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
}

#[derive(Clone)]
struct TitleChrome {
    root: gtk::Box,
    tabs_box: gtk::Box,
    add_button: gtk::Button,
}

impl TitleChrome {
    fn new(_tab_view: &adw::TabView) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();

        let tabs_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();
        tabs_box.add_css_class("app-tab-strip");

        let add_button = gtk::Button::with_label("+");
        add_button.add_css_class("flat");
        add_button.add_css_class("app-tab-add");
        root.append(&tabs_box);
        root.append(&add_button);

        Self {
            root,
            tabs_box,
            add_button,
        }
    }
}

#[allow(deprecated)]
fn prompt_tab_rename<F>(window: &adw::ApplicationWindow, current_title: &str, on_submit: F)
where
    F: Fn(Option<String>) + 'static,
{
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Rename Workspace")
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Apply", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let body = gtk::Label::builder()
        .label("Enter a new workspace tab name. Leave it blank to restore automatic naming.")
        .wrap(true)
        .halign(gtk::Align::Start)
        .build();
    let entry = gtk::Entry::builder()
        .hexpand(true)
        .text(current_title)
        .activates_default(true)
        .build();
    content.append(&body);
    content.append(&entry);

    let on_submit = Rc::new(on_submit);
    let entry_for_response = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let requested_title = entry_for_response.text().trim().to_string();
            if requested_title.is_empty() {
                on_submit(None);
            } else {
                on_submit(Some(requested_title));
            }
        }
        dialog.close();
    });

    dialog.present();
    entry.grab_focus();
    entry.set_position(-1);
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

    apply_theme_mode(window, &preset.theme);

    apply_window_density(window, Some(preset.density));
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
    apply_theme_mode(window, &preferences.default_theme);
    apply_window_density(window, Some(preferences.default_density));
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
    if active_tab_is_workspace(tabs, active_tab_id) || window.is_fullscreen() {
        window.set_fullscreened(!window.is_fullscreen());
    }
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
    apply_window_density(window, Some(next_density));
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
    shortcut: &str,
) {
    let window_for_shortcut = window.clone();
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
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
    shortcut: &str,
) {
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    let window_for_shortcut = window.clone();
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
    shortcut: &str,
) {
    let tabs_for_shortcut = tabs.clone();
    let active_for_shortcut = active_tab_id.clone();
    let window_for_shortcut = window.clone();
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
    if !is_workspace {
        title_widget.set_visible(true);
        fullscreen_button.set_visible(false);
        if window.is_fullscreen() {
            window.set_fullscreened(false);
        }
        return;
    }

    let is_fullscreen = window.is_fullscreen();
    title_widget.set_visible(!is_fullscreen);
    fullscreen_button.set_visible(true);
    if is_fullscreen {
        fullscreen_button.set_label("Exit Fullscreen");
        fullscreen_button.set_tooltip_text(Some(&format!(
            "Exit fullscreen ({})",
            shortcut_display_label(
                window,
                fullscreen_shortcut,
                DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT
            )
        )));
    } else {
        fullscreen_button.set_label("Fullscreen");
        fullscreen_button.set_tooltip_text(Some(&format!(
            "Enter fullscreen ({})",
            shortcut_display_label(
                window,
                fullscreen_shortcut,
                DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT
            )
        )));
    }
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
        return;
    }

    if let Some(next_active_id) = next_active_id
        && let Some(select) = select_tab.borrow().as_ref()
    {
        select(next_active_id);
    }
}

fn has_active_workspace_processes(tabs: &Rc<RefCell<Vec<WorkspaceTab>>>) -> bool {
    tabs.borrow().iter().any(|tab| match &tab.content {
        TabContent::Workspace(workspace) => workspace.runtime.has_active_processes(),
        TabContent::LaunchDeck => false,
    })
}

fn persist_application_session(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    session_store: &SessionStore,
) {
    if let Some(session) = collect_session(tabs, active_tab_id) {
        logging::info(format!(
            "saving session with {} workspace tab(s)",
            session.tabs.len()
        ));
        session_store.save(&session);
    } else {
        logging::info("no workspace tabs to save, clearing session");
        session_store.clear();
    }
}

fn force_quit_application(
    window: &adw::ApplicationWindow,
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    active_tab_id: usize,
    session_store: &SessionStore,
) {
    logging::info("force quitting application window");
    persist_application_session(tabs, active_tab_id, session_store);
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
        WorkspaceTab, move_item_to_position, move_tab_to_position, preview_index_for_pointer,
    };

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
}
