use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::app::tray::TrayController;
use crate::logging;
use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset};
use crate::storage::preference_store::{AppPreferences, PreferenceStore};
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
use crate::terminal::session::clamp_terminal_zoom_steps;
use crate::ui::{launch_screen, settings_dialog, workspace_view};

type SelectTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type TabActionHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type ReorderTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, usize, bool)>>>>;
type RenameTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, Option<String>)>>>>;
type ShowWorkspaceHandle = Rc<RefCell<Option<Box<dyn Fn(usize, WorkspacePreset, PathBuf)>>>>;
type VoidHandle = Rc<RefCell<Option<Box<dyn Fn()>>>>;
type ShortcutControllerHandle = Rc<RefCell<Option<gtk::ShortcutController>>>;

const DEFAULT_WORKSPACE_FULLSCREEN_SHORTCUT: &str = "F11";
const DEFAULT_WORKSPACE_DENSITY_SHORTCUT: &str = "<Ctrl><Shift>D";
const DEFAULT_WORKSPACE_ZOOM_IN_SHORTCUT: &str = "<Ctrl>plus";
const DEFAULT_WORKSPACE_ZOOM_OUT_SHORTCUT: &str = "<Ctrl>minus";

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

#[derive(Clone)]
struct WorkspaceTab {
    id: usize,
    default_title: String,
    custom_title: Option<String>,
    subtitle: String,
    page_name: String,
    content: TabContent,
    workspace_root: Option<PathBuf>,
}

#[derive(Clone)]
enum TabContent {
    LaunchDeck,
    Workspace(Box<WorkspaceState>),
}

#[derive(Clone)]
struct TabLabel {
    id: usize,
    title: String,
    tile_count: Option<usize>,
}

#[derive(Clone)]
struct WorkspaceState {
    preset: WorkspacePreset,
    runtime: workspace_view::WorkspaceRuntime,
    terminal_zoom_steps: i32,
}

#[derive(Clone)]
struct LaunchTabContext {
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    stack: gtk::Stack,
    window: adw::ApplicationWindow,
    preference_store: Rc<PreferenceStore>,
    preset_store: Rc<PresetStore>,
    show_workspace_handle: ShowWorkspaceHandle,
    close_tab_handle: TabActionHandle,
    refresh_launch_tabs: VoidHandle,
}

pub fn present(
    app: &adw::Application,
    preference_store: PreferenceStore,
    preset_store: PresetStore,
    session_store: SessionStore,
    saved_session: Option<SavedSession>,
    startup_warning: Option<String>,
    tray_controller: TrayController,
) {
    let preference_store = Rc::new(preference_store);
    let preset_store = Rc::new(preset_store);
    let session_store = Rc::new(session_store);

    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    header.add_css_class("app-headerbar");

    let title = TitleChrome::new();
    title.root.add_css_class("app-title-handle");
    header.set_title_widget(Some(&title.root));

    let stack = gtk::Stack::builder()
        .hexpand(true)
        .vexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&stack));

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
            .valign(gtk::Align::Start)
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
    settings_button.add_css_class("flat");
    settings_button.add_css_class("titlebar-action-button");
    settings_button.add_css_class("titlebar-icon-button");
    settings_button.set_tooltip_text(Some("Application settings"));
    header.pack_end(&settings_button);

    let tabs = Rc::new(RefCell::new(Vec::<WorkspaceTab>::new()));
    let next_tab_id = Rc::new(Cell::new(1usize));
    let active_tab_id = Rc::new(Cell::new(0usize));
    let select_tab: SelectTabHandle = Rc::new(RefCell::new(None));
    let close_tab: TabActionHandle = Rc::new(RefCell::new(None));
    let request_tab_rename: TabActionHandle = Rc::new(RefCell::new(None));
    let reorder_tabs: ReorderTabHandle = Rc::new(RefCell::new(None));
    let apply_tab_rename: RenameTabHandle = Rc::new(RefCell::new(None));
    let show_workspace_in_tab: ShowWorkspaceHandle = Rc::new(RefCell::new(None));
    let refresh_launch_tabs: VoidHandle = Rc::new(RefCell::new(None));
    let add_workspace_tab: VoidHandle = Rc::new(RefCell::new(None));
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
    let quit_requested = Rc::new(Cell::new(false));
    let fullscreen_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let density_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let zoom_in_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
    let zoom_out_shortcut_controller: ShortcutControllerHandle = Rc::new(RefCell::new(None));
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
        let title_for_select = title.clone();
        let title_root_for_select = title.root.clone();
        let stack_for_select = stack.clone();
        let header_for_select = header.clone();
        let window_for_select = window.clone();
        let back_for_select = back_button.clone();
        let fullscreen_for_select = fullscreen_button.clone();
        let tabs_for_select = tabs.clone();
        let active_for_select = active_tab_id.clone();
        let preference_store_for_select = preference_store.clone();
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let select_handle = select_tab.clone();
        let rename_handle = request_tab_rename.clone();
        let close_handle = close_tab.clone();
        let reorder_handle = reorder_tabs.clone();

        *select_tab.borrow_mut() = Some(Box::new(move |tab_id| {
            let (page_name, is_workspace, preset_for_profile, labels) = {
                let tabs = tabs_for_select.borrow();
                let active = tabs
                    .iter()
                    .find(|tab| tab.id == tab_id)
                    .cloned()
                    .expect("active workspace tab should exist");
                let labels = tabs
                    .iter()
                    .map(|tab| TabLabel {
                        id: tab.id,
                        title: tab_display_title(tab),
                        tile_count: match &tab.content {
                            TabContent::Workspace(workspace) => Some(workspace.preset.tile_count()),
                            TabContent::LaunchDeck => None,
                        },
                    })
                    .collect::<Vec<_>>();

                (
                    active.page_name,
                    matches!(active.content, TabContent::Workspace(_)),
                    match active.content {
                        TabContent::LaunchDeck => None,
                        TabContent::Workspace(workspace) => Some(workspace.preset),
                    },
                    labels,
                )
            };

            active_for_select.set(tab_id);
            stack_for_select.set_visible_child_name(&page_name);

            if let Some(preset) = preset_for_profile.as_ref() {
                apply_shell_profile(&header_for_select, &window_for_select, preset);
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

            let on_select = Rc::new({
                let select_handle = select_handle.clone();
                move |selected_id| {
                    if let Some(select) = select_handle.borrow().as_ref() {
                        select(selected_id);
                    }
                }
            });
            let on_rename = Rc::new({
                let rename_handle = rename_handle.clone();
                move |selected_id| {
                    if let Some(rename) = rename_handle.borrow().as_ref() {
                        rename(selected_id);
                    }
                }
            });
            let on_close = Rc::new({
                let close_handle = close_handle.clone();
                move |selected_id| {
                    if let Some(close) = close_handle.borrow().as_ref() {
                        close(selected_id);
                    }
                }
            });
            let on_reorder = Rc::new({
                let reorder_handle = reorder_handle.clone();
                move |dragged_id, target_id, insert_after| {
                    if let Some(reorder) = reorder_handle.borrow().as_ref() {
                        reorder(dragged_id, target_id, insert_after);
                    }
                }
            });
            title_for_select
                .render_tabs(&labels, tab_id, on_select, on_rename, on_close, on_reorder);
        }));
    }

    {
        let tabs_for_rename = tabs.clone();
        let active_for_rename = active_tab_id.clone();
        let select_for_rename = select_tab.clone();

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
        let active_for_reorder = active_tab_id.clone();
        let select_for_reorder = select_tab.clone();

        *reorder_tabs.borrow_mut() = Some(Box::new(move |dragged_id, target_id, insert_after| {
            let moved = {
                let mut tabs = tabs_for_reorder.borrow_mut();
                reorder_tab_list(&mut tabs, dragged_id, target_id, insert_after)
            };

            if !moved {
                return;
            }

            logging::info(format!(
                "reordered workspace tab {} around tab {} ({})",
                dragged_id,
                target_id,
                if insert_after { "after" } else { "before" }
            ));

            let active_id = active_for_reorder.get();
            if active_id != 0
                && let Some(select) = select_for_reorder.borrow().as_ref()
            {
                select(active_id);
            }
        }));
    }

    {
        let tabs_for_workspace = tabs.clone();
        let stack_for_workspace = stack.clone();
        let select_for_workspace = select_tab.clone();

        *show_workspace_in_tab.borrow_mut() =
            Some(Box::new(move |tab_id, preset, workspace_root| {
                let terminal_zoom_steps = 0;
                let built_workspace = workspace_view::build_with_layout_change_handler(
                    &preset,
                    &workspace_root,
                    terminal_zoom_steps,
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
                let (page_name, previous_runtime) = {
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
                        runtime: built_workspace.runtime.clone(),
                        terminal_zoom_steps,
                    }));
                    tab.workspace_root = Some(workspace_root.clone());
                    (tab.page_name.clone(), previous_runtime)
                };

                if let Some(runtime) = previous_runtime {
                    runtime.terminate_all("replacing workspace view");
                }

                replace_stack_page(&stack_for_workspace, &page_name, &built_workspace.widget);

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
        let stack_for_refresh = stack.clone();
        let window_for_refresh = window.clone();
        let preference_store = preference_store.clone();
        let preset_store = preset_store.clone();
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
                        stack: stack_for_refresh.clone(),
                        window: window_for_refresh.clone(),
                        preference_store: preference_store.clone(),
                        preset_store: preset_store.clone(),
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
        let stack_for_close = stack.clone();
        let active_for_close = active_tab_id.clone();
        let select_for_close = select_tab.clone();
        let add_for_close = add_workspace_tab.clone();
        let window_for_close = window.clone();

        let do_close: Rc<dyn Fn(usize)> = Rc::new({
            let tabs_for_close = tabs_for_close.clone();
            let stack_for_close = stack_for_close.clone();
            let active_for_close = active_for_close.clone();
            let select_for_close = select_for_close.clone();
            let add_for_close = add_for_close.clone();

            move |tab_id| {
                let (page_name, runtime, next_active_id, should_create_replacement) = {
                    let mut tabs = tabs_for_close.borrow_mut();
                    let Some(index) = tabs.iter().position(|tab| tab.id == tab_id) else {
                        return;
                    };

                    let removed = tabs.remove(index);
                    let runtime = match removed.content {
                        TabContent::Workspace(workspace) => Some(workspace.runtime),
                        TabContent::LaunchDeck => None,
                    };
                    let next_active_id = if tabs.is_empty() {
                        None
                    } else if active_for_close.get() == tab_id {
                        tabs.get(index).or_else(|| tabs.last()).map(|tab| tab.id)
                    } else {
                        Some(active_for_close.get())
                    };

                    (removed.page_name, runtime, next_active_id, tabs.is_empty())
                };

                if let Some(runtime) = runtime {
                    runtime.terminate_all("closing workspace tab");
                }
                remove_stack_page(&stack_for_close, &page_name);
                logging::info(format!("closed workspace tab {}", tab_id));

                if should_create_replacement {
                    if let Some(add_tab) = add_for_close.borrow().as_ref() {
                        add_tab();
                    }
                    return;
                }

                if let Some(next_active_id) = next_active_id
                    && let Some(select) = select_for_close.borrow().as_ref()
                {
                    select(next_active_id);
                }
            }
        });

        *close_tab.borrow_mut() = Some(Box::new(move |tab_id| {
            let is_workspace = {
                let tabs = tabs_for_close.borrow();
                tabs.iter()
                    .find(|tab| tab.id == tab_id)
                    .map(|tab| matches!(tab.content, TabContent::Workspace(_)))
                    .unwrap_or(false)
            };

            if is_workspace {
                let do_close = do_close.clone();
                confirm_destructive_action(
                    &window_for_close,
                    "Close Workspace?",
                    "Running terminal sessions in this workspace will be terminated.",
                    "Close",
                    move || do_close(tab_id),
                );
            } else {
                do_close(tab_id);
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
        let stack_for_add = stack.clone();
        let window_for_add = window.clone();
        let preference_store = preference_store.clone();
        let preset_store = preset_store.clone();
        let show_workspace_handle = show_workspace_in_tab.clone();
        let close_tab_for_add = close_tab.clone();
        let refresh_handle = refresh_launch_tabs.clone();
        let select_for_add = select_tab.clone();

        *add_workspace_tab.borrow_mut() = Some(Box::new(move || {
            let tab_id = next_tab_id.get();
            next_tab_id.set(tab_id + 1);

            let launch_title = format!("Workspace {}", tab_id);
            tabs_for_add.borrow_mut().push(WorkspaceTab {
                id: tab_id,
                default_title: launch_title,
                custom_title: None,
                subtitle: "Launch deck".into(),
                page_name: tab_page_name(tab_id),
                content: TabContent::LaunchDeck,
                workspace_root: None,
            });

            rebuild_launch_tab(
                tab_id,
                &LaunchTabContext {
                    tabs: tabs_for_add.clone(),
                    stack: stack_for_add.clone(),
                    window: window_for_add.clone(),
                    preference_store: preference_store.clone(),
                    preset_store: preset_store.clone(),
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
        let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
        let current_density_shortcut = current_density_shortcut.clone();
        let current_close_to_background = current_close_to_background.clone();
        let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
        let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
        let sync_close_to_background_notice = sync_close_to_background_notice.clone();
        let tray_controller = tray_controller.clone();

        Rc::new(move || {
            let preferences = preference_store_for_settings.load();
            settings_dialog::present(
                &window_for_settings,
                preferences.default_theme,
                preferences.default_density,
                preferences.close_to_background,
                preferences.workspace_fullscreen_shortcut,
                preferences.workspace_density_shortcut,
                preferences.workspace_zoom_in_shortcut,
                preferences.workspace_zoom_out_shortcut,
                preferences.settings_dialog_width,
                preferences.settings_dialog_height,
                {
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
                },
                {
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
                },
                {
                    let preference_store = preference_store_for_settings.clone();
                    let toast_overlay = toast_overlay_for_settings.clone();
                    let current_close_to_background = current_close_to_background.clone();
                    let sync_close_to_background_notice = sync_close_to_background_notice.clone();
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
                },
                {
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
                },
                {
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
                },
                {
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
                },
                {
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
                },
                {
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
                    let current_fullscreen_shortcut = current_fullscreen_shortcut.clone();
                    let current_density_shortcut = current_density_shortcut.clone();
                    let current_close_to_background = current_close_to_background.clone();
                    let current_zoom_in_shortcut = current_zoom_in_shortcut.clone();
                    let current_zoom_out_shortcut = current_zoom_out_shortcut.clone();
                    let sync_close_to_background_notice = sync_close_to_background_notice.clone();
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
                },
                {
                    let preference_store = preference_store_for_settings.clone();
                    move |width, height| {
                        preference_store.save_settings_dialog_size(width, height);
                    }
                },
            );
        })
    };

    {
        let open_settings_dialog = open_settings_dialog.clone();
        settings_button.connect_clicked(move |_| open_settings_dialog());
    }

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
        let window_for_quit_action = window.clone();
        let tray_controller = tray_controller.clone();
        let quit_requested = quit_requested.clone();
        let action = gio::SimpleAction::new("quit-app", None);
        action.connect_activate(move |_, _| {
            tray_controller.set_window_hidden(false);
            quit_requested.set(true);
            window_for_quit_action.close();
        });
        window.add_action(&action);
    }

    if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
        add_tab();
    }

    let tabs_for_back = tabs.clone();
    let stack_for_back = stack.clone();
    let window_for_back = window.clone();
    let preference_store_for_back = preference_store.clone();
    let preset_store_for_back = preset_store.clone();
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
            let stack_for_back = stack_for_back.clone();
            let window_for_back = window_for_back.clone();
            let preference_store_for_back = preference_store_for_back.clone();
            let preset_store_for_back = preset_store_for_back.clone();
            let show_workspace_for_back = show_workspace_for_back.clone();
            let close_tab_for_back = close_tab_for_back.clone();
            let refresh_for_back = refresh_for_back.clone();
            let select_for_back = select_for_back.clone();

            move || {
                let (page_name, runtime) = {
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
                    (tab.page_name.clone(), runtime)
                };

                logging::info(format!("returning workspace tab {} to launch deck", tab_id));

                if let Some(runtime) = runtime {
                    runtime.terminate_all("returning workspace tab to templates");
                }
                remove_stack_page(&stack_for_back, &page_name);
                rebuild_launch_tab(
                    tab_id,
                    &LaunchTabContext {
                        tabs: tabs_for_back.clone(),
                        stack: stack_for_back.clone(),
                        window: window_for_back.clone(),
                        preference_store: preference_store_for_back.clone(),
                        preset_store: preset_store_for_back.clone(),
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
        let tray_controller = tray_controller.clone();
        window.connect_close_request(move |window| {
            if !quit_requested.replace(false)
                && current_close_to_background.get()
                && tray_controller.is_available()
            {
                logging::info("hiding application window to background");
                tray_controller.set_window_hidden(true);
                window.set_visible(false);
                return glib::Propagation::Stop;
            }

            tray_controller.set_window_hidden(false);
            let runtimes = workspace_runtimes(&tabs_for_save);
            if let Some(session) = collect_session(&tabs_for_save, active_for_save.get()) {
                logging::info(format!(
                    "saving session with {} workspace tab(s)",
                    session.tabs.len()
                ));
                session_store.save(&session);
            } else {
                logging::info("no workspace tabs to save, clearing session");
                session_store.clear();
            }

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
        let stack_for_restore = stack.clone();
        let select_for_restore = select_tab.clone();
        let active_for_restore = active_tab_id.clone();
        let session_store_for_restore = session_store.clone();
        let window_for_restore = window.clone();
        let warning = startup_warning.clone();

        glib::idle_add_local_once(move || {
            prompt_session_resume(
                &window_for_restore,
                &saved_session,
                warning.as_deref(),
                {
                    let tabs = tabs_for_restore.clone();
                    let next_tab_id = next_tab_id_for_restore.clone();
                    let stack = stack_for_restore.clone();
                    let select_tab = select_for_restore.clone();
                    let active_tab_id = active_for_restore.clone();
                    move || {
                        restore_saved_session(
                            &tabs,
                            &next_tab_id,
                            &stack,
                            &select_tab,
                            &active_tab_id,
                            resume_session.clone(),
                            true,
                        );
                    }
                },
                move || {
                    session_store_for_restore.clear();
                },
            );
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

fn reorder_tab_list(
    tabs: &mut Vec<WorkspaceTab>,
    dragged_id: usize,
    target_id: usize,
    insert_after: bool,
) -> bool {
    if dragged_id == target_id {
        return false;
    }

    let Some(from_index) = tabs.iter().position(|tab| tab.id == dragged_id) else {
        return false;
    };
    let moved_tab = tabs.remove(from_index);
    let Some(target_index) = tabs.iter().position(|tab| tab.id == target_id) else {
        tabs.insert(from_index.min(tabs.len()), moved_tab);
        return false;
    };

    let insert_index = if insert_after {
        (target_index + 1).min(tabs.len())
    } else {
        target_index
    };
    tabs.insert(insert_index, moved_tab);
    true
}

fn attach_tab_drop_target(
    widget: &impl IsA<gtk::Widget>,
    shell: &gtk::Box,
    target_id: usize,
    on_reorder: Rc<dyn Fn(usize, usize, bool)>,
    insert_after_override: Option<bool>,
) {
    let drop_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    {
        let shell = shell.clone();
        drop_target.connect_enter(move |_, _, _| {
            shell.add_css_class("is-drop-target");
            gdk::DragAction::MOVE
        });
    }
    {
        let shell = shell.clone();
        drop_target.connect_leave(move |_| {
            shell.remove_css_class("is-drop-target");
        });
    }
    {
        let shell = shell.clone();
        let drop_surface = widget.as_ref().clone();
        drop_target.connect_drop(move |_, value, x, _| {
            shell.remove_css_class("is-drop-target");

            let Ok(dragged_id) = value.get::<String>() else {
                return false;
            };
            let Ok(dragged_id) = dragged_id.parse::<usize>() else {
                return false;
            };

            let insert_after = insert_after_override.unwrap_or_else(|| {
                x >= (drop_surface.allocated_width() as f64 / 2.0)
            });
            on_reorder(dragged_id, target_id, insert_after);
            true
        });
    }
    widget.as_ref().add_controller(drop_target);
}

fn clear_tab_drag_state(tabs_box: &gtk::Box) {
    let mut child = tabs_box.first_child();
    while let Some(widget) = child {
        widget.remove_css_class("is-dragging");
        widget.remove_css_class("is-drop-target");
        child = widget.next_sibling();
    }
}

fn build_tab_button_content(tab: &TabLabel, is_active: bool) -> gtk::Box {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .valign(gtk::Align::Center)
        .build();

    let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    icon.add_css_class("app-tab-icon");
    content.append(&icon);

    let title = gtk::Label::builder()
        .label(&tab.title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["app-tab-title"])
        .build();
    content.append(&title);

    if !is_active && let Some(count) = tab.tile_count {
        let badge = gtk::Label::builder()
            .label(count.to_string())
            .css_classes(["app-tab-badge"])
            .build();
        content.append(&badge);
    }

    content
}

fn build_tab_drag_preview(tab: &TabLabel, is_active: bool, width: i32) -> gtk::Box {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["app-tab-shell", "app-tab-drag-icon"])
        .build();

    if is_active {
        shell.add_css_class("is-active");
    } else {
        shell.add_css_class("is-inactive");
    }
    shell.set_size_request(width.max(142), -1);

    let button = gtk::Button::builder()
        .focus_on_click(false)
        .sensitive(false)
        .hexpand(true)
        .css_classes(["flat", "app-tab-select"])
        .build();
    button.set_can_target(false);
    button.set_child(Some(&build_tab_button_content(tab, is_active)));
    shell.append(&button);

    if is_active {
        let close_button = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .focus_on_click(false)
            .sensitive(false)
            .css_classes(["flat", "app-tab-close"])
            .build();
        close_button.set_can_target(false);
        shell.append(&close_button);
    }

    shell
}

fn rebuild_launch_tab(tab_id: usize, context: &LaunchTabContext) {
    let page_name = context
        .tabs
        .borrow()
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.page_name.clone())
        .expect("launch tab should exist");

    let load_outcome = context.preset_store.load_presets_with_status();
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
        load_outcome.warning,
        &presets,
        preferences.default_theme,
        preferences.default_density,
        preset_store,
        move |theme| {
            apply_theme_mode(&theme_preview_window, &theme);
        },
        {
            move |density| {
                apply_window_density(&density_preview_window, Some(density));
            }
        },
        move |preset, workspace_root| {
            if let Some(show_workspace) = show_workspace_handle.borrow().as_ref() {
                show_workspace(tab_id, preset, workspace_root);
            }
        },
        {
            let close_tab_handle = close_tab_handle.clone();
            move || {
                if let Some(close) = close_tab_handle.borrow().as_ref() {
                    close(tab_id);
                }
            }
        },
        move || {
            let refresh_for_idle = refresh_handle.clone();
            glib::idle_add_local_once(move || {
                if let Some(refresh) = refresh_for_idle.borrow().as_ref() {
                    refresh();
                }
            });
        },
    );

    replace_stack_page(&context.stack, &page_name, &launch_surface);
}

fn replace_stack_page(stack: &gtk::Stack, page_name: &str, widget: &gtk::Widget) {
    remove_stack_page(stack, page_name);
    stack.add_named(widget, Some(page_name));
}

fn remove_stack_page(stack: &gtk::Stack, page_name: &str) {
    if let Some(existing) = stack.child_by_name(page_name) {
        stack.remove(&existing);
    }
}

fn tab_page_name(tab_id: usize) -> String {
    format!("workspace-tab-{}", tab_id)
}

fn clear_all_tabs(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    stack: &gtk::Stack,
    active_tab_id: &Cell<usize>,
) {
    let (page_names, runtimes) = tabs
        .borrow()
        .iter()
        .map(|tab| {
            (
                tab.page_name.clone(),
                match &tab.content {
                    TabContent::Workspace(workspace) => Some(workspace.runtime.clone()),
                    TabContent::LaunchDeck => None,
                },
            )
        })
        .unzip::<_, _, Vec<_>, Vec<_>>();
    tabs.borrow_mut().clear();
    active_tab_id.set(0);

    for runtime in runtimes.into_iter().flatten() {
        runtime.terminate_all("clearing workspace tabs");
    }

    for page_name in page_names {
        remove_stack_page(stack, &page_name);
    }
}

fn restore_saved_session(
    tabs: &Rc<RefCell<Vec<WorkspaceTab>>>,
    next_tab_id: &Rc<Cell<usize>>,
    stack: &gtk::Stack,
    select_tab: &SelectTabHandle,
    active_tab_id: &Cell<usize>,
    saved_session: SavedSession,
    replace_existing: bool,
) {
    if replace_existing {
        clear_all_tabs(tabs, stack, active_tab_id);
    }

    let mut restored_ids = Vec::with_capacity(saved_session.tabs.len());
    for saved_tab in saved_session.tabs {
        let tab_id = next_tab_id.get();
        next_tab_id.set(tab_id + 1);

        let page_name = tab_page_name(tab_id);
        let workspace_root = saved_tab.workspace_root;
        let preset = saved_tab.preset;
        let terminal_zoom_steps =
            clamp_terminal_zoom_steps(preset.density, saved_tab.terminal_zoom_steps);

        let built_workspace = workspace_view::build_with_layout_change_handler(
            &preset,
            &workspace_root,
            terminal_zoom_steps,
            {
                let tabs = tabs.clone();
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
        tabs.borrow_mut().push(WorkspaceTab {
            id: tab_id,
            default_title: format!("Workspace {}", tab_id),
            custom_title: saved_tab.custom_title,
            subtitle: workspace_root.display().to_string(),
            page_name: page_name.clone(),
            content: TabContent::Workspace(Box::new(WorkspaceState {
                preset: preset.clone(),
                runtime: built_workspace.runtime.clone(),
                terminal_zoom_steps,
            })),
            workspace_root: Some(workspace_root.clone()),
        });

        stack.add_named(&built_workspace.widget, Some(&page_name));
        restored_ids.push(tab_id);
    }

    let restored_active_id = restored_ids
        .get(saved_session.active_tab_index)
        .copied()
        .or_else(|| restored_ids.first().copied());

    if let Some(active_id) = restored_active_id
        && let Some(select) = select_tab.borrow().as_ref()
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
    fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .hexpand(true)
            .halign(gtk::Align::Center)
            .css_classes(["app-tab-strip"])
            .build();

        let tabs_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();

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

    fn render_tabs(
        &self,
        tabs: &[TabLabel],
        active_id: usize,
        on_select: Rc<dyn Fn(usize)>,
        on_rename: Rc<dyn Fn(usize)>,
        on_close: Rc<dyn Fn(usize)>,
        on_reorder: Rc<dyn Fn(usize, usize, bool)>,
    ) {
        while let Some(child) = self.tabs_box.first_child() {
            self.tabs_box.remove(&child);
        }

        for tab in tabs {
            let is_active = tab.id == active_id;

            let shell = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(0)
                .css_classes(["app-tab-shell"])
                .build();

            let button = gtk::Button::builder()
                .focus_on_click(false)
                .hexpand(true)
                .css_classes(["flat", "app-tab-select"])
                .build();
            if is_active {
                shell.add_css_class("is-active");
            } else {
                shell.add_css_class("is-inactive");
            }

            button.set_child(Some(&build_tab_button_content(tab, is_active)));

            let tab_id = tab.id;
            let on_select = on_select.clone();
            button.connect_clicked(move |_| {
                if !is_active {
                    on_select(tab_id);
                }
            });

            let middle_click = gtk::GestureClick::builder().button(2).build();
            let tab_id = tab.id;
            let on_close_gesture = on_close.clone();
            middle_click.connect_pressed(move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                on_close_gesture(tab_id);
            });
            button.add_controller(middle_click);

            let rename_click = gtk::GestureClick::builder()
                .button(1)
                .propagation_phase(gtk::PropagationPhase::Capture)
                .build();
            let tab_id = tab.id;
            let on_rename_gesture = on_rename.clone();
            rename_click.connect_pressed(move |gesture, n_press, _, _| {
                if n_press == 2 {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    on_rename_gesture(tab_id);
                }
            });
            shell.add_controller(rename_click);

            let drag_source = gtk::DragSource::builder()
                .actions(gdk::DragAction::MOVE)
                .build();
            let drag_shell = shell.clone();
            let drag_hotspot = Rc::new(Cell::new((0, 0)));
            let drag_tab = tab.clone();
            let tabs_box = self.tabs_box.clone();
            let tab_id = tab.id;
            {
                let drag_shell = drag_shell.clone();
                let drag_hotspot = drag_hotspot.clone();
                drag_source.connect_prepare(move |_, x, y| {
                    let hot_x = x.round().clamp(0.0, drag_shell.allocated_width() as f64) as i32;
                    let hot_y = y.round().clamp(0.0, drag_shell.allocated_height() as f64) as i32;
                    drag_hotspot.set((hot_x, hot_y));
                    Some(gdk::ContentProvider::for_value(
                        &tab_id.to_string().to_value(),
                    ))
                });
            }
            {
                let drag_shell = drag_shell.clone();
                let drag_tab = drag_tab.clone();
                let tabs_box = tabs_box.clone();
                drag_source.connect_drag_begin(move |_, drag| {
                    clear_tab_drag_state(&tabs_box);
                    let preview =
                        build_tab_drag_preview(&drag_tab, is_active, drag_shell.allocated_width());
                    gtk::DragIcon::for_drag(drag).set_child(Some(&preview));
                    drag_shell.add_css_class("is-dragging");
                });
            }
            {
                let tabs_box = tabs_box.clone();
                drag_source.connect_drag_cancel(move |_, _, _| {
                    clear_tab_drag_state(&tabs_box);
                    false
                });
            }
            let tabs_box = self.tabs_box.clone();
            drag_source.connect_drag_end(move |_, _, _| {
                clear_tab_drag_state(&tabs_box);
            });
            button.add_controller(drag_source);

            shell.append(&button);

            attach_tab_drop_target(&button, &shell, tab.id, on_reorder.clone(), None);

            if is_active {
                let close_button = gtk::Button::builder()
                    .icon_name("window-close-symbolic")
                    .focus_on_click(false)
                    .css_classes(["flat", "app-tab-close"])
                    .build();
                close_button.set_tooltip_text(Some("Close workspace tab"));
                let tab_id = tab.id;
                let on_close = on_close.clone();
                close_button.connect_clicked(move |_| {
                    on_close(tab_id);
                });
                attach_tab_drop_target(
                    &close_button,
                    &shell,
                    tab.id,
                    on_reorder.clone(),
                    Some(true),
                );
                shell.append(&close_button);
            }

            self.tabs_box.append(&shell);
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

    runtime.apply_density(next_density, terminal_zoom_steps);
    apply_window_density(window, Some(next_density));
    logging::info(format!(
        "cycled workspace density preset='{}' density={}",
        workspace_name,
        next_density.label()
    ));
    Some(next_density)
}

fn adjust_active_workspace_zoom(
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

    runtime.apply_density(density, terminal_zoom_steps);
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
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_zoom_in",
        &zoom_in_shortcut_accelerators(shortcut),
        move || {
            if adjust_active_workspace_zoom(&tabs_for_shortcut, active_for_shortcut.get(), 1)
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
    install_shortcut_controller(
        window,
        controller_handle,
        "workspace_zoom_out",
        &zoom_out_shortcut_accelerators(shortcut),
        move || {
            if adjust_active_workspace_zoom(&tabs_for_shortcut, active_for_shortcut.get(), -1)
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

fn prompt_session_resume<F, G>(
    window: &adw::ApplicationWindow,
    saved_session: &SavedSession,
    warning: Option<&str>,
    on_resume: F,
    on_start_fresh: G,
) where
    F: Fn() + 'static,
    G: Fn() + 'static,
{
    let body = if let Some(warning) = warning {
        format!(
            "TerminalTiler found {} saved workspace(s). Resuming will re-run their startup commands.\n\n{}",
            saved_session.tabs.len(),
            warning
        )
    } else {
        format!(
            "TerminalTiler found {} saved workspace(s). Resuming will re-run their startup commands.",
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
    dialog.add_response("resume", "Resume");
    dialog.set_response_appearance("resume", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("fresh"));
    dialog.set_close_response("fresh");

    dialog.connect_response(None, move |dialog, response| {
        match response {
            "resume" => on_resume(),
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
    use super::{TabContent, WorkspaceTab, reorder_tab_list};

    fn launch_tab(id: usize) -> WorkspaceTab {
        WorkspaceTab {
            id,
            default_title: format!("Workspace {id}"),
            custom_title: None,
            subtitle: String::new(),
            page_name: format!("workspace-tab-{id}"),
            content: TabContent::LaunchDeck,
            workspace_root: None,
        }
    }

    fn tab_ids(tabs: &[WorkspaceTab]) -> Vec<usize> {
        tabs.iter().map(|tab| tab.id).collect()
    }

    #[test]
    fn reorders_tab_before_target() {
        let mut tabs = vec![launch_tab(1), launch_tab(2), launch_tab(3)];

        let moved = reorder_tab_list(&mut tabs, 3, 1, false);

        assert!(moved);
        assert_eq!(tab_ids(&tabs), vec![3, 1, 2]);
    }

    #[test]
    fn reorders_tab_after_target() {
        let mut tabs = vec![launch_tab(1), launch_tab(2), launch_tab(3)];

        let moved = reorder_tab_list(&mut tabs, 1, 3, true);

        assert!(moved);
        assert_eq!(tab_ids(&tabs), vec![2, 3, 1]);
    }

    #[test]
    fn ignores_reorder_when_dragging_onto_same_tab() {
        let mut tabs = vec![launch_tab(1), launch_tab(2), launch_tab(3)];

        let moved = reorder_tab_list(&mut tabs, 2, 2, true);

        assert!(!moved);
        assert_eq!(tab_ids(&tabs), vec![1, 2, 3]);
    }

    #[test]
    fn ignores_reorder_for_unknown_target() {
        let mut tabs = vec![launch_tab(1), launch_tab(2), launch_tab(3)];

        let moved = reorder_tab_list(&mut tabs, 2, 99, false);

        assert!(!moved);
        assert_eq!(tab_ids(&tabs), vec![1, 2, 3]);
    }
}
