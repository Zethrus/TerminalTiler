use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::app::logging;
use crate::model::preset::{ThemeMode, WindowChrome, WorkspacePreset};
use crate::storage::preset_store::PresetStore;
use crate::storage::session_store::{SavedSession, SavedTab, SessionStore};
use crate::ui::{launch_screen, workspace_view};

type SelectTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type TabActionHandle = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;
type ReorderTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, usize, bool)>>>>;
type RenameTabHandle = Rc<RefCell<Option<Box<dyn Fn(usize, Option<String>)>>>>;
type ShowWorkspaceHandle = Rc<RefCell<Option<Box<dyn Fn(usize, WorkspacePreset, PathBuf)>>>>;
type VoidHandle = Rc<RefCell<Option<Box<dyn Fn()>>>>;

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
}

#[derive(Clone)]
struct LaunchTabContext {
    tabs: Rc<RefCell<Vec<WorkspaceTab>>>,
    stack: gtk::Stack,
    window: adw::ApplicationWindow,
    preset_store: Rc<PresetStore>,
    show_workspace_handle: ShowWorkspaceHandle,
    close_tab_handle: TabActionHandle,
    refresh_launch_tabs: VoidHandle,
}

pub fn present(
    app: &adw::Application,
    preset_store: PresetStore,
    session_store: SessionStore,
    saved_session: Option<SavedSession>,
    startup_warning: Option<String>,
) {
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

    let window_shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    window_shell.append(&header);
    window_shell.append(&stack);

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
                reset_shell_profile(&header_for_select, &window_for_select);
            }
            back_for_select.set_visible(is_workspace);
            sync_fullscreen_chrome(
                &window_for_select,
                title_root_for_select.upcast_ref(),
                &fullscreen_for_select,
                is_workspace,
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
                let built_workspace = workspace_view::build(&preset, &workspace_root);
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

    {
        let tabs_for_shortcut = tabs.clone();
        let active_for_shortcut = active_tab_id.clone();
        let window_for_shortcut = window.clone();

        let shortcut_controller = gtk::ShortcutController::new();
        shortcut_controller.set_scope(gtk::ShortcutScope::Global);
        let trigger =
            gtk::ShortcutTrigger::parse_string("F11").expect("F11 shortcut trigger should parse");
        let action = gtk::CallbackAction::new(move |_, _| {
            toggle_workspace_fullscreen(
                &window_for_shortcut,
                &tabs_for_shortcut,
                active_for_shortcut.get(),
            );
            glib::Propagation::Stop
        });
        shortcut_controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
        window.add_controller(shortcut_controller);
    }

    {
        let window_for_notify = window.clone();
        let title_root_for_notify = title.root.clone();
        let fullscreen_for_notify = fullscreen_button.clone();
        let tabs_for_notify = tabs.clone();
        let active_for_notify = active_tab_id.clone();
        window.connect_fullscreened_notify(move |window| {
            let is_workspace = active_tab_is_workspace(&tabs_for_notify, active_for_notify.get());
            sync_fullscreen_chrome(
                &window_for_notify,
                title_root_for_notify.upcast_ref(),
                &fullscreen_for_notify,
                is_workspace,
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

    if let Some(add_tab) = add_workspace_tab.borrow().as_ref() {
        add_tab();
    }

    let tabs_for_back = tabs.clone();
    let stack_for_back = stack.clone();
    let window_for_back = window.clone();
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
        window.connect_close_request(move |_| {
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
    let preset_store = context.preset_store.as_ref().clone();
    let window = context.window.clone();
    let show_workspace_handle = context.show_workspace_handle.clone();
    let close_tab_handle = context.close_tab_handle.clone();
    let refresh_handle = context.refresh_launch_tabs.clone();

    let launch_surface = launch_screen::build(
        load_outcome.warning,
        &presets,
        preset_store,
        move |theme| {
            apply_theme_mode(&window, &theme);
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

        let built_workspace = workspace_view::build(&preset, &workspace_root);
        tabs.borrow_mut().push(WorkspaceTab {
            id: tab_id,
            default_title: format!("Workspace {}", tab_id),
            custom_title: saved_tab.custom_title,
            subtitle: workspace_root.display().to_string(),
            page_name: page_name.clone(),
            content: TabContent::Workspace(Box::new(WorkspaceState {
                preset: preset.clone(),
                runtime: built_workspace.runtime.clone(),
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

            button.set_child(Some(&content));

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
            let tab_id = tab.id;
            drag_source.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(
                    &tab_id.to_string().to_value(),
                ))
            });
            drag_source.connect_drag_begin(move |_, _| {
                drag_shell.add_css_class("is-dragging");
            });
            let drag_shell = shell.clone();
            drag_source.connect_drag_end(move |_, _, _| {
                drag_shell.remove_css_class("is-dragging");
            });
            button.add_controller(drag_source);

            shell.append(&button);

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
                shell.append(&close_button);
            }

            let drop_target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            let drop_shell = shell.clone();
            drop_target.connect_enter(move |_, _, _| {
                drop_shell.add_css_class("is-drop-target");
                gdk::DragAction::MOVE
            });
            let drop_shell = shell.clone();
            drop_target.connect_leave(move |_| {
                drop_shell.remove_css_class("is-drop-target");
            });
            let drop_shell = shell.clone();
            let on_reorder = on_reorder.clone();
            let target_id = tab.id;
            drop_target.connect_drop(move |_, value, x, _| {
                drop_shell.remove_css_class("is-drop-target");

                let Ok(dragged_id) = value.get::<String>() else {
                    return false;
                };
                let Ok(dragged_id) = dragged_id.parse::<usize>() else {
                    return false;
                };
                let insert_after = x >= (drop_shell.allocated_width() as f64 / 2.0);
                on_reorder(dragged_id, target_id, insert_after);
                true
            });
            shell.add_controller(drop_target);

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
        "applying shell profile preset='{}' theme={} chrome={}",
        preset.name,
        preset.theme.label(),
        preset.chrome.label()
    ));

    apply_theme_mode(window, &preset.theme);

    window.remove_css_class("profile-standard");
    window.remove_css_class("profile-compact");
    window.add_css_class(match preset.chrome {
        WindowChrome::Standard => "profile-standard",
        WindowChrome::Compact => "profile-compact",
    });
}

fn reset_shell_profile(header: &adw::HeaderBar, window: &adw::ApplicationWindow) {
    configure_window_controls(header);
    logging::info("resetting shell chrome for launch deck");
    apply_theme_mode(window, &ThemeMode::System);
    window.remove_css_class("profile-standard");
    window.remove_css_class("profile-compact");
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

fn sync_fullscreen_chrome(
    window: &adw::ApplicationWindow,
    title_widget: &gtk::Widget,
    fullscreen_button: &gtk::Button,
    is_workspace: bool,
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
        fullscreen_button.set_tooltip_text(Some("Exit fullscreen (F11)"));
    } else {
        fullscreen_button.set_label("Fullscreen");
        fullscreen_button.set_tooltip_text(Some("Enter fullscreen"));
    }
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
