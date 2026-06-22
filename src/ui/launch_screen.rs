use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use uuid::Uuid;

use crate::logging;
use crate::model::assets::{RestoreLaunchMode, WorkspaceAssets};
use crate::model::board_workspace::{BoardLaunchRequest, BoardWorkspace};
use crate::model::layout::{
    DEFAULT_WEB_URL, LayoutNode, LayoutTemplate, SplitAxis, TileKind, TileSpec, builtin_templates,
    generate_layout, normalize_web_url,
};
use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset, is_builtin_preset_id};
use crate::platform::{home_dir, resolve_workspace_root};
use crate::services::agent_config;
use crate::services::layout_editor::{close_tile, split_tile};
use crate::services::project_suggestions::detect_project_suggestions;
use crate::services::tile_draft::{
    apply_project_suggestion as apply_suggestion_to_layout, apply_role_to_tile, resize_layout,
    resolve_role,
};
use crate::storage::board_workspace_store::BoardWorkspaceStore;
use crate::storage::preset_store::PresetStore;
use crate::ui::dialog_chrome;
use crate::ui::icons::{self, name as icon_name};

#[derive(Clone, Copy, Debug)]
enum Selection {
    Template(usize),
    Preset(usize),
}

#[derive(Clone)]
struct TileEditorPanel {
    root: gtk::Box,
    tile_count: gtk::SpinButton,
    status_label: gtk::Label,
    rows: gtk::Box,
    scroller: gtk::ScrolledWindow,
}

#[derive(Clone)]
struct WizardStepper {
    root: gtk::Box,
    steps: Vec<gtk::Button>,
}

pub struct LaunchScreenInput {
    pub load_warning: Option<String>,
    pub presets: Vec<WorkspacePreset>,
    pub board_workspaces: Option<Vec<BoardWorkspace>>,
    pub assets: WorkspaceAssets,
    pub default_theme: ThemeMode,
    pub default_density: ApplicationDensity,
    pub default_restore_mode: RestoreLaunchMode,
    pub preset_store: PresetStore,
    pub board_workspace_store: Option<BoardWorkspaceStore>,
}

#[derive(Clone)]
pub struct LaunchScreenActions {
    pub on_theme_preview: Rc<dyn Fn(ThemeMode)>,
    pub on_density_preview: Rc<dyn Fn(ApplicationDensity)>,
    pub on_launch: Rc<dyn Fn(WorkspacePreset, PathBuf)>,
    pub on_launch_board: Option<Rc<dyn Fn(BoardLaunchRequest)>>,
    pub on_cancel: Rc<dyn Fn()>,
    pub on_presets_changed: Rc<dyn Fn()>,
}

pub fn build(input: LaunchScreenInput, actions: LaunchScreenActions) -> gtk::Widget {
    let LaunchScreenInput {
        load_warning,
        presets,
        board_workspaces,
        assets,
        default_theme,
        default_density,
        default_restore_mode,
        preset_store,
        board_workspace_store,
    } = input;
    let LaunchScreenActions {
        on_theme_preview,
        on_density_preview,
        on_launch,
        on_launch_board,
        on_cancel,
        on_presets_changed,
    } = actions;
    let current_dir = std::env::current_dir()
        .ok()
        .or_else(home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    logging::info(format!(
        "GTK launch deck default workspace root resolved to {}",
        current_dir.display()
    ));
    let templates = builtin_templates();
    let presets = Rc::new(presets);
    let board_workspaces = board_workspaces.map(Rc::new);
    let assets = Rc::new(assets);
    let launch_callback = on_launch;
    let theme_preview_callback = on_theme_preview;
    let density_preview_callback = on_density_preview;
    let preset_store = Rc::new(preset_store);
    let board_workspace_store = board_workspace_store.map(Rc::new);
    let board_launch_callback = on_launch_board;
    let board_launch_supported = board_workspaces.is_some()
        && board_workspace_store.is_some()
        && board_launch_callback.is_some();
    let selected: Rc<Cell<Selection>> = Rc::new(Cell::new(Selection::Template(0)));
    let chosen_theme: Rc<Cell<ThemeMode>> = Rc::new(Cell::new(default_theme));
    let chosen_density: Rc<Cell<ApplicationDensity>> = Rc::new(Cell::new(default_density));
    let active_layout = Rc::new(RefCell::new(generate_layout(
        templates
            .first()
            .map(|template| template.tile_count)
            .unwrap_or(1),
    )));
    let suggestion_cards: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["launch-shell"])
        .build();

    let stage = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(14)
        .margin_bottom(14)
        .margin_start(16)
        .margin_end(16)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Start)
        .css_classes(["launch-stage", "launch-config-stage"])
        .build();
    let stage_clamp = adw::Clamp::builder()
        .maximum_size(1600)
        .tightening_threshold(1440)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .child(&stage)
        .css_classes(["launch-stage-clamp"])
        .build();
    root.append(&stage_clamp);

    let header = build_header(default_restore_mode);
    stage.append(&header);

    if let Some(load_warning) = load_warning {
        let warning = gtk::Label::builder()
            .label(load_warning)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["config-panel", "field-hint", "path-invalid"])
            .build();
        stage.append(&warning);
    }

    let mode_stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .hexpand(true)
        .vexpand(false)
        .css_classes(["launch-mode-stack"])
        .build();
    stage.append(&mode_stack);

    let dashboard = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["launch-dashboard"])
        .build();
    mode_stack.add_named(&dashboard, Some("dashboard"));

    let wizard = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["launch-wizard-shell"])
        .build();
    mode_stack.add_named(&wizard, Some("wizard"));

    let wizard_stepper = build_wizard_stepper();
    wizard.append(&wizard_stepper.root);

    let wizard_steps = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .transition_duration(180)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .hexpand(true)
        .vexpand(false)
        .css_classes(["launch-wizard-steps"])
        .build();
    wizard.append(&wizard_steps);

    let wizard_step_index = Rc::new(Cell::new(0usize));
    let wizard_step_names = Rc::new(vec!["setup", "appearance", "layout", "tiles"]);

    let board_wizard = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["launch-wizard-shell", "board-wizard-shell"])
        .build();
    mode_stack.add_named(&board_wizard, Some("board-wizard"));

    let board_stepper = build_board_wizard_stepper();
    board_wizard.append(&board_stepper.root);

    let board_steps = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .transition_duration(180)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .hexpand(true)
        .vexpand(false)
        .css_classes(["launch-wizard-steps", "board-wizard-steps"])
        .build();
    board_wizard.append(&board_steps);

    let board_step_index = Rc::new(Cell::new(0usize));
    let board_step_names = Rc::new(vec!["board-setup", "board-agent", "board-review"]);
    let editing_board_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let board_setup_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "directory-panel", "board-setup-panel"])
        .build();
    board_steps.add_named(&board_setup_panel, Some("board-setup"));
    board_setup_panel.append(&build_section_header(
        "Step 1",
        "Kanban project",
        "Choose the project directory that owns .terminaltiler/board.json and name the board shortcut.",
    ));

    board_setup_panel.append(
        &gtk::Label::builder()
            .label("Project directory")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow"])
            .build(),
    );
    let board_path_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(current_dir.display().to_string())
        .placeholder_text("/path/to/project")
        .css_classes(["workspace-path", "board-project-path"])
        .primary_icon_name("folder-symbolic")
        .build();
    let board_path_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["workspace-path-row", "launch-field-row"])
        .build();
    board_path_row.append(&board_path_entry);
    let board_browse_button = icons::labeled_button(
        "Browse",
        icon_name::FOLDER,
        &[
            "pill-button",
            "secondary-button",
            "workspace-browse-button",
            "launch-browse-button",
        ],
    );
    board_path_row.append(&board_browse_button);
    board_setup_panel.append(&board_path_row);
    {
        let board_path_entry = board_path_entry.clone();
        board_path_entry.connect_icon_press(move |entry, position| {
            if position == gtk::EntryIconPosition::Primary {
                prompt_for_workspace_directory(entry);
            }
        });
    }
    {
        let board_path_entry = board_path_entry.clone();
        board_browse_button.connect_clicked(move |_| {
            prompt_for_workspace_directory(&board_path_entry);
        });
    }
    board_path_entry.connect_changed(move |entry| match validate_workspace_path(entry) {
        Ok(_) => {
            entry.remove_css_class("path-invalid");
            entry.add_css_class("path-valid");
        }
        Err(_) => {
            entry.remove_css_class("path-valid");
            entry.add_css_class("path-invalid");
        }
    });

    board_setup_panel.append(
        &gtk::Label::builder()
            .label("Board name")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow"])
            .build(),
    );
    let board_name_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Kanban board name, for example: Project Delivery")
        .text("Project Kanban")
        .css_classes(["workspace-path", "board-name-entry"])
        .build();
    board_setup_panel.append(&board_name_entry);

    let board_agent_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "board-agent-panel"])
        .build();
    board_steps.add_named(&board_agent_panel, Some("board-agent"));
    board_agent_panel.append(&build_section_header(
        "Step 2",
        "MCP / agent setup",
        "Connect Claude Code or Codex so agents can update this project's board through the bundled MCP server.",
    ));
    board_agent_panel.append(
        &gtk::Label::builder()
            .label(format!(
                "Server: {}",
                agent_config::mcp_binary_path().display()
            ))
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["status-chip", "settings-meta-chip", "board-mcp-server-chip"])
            .build(),
    );
    let board_agent_status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["field-hint", "board-agent-status"])
        .build();
    let board_agent_actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["board-agent-actions"])
        .build();
    let board_claude_button = icons::labeled_button(
        "Connect Claude",
        icon_name::TERMINAL,
        &[
            "pill-button",
            "suggested-action",
            "board-connect-claude-button",
        ],
    );
    let board_codex_button = icons::labeled_button(
        "Connect Codex",
        icon_name::TERMINAL,
        &[
            "pill-button",
            "surface-button",
            "board-connect-codex-button",
        ],
    );
    board_agent_actions.append(&board_claude_button);
    board_agent_actions.append(&board_codex_button);
    board_agent_panel.append(&board_agent_actions);
    board_agent_panel.append(&board_agent_status);
    {
        let board_path_entry = board_path_entry.clone();
        let board_agent_status = board_agent_status.clone();
        board_claude_button.connect_clicked(move |_| {
            board_agent_status.set_visible(true);
            match validate_workspace_path(&board_path_entry)
                .map_err(|message| message.to_string())
                .and_then(|project_root| agent_config::connect_claude(&project_root))
            {
                Ok(path) => {
                    board_agent_status.remove_css_class("error-text");
                    board_agent_status
                        .set_text(&format!("Connected Claude. Wrote {}", path.display()));
                }
                Err(message) => {
                    board_agent_status.add_css_class("error-text");
                    board_agent_status.set_text(&format!("Could not connect Claude: {message}"));
                }
            }
        });
    }
    {
        let board_path_entry = board_path_entry.clone();
        let board_agent_status = board_agent_status.clone();
        board_codex_button.connect_clicked(move |_| {
            board_agent_status.set_visible(true);
            match validate_workspace_path(&board_path_entry)
                .map_err(|message| message.to_string())
                .and_then(|project_root| agent_config::connect_codex(&project_root))
            {
                Ok(path) => {
                    board_agent_status.remove_css_class("error-text");
                    board_agent_status
                        .set_text(&format!("Connected Codex. Wrote {}", path.display()));
                }
                Err(message) => {
                    board_agent_status.add_css_class("error-text");
                    board_agent_status.set_text(&format!("Could not connect Codex: {message}"));
                }
            }
        });
    }

    let board_review_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "board-review-panel"])
        .build();
    board_steps.add_named(&board_review_panel, Some("board-review"));
    board_review_panel.append(&build_section_header(
        "Step 3",
        "Review & open",
        "TerminalTiler will save a launch-deck shortcut and create an empty board file if this project does not have one yet.",
    ));
    let board_review_name = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes(["card-title", "board-review-name"])
        .build();
    let board_review_path = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["field-hint", "board-review-path"])
        .build();
    board_review_panel.append(&board_review_name);
    board_review_panel.append(&board_review_path);

    let board_action_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .css_classes(["action-bar-bottom", "launch-action-bar", "board-action-bar"])
        .build();
    board_wizard.append(&board_action_bar);
    let board_dashboard_button = icons::labeled_button(
        "Workspaces",
        icon_name::WORKSPACES,
        &["pill-button", "secondary-button"],
    );
    {
        let mode_stack = mode_stack.clone();
        board_dashboard_button.connect_clicked(move |_| {
            mode_stack.set_visible_child_name("dashboard");
        });
    }
    board_action_bar.append(&board_dashboard_button);
    let board_back_button = icons::labeled_button(
        "Back",
        icon_name::BACK,
        &["pill-button", "secondary-button"],
    );
    board_action_bar.append(&board_back_button);
    let board_spacer = gtk::Box::builder().hexpand(true).build();
    board_action_bar.append(&board_spacer);
    let board_next_button = icons::labeled_button(
        "Next",
        icon_name::NEXT,
        &[
            "pill-button",
            "primary-cta-button",
            "open-kanban-board-button",
        ],
    );
    board_action_bar.append(&board_next_button);

    let directory_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "directory-panel", "setup-panel"])
        .build();
    wizard_steps.add_named(&directory_panel, Some("setup"));
    directory_panel.append(&build_section_header(
        "Step 1",
        "Workspace setup",
        "Choose the workspace folder and give this launch a clear name.",
    ));

    let path_label = gtk::Label::builder()
        .label("Workspace root")
        .halign(gtk::Align::Start)
        .css_classes(["eyebrow"])
        .build();
    directory_panel.append(&path_label);

    let path_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(current_dir.display().to_string())
        .placeholder_text("/path/to/workspace")
        .css_classes(["workspace-path"])
        .primary_icon_name("folder-symbolic")
        .build();

    let path_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["workspace-path-row", "launch-field-row"])
        .build();
    path_row.append(&path_entry);

    let browse_button = icons::labeled_button(
        "Browse",
        icon_name::FOLDER,
        &[
            "pill-button",
            "secondary-button",
            "workspace-browse-button",
            "launch-browse-button",
        ],
    );
    browse_button.set_valign(gtk::Align::Center);
    path_row.append(&browse_button);
    directory_panel.append(&path_row);

    {
        let path_entry = path_entry.clone();
        path_entry.connect_icon_press(move |entry, position| {
            if position != gtk::EntryIconPosition::Primary {
                return;
            }

            prompt_for_workspace_directory(entry);
        });
    }

    {
        let path_entry = path_entry.clone();
        browse_button.connect_clicked(move |_| {
            prompt_for_workspace_directory(&path_entry);
        });
    }

    path_entry.connect_changed(move |entry| match validate_workspace_path(entry) {
        Ok(_) => {
            entry.remove_css_class("path-invalid");
            entry.add_css_class("path-valid");
        }
        Err(_) => {
            entry.remove_css_class("path-valid");
            entry.add_css_class("path-invalid");
        }
    });

    let breadcrumb_target: Rc<std::cell::RefCell<String>> =
        Rc::new(std::cell::RefCell::new(String::new()));

    let breadcrumb = gtk::Button::builder()
        .halign(gtk::Align::Start)
        .css_classes(["breadcrumb-hint"])
        .build();

    {
        let path = PathBuf::from(current_dir.display().to_string());
        if let Some(parent_path) = path.parent() {
            let parent_name = parent_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| parent_path.display().to_string());
            breadcrumb.set_label(&format!("> cd ../{}", parent_name));
            *breadcrumb_target.borrow_mut() = parent_path.display().to_string();
        } else {
            breadcrumb.set_visible(false);
        }
    }

    {
        let path_entry_for_bc = path_entry.clone();
        let target = breadcrumb_target.clone();
        breadcrumb.connect_clicked(move |_| {
            let parent_str = target.borrow().clone();
            if !parent_str.is_empty() {
                path_entry_for_bc.set_text(&parent_str);
            }
        });
    }
    directory_panel.append(&breadcrumb);

    {
        let breadcrumb = breadcrumb.clone();
        let target = breadcrumb_target.clone();
        path_entry.connect_changed(move |entry| {
            let text = entry.text().to_string();
            let path = PathBuf::from(&text);
            if let Some(parent_path) = path.parent() {
                let parent_name = parent_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| parent_path.display().to_string());
                breadcrumb.set_label(&format!("> cd ../{}", parent_name));
                breadcrumb.set_visible(true);
                *target.borrow_mut() = parent_path.display().to_string();
            } else {
                breadcrumb.set_visible(false);
            }
        });
    }

    let session_name_label = gtk::Label::builder()
        .label("Launch name")
        .halign(gtk::Align::Start)
        .css_classes(["eyebrow"])
        .build();
    directory_panel.append(&session_name_label);

    let session_name_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("Session name, for example: Review Pair")
        .css_classes(["workspace-path"])
        .build();
    if let Some(first) = templates.first() {
        session_name_entry.set_text(first.label);
    }
    directory_panel.append(&session_name_entry);

    let options_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes([
            "config-panel",
            "appearance-panel",
            "launch-appearance-panel",
        ])
        .build();
    wizard_steps.add_named(&options_panel, Some("appearance"));
    options_panel.append(&build_section_header(
        "Step 2",
        "Appearance",
        "Preview the theme and density before you open the workspace.",
    ));

    let theme_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip"])
        .build();

    {
        for (mode, label) in [
            (ThemeMode::System, "System"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::Dark, "Dark"),
        ] {
            let btn = gtk::Button::with_label(label);
            btn.add_css_class("flat");
            if mode == default_theme {
                btn.add_css_class("is-active");
            }

            let chosen_theme = chosen_theme.clone();
            let theme_preview_callback = theme_preview_callback.clone();
            let theme_strip_ref = theme_strip.clone();
            btn.connect_clicked(move |clicked| {
                chosen_theme.set(mode);
                theme_preview_callback(mode);
                sync_theme_strip_active(&theme_strip_ref, mode);
                clicked.add_css_class("is-active");
            });
            theme_strip.append(&btn);
        }
        options_panel.append(&build_launch_control_row(
            "Theme",
            "Preview the overall shell before you launch the workspace.",
            &theme_strip,
        ));
    }

    let density_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip"])
        .build();

    {
        for (density, label) in [
            (ApplicationDensity::Comfortable, "Comfortable"),
            (ApplicationDensity::Standard, "Standard"),
            (ApplicationDensity::Compact, "Compact"),
        ] {
            let btn = gtk::Button::with_label(label);
            btn.add_css_class("flat");
            if density == default_density {
                btn.add_css_class("is-active");
            }

            let chosen_density = chosen_density.clone();
            let density_strip_ref = density_strip.clone();
            let density_preview_callback = density_preview_callback.clone();
            btn.connect_clicked(move |_| {
                chosen_density.set(density);
                density_preview_callback(density);
                sync_density_strip_active(&density_strip_ref, density);
            });
            density_strip.append(&btn);
        }
        options_panel.append(&build_launch_control_row(
            "Density",
            "Density changes panel spacing, titlebars, and terminal shell size.",
            &density_strip,
        ));
    }

    let summary = build_selection_summary();
    if let Some(first) = templates.first() {
        summary.name_label.set_text(first.label);
        summary.subtitle_label.set_text(first.subtitle);
    }

    let layout_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "layout-selection-panel"])
        .build();
    wizard_steps.add_named(&layout_panel, Some("layout"));
    layout_panel.append(&build_section_header(
        "Step 3",
        "Choose a layout",
        "Choose a template, project suggestion, or saved preset. Saving and launching happen on the final step.",
    ));
    layout_panel.append(&summary.root);

    let templates_header = gtk::Label::builder()
        .label("Templates")
        .halign(gtk::Align::Start)
        .css_classes(["eyebrow"])
        .build();
    layout_panel.append(&templates_header);

    let template_grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .row_spacing(8)
        .column_spacing(8)
        .min_children_per_line(4)
        .max_children_per_line(4)
        .homogeneous(true)
        .hexpand(true)
        .css_classes(["template-grid"])
        .build();
    layout_panel.append(&template_grid);

    let suggestions_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "launch-suggestions-panel"])
        .build();
    suggestions_section.append(&build_section_header(
        "Suggestions",
        "Project-aware workspaces",
        "Use detected project files to prefill a workspace tuned for the current folder.",
    ));
    let suggestions_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    suggestions_section.append(&suggestions_row);
    layout_panel.append(&suggestions_section);

    let tile_editor = build_tile_editor_panel();
    tile_editor
        .tile_count
        .set_value(active_layout.borrow().tile_count() as f64);
    refresh_tile_editor(&tile_editor, &active_layout, &assets);

    let template_buttons: Rc<std::cell::RefCell<Vec<gtk::Widget>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let preset_buttons: Rc<std::cell::RefCell<Vec<gtk::Widget>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    let assets_for_suggestions = assets.clone();
    rebuild_suggestion_panel(
        &suggestions_section,
        &suggestions_row,
        &suggestion_cards,
        &PathBuf::from(path_entry.text().as_str()),
        &assets_for_suggestions,
        {
            let summary = summary.clone();
            let active_layout = active_layout.clone();
            let tile_editor = tile_editor.clone();
            let session_name_entry = session_name_entry.clone();
            let assets = assets_for_suggestions.clone();
            move |suggestion| {
                apply_project_suggestion(
                    &suggestion,
                    &summary,
                    &active_layout,
                    &tile_editor,
                    &assets,
                    &session_name_entry,
                );
            }
        },
    );

    {
        let suggestions_section = suggestions_section.clone();
        let suggestions_row = suggestions_row.clone();
        let suggestion_cards = suggestion_cards.clone();
        let assets = assets.clone();
        let summary = summary.clone();
        let active_layout = active_layout.clone();
        let tile_editor = tile_editor.clone();
        let session_name_entry = session_name_entry.clone();
        let pending_rebuild: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        path_entry.connect_changed(move |entry| {
            if let Some(source_id) = pending_rebuild.borrow_mut().take() {
                source_id.remove();
            }

            let suggestions_section = suggestions_section.clone();
            let suggestions_row = suggestions_row.clone();
            let suggestion_cards = suggestion_cards.clone();
            let assets = assets.clone();
            let summary = summary.clone();
            let active_layout = active_layout.clone();
            let tile_editor = tile_editor.clone();
            let session_name_entry = session_name_entry.clone();
            let workspace_root = PathBuf::from(entry.text().as_str());
            let pending_rebuild_for_timeout = pending_rebuild.clone();
            let source_id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
                rebuild_suggestion_panel(
                    &suggestions_section,
                    &suggestions_row,
                    &suggestion_cards,
                    &workspace_root,
                    &assets,
                    {
                        let summary = summary.clone();
                        let active_layout = active_layout.clone();
                        let tile_editor = tile_editor.clone();
                        let session_name_entry = session_name_entry.clone();
                        let assets = assets.clone();
                        move |suggestion| {
                            apply_project_suggestion(
                                &suggestion,
                                &summary,
                                &active_layout,
                                &tile_editor,
                                &assets,
                                &session_name_entry,
                            );
                        }
                    },
                );
                pending_rebuild_for_timeout.borrow_mut().take();
            });
            *pending_rebuild.borrow_mut() = Some(source_id);
        });
    }

    {
        let tile_editor = tile_editor.clone();
        let active_layout = active_layout.clone();
        let summary = summary.clone();
        let tile_count = tile_editor.tile_count.clone();
        let assets = assets.clone();
        tile_count.connect_value_changed(move |spinner| {
            let requested = spinner.value_as_int().max(1) as usize;
            let next_layout = resize_layout(&active_layout.borrow(), requested);
            *active_layout.borrow_mut() = next_layout;
            refresh_tile_editor(&tile_editor, &active_layout, &assets);
            summary
                .subtitle_label
                .set_text(&format!("{} tiles configured", requested));
        });
    }

    for (index, template) in templates.iter().enumerate() {
        let button = build_template_button(template, index, {
            let selected = selected.clone();
            let summary = summary.clone();
            let template_buttons = template_buttons.clone();
            let preset_buttons = preset_buttons.clone();
            let active_layout = active_layout.clone();
            let tile_editor = tile_editor.clone();
            let theme_preview_callback = theme_preview_callback.clone();
            let session_name_entry = session_name_entry.clone();
            let chosen_theme = chosen_theme.clone();
            let chosen_density = chosen_density.clone();
            let theme_strip = theme_strip.clone();
            let density_strip = density_strip.clone();
            let density_preview_callback = density_preview_callback.clone();
            let assets = assets.clone();
            let label = template.label;
            let subtitle = template.subtitle;
            let tile_count = template.tile_count;

            move |idx| {
                selected.set(Selection::Template(idx));
                summary.name_label.set_text(label);
                summary.subtitle_label.set_text(subtitle);
                session_name_entry.set_text(label);
                chosen_theme.set(default_theme);
                theme_preview_callback(default_theme);
                sync_theme_strip_active(&theme_strip, default_theme);
                let density = chosen_density.get();
                density_preview_callback(density);
                sync_density_strip_active(&density_strip, density);
                *active_layout.borrow_mut() = generate_layout(tile_count);
                tile_editor.tile_count.set_value(tile_count as f64);
                refresh_tile_editor(&tile_editor, &active_layout, &assets);

                for (i, btn) in template_buttons.borrow().iter().enumerate() {
                    if i == idx {
                        btn.add_css_class("is-selected");
                    } else {
                        btn.remove_css_class("is-selected");
                    }
                }
                for btn in preset_buttons.borrow().iter() {
                    btn.remove_css_class("is-selected");
                }
            }
        });

        if index == 0 {
            button.add_css_class("is-selected");
        }

        template_buttons.borrow_mut().push(button.clone());
        template_grid.insert(&button, -1);
    }

    if !presets.is_empty() {
        let presets_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .css_classes(["config-panel", "presets-section", "launch-presets-panel"])
            .build();
        layout_panel.append(&presets_section);
        presets_section.append(&build_section_header(
            "Saved presets",
            "Reuse a preset",
            "Load an existing setup. You can save or update presets on the final step.",
        ));

        let presets_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .css_classes(["presets-scroll"])
            .build();

        let presets_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .build();
        presets_scroll.set_child(Some(&presets_row));
        presets_section.append(&presets_scroll);

        for (index, preset) in presets.iter().enumerate() {
            let card =
                build_compact_preset_card(preset, index, &preset_store, &on_presets_changed, {
                    let selected = selected.clone();
                    let summary = summary.clone();
                    let template_buttons = template_buttons.clone();
                    let preset_buttons = preset_buttons.clone();
                    let presets = presets.clone();
                    let active_layout = active_layout.clone();
                    let tile_editor = tile_editor.clone();
                    let theme_preview_callback = theme_preview_callback.clone();
                    let session_name_entry = session_name_entry.clone();
                    let path_entry = path_entry.clone();
                    let chosen_theme = chosen_theme.clone();
                    let chosen_density = chosen_density.clone();
                    let theme_strip = theme_strip.clone();
                    let density_strip = density_strip.clone();
                    let density_preview_callback = density_preview_callback.clone();
                    let assets = assets.clone();

                    move |idx| {
                        selected.set(Selection::Preset(idx));
                        let p = &presets[idx];
                        summary.name_label.set_text(&p.name);
                        summary.subtitle_label.set_text(&format!(
                            "{} - {}",
                            p.template_badge(),
                            p.description
                        ));
                        session_name_entry.set_text(&p.name);
                        if let Some(workspace_root) = p.workspace_root.as_ref() {
                            path_entry.set_text(&workspace_root.display().to_string());
                        }
                        chosen_theme.set(p.theme);
                        chosen_density.set(p.density);
                        theme_preview_callback(p.theme);
                        sync_theme_strip_active(&theme_strip, p.theme);
                        density_preview_callback(p.density);
                        sync_density_strip_active(&density_strip, p.density);
                        *active_layout.borrow_mut() = p.layout.clone();
                        tile_editor.tile_count.set_value(p.tile_count() as f64);
                        refresh_tile_editor(&tile_editor, &active_layout, &assets);

                        for btn in template_buttons.borrow().iter() {
                            btn.remove_css_class("is-selected");
                        }
                        for (i, btn) in preset_buttons.borrow().iter().enumerate() {
                            if i == idx {
                                btn.add_css_class("is-selected");
                            } else {
                                btn.remove_css_class("is-selected");
                            }
                        }
                    }
                });
            preset_buttons.borrow_mut().push(card.clone());
            presets_row.append(&card);
        }
    }

    wizard_steps.add_named(&tile_editor.root, Some("tiles"));

    let action_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .css_classes(["action-bar-bottom", "launch-action-bar"])
        .build();
    wizard.append(&action_bar);

    let dashboard_button = icons::labeled_button(
        "Workspaces",
        icon_name::WORKSPACES,
        &["pill-button", "secondary-button"],
    );
    {
        let mode_stack = mode_stack.clone();
        dashboard_button.connect_clicked(move |_| {
            mode_stack.set_visible_child_name("dashboard");
        });
    }
    action_bar.append(&dashboard_button);

    let cancel_button = icons::labeled_button(
        "Close Tab",
        icon_name::CLOSE,
        &["pill-button", "ghost-link-button"],
    );
    cancel_button.connect_clicked(move |_| on_cancel());
    action_bar.append(&cancel_button);

    let previous_button = icons::labeled_button(
        "Back",
        icon_name::BACK,
        &["pill-button", "secondary-button"],
    );
    action_bar.append(&previous_button);

    let spacer = gtk::Box::builder().hexpand(true).build();
    action_bar.append(&spacer);

    let preset_action_button = icons::labeled_button(
        "Save as Preset",
        icon_name::SAVE,
        &[
            "pill-button",
            "secondary-button",
            "new-preset-button",
            "final-preset-action-button",
        ],
    );
    preset_action_button.set_visible(false);
    {
        let selected = selected.clone();
        let templates_ref = builtin_templates();
        let presets = presets.clone();
        let preset_store = preset_store.clone();
        let on_presets_changed = on_presets_changed.clone();
        let session_name_entry = session_name_entry.clone();
        let path_entry = path_entry.clone();
        let chosen_theme = chosen_theme.clone();
        let chosen_density = chosen_density.clone();
        let active_layout = active_layout.clone();

        preset_action_button.connect_clicked(move |button| {
            let session_name = session_name_entry.text().to_string();
            let layout = active_layout.borrow().clone();
            handle_final_preset_action(FinalPresetAction {
                button,
                selected: &selected,
                templates: &templates_ref,
                presets: &presets,
                preset_store: &preset_store,
                on_presets_changed: &on_presets_changed,
                layout: &layout,
                session_name: &session_name,
                workspace_root: preset_workspace_root(&path_entry),
                theme: chosen_theme.get(),
                density: chosen_density.get(),
            });
        });
    }
    action_bar.append(&preset_action_button);

    let configure_button = icons::labeled_button(
        "Next",
        icon_name::NEXT,
        &["pill-button", "primary-cta-button"],
    );
    action_bar.append(&configure_button);

    let sync_wizard_navigation = Rc::new({
        let wizard_stepper = wizard_stepper.clone();
        let wizard_step_names = wizard_step_names.clone();
        let wizard_step_index = wizard_step_index.clone();
        let previous_button = previous_button.clone();
        let configure_button = configure_button.clone();
        let preset_action_button = preset_action_button.clone();
        let selected = selected.clone();
        let presets = presets.clone();
        move || {
            let index = wizard_step_index
                .get()
                .min(wizard_step_names.len().saturating_sub(1));
            wizard_step_index.set(index);
            previous_button.set_sensitive(index > 0);
            let is_final_step = index + 1 == wizard_step_names.len();
            preset_action_button.set_visible(is_final_step);
            icons::set_button_icon_label(
                &preset_action_button,
                final_preset_action_label(&selected, &presets),
                icon_name::SAVE,
            );
            if index + 1 == wizard_step_names.len() {
                icons::set_button_icon_label(
                    &configure_button,
                    "Launch Workspace",
                    icon_name::LAUNCH,
                );
            } else {
                icons::set_button_icon_label(&configure_button, "Next", icon_name::NEXT);
            }
            for (step_index, label) in wizard_stepper.steps.iter().enumerate() {
                label.remove_css_class("is-active");
                label.remove_css_class("is-complete");
                if step_index == index {
                    label.add_css_class("is-active");
                } else if step_index < index {
                    label.add_css_class("is-complete");
                }
            }
        }
    });

    let go_to_wizard_step: Rc<dyn Fn(usize)> = Rc::new({
        let wizard_steps = wizard_steps.clone();
        let wizard_step_names = wizard_step_names.clone();
        let wizard_step_index = wizard_step_index.clone();
        let sync_wizard_navigation = sync_wizard_navigation.clone();
        move |target_index| {
            let last_index = wizard_step_names.len().saturating_sub(1);
            let current_index = wizard_step_index.get().min(last_index);
            let next_index = target_index.min(last_index);
            let transition = if next_index > current_index {
                gtk::StackTransitionType::SlideLeft
            } else {
                gtk::StackTransitionType::SlideRight
            };

            wizard_step_index.set(next_index);
            if next_index == current_index {
                sync_wizard_navigation();
                return;
            }
            wizard_steps.set_visible_child_full(wizard_step_names[next_index], transition);
            sync_wizard_navigation();
        }
    });

    for (step_index, step_button) in wizard_stepper.steps.iter().enumerate() {
        let go_to_wizard_step = go_to_wizard_step.clone();
        step_button.connect_clicked(move |_| {
            go_to_wizard_step(step_index);
        });
    }

    {
        let wizard_step_index = wizard_step_index.clone();
        let go_to_wizard_step = go_to_wizard_step.clone();
        previous_button.connect_clicked(move |_| {
            let index = wizard_step_index.get();
            if index > 0 {
                go_to_wizard_step(index - 1);
            }
        });
    }

    {
        let path_entry = path_entry.clone();
        let selected = selected.clone();
        let presets = presets.clone();
        let launch_callback = launch_callback.clone();
        let templates_ref = builtin_templates();
        let session_name_entry = session_name_entry.clone();
        let chosen_theme = chosen_theme.clone();
        let chosen_density = chosen_density.clone();
        let active_layout = active_layout.clone();
        let wizard_step_index = wizard_step_index.clone();
        let wizard_step_names = wizard_step_names.clone();
        let go_to_wizard_step = go_to_wizard_step.clone();

        configure_button.connect_clicked(move |_| {
            let index = wizard_step_index.get();
            if index + 1 < wizard_step_names.len() {
                go_to_wizard_step(index + 1);
                return;
            }

            match validate_workspace_path(&path_entry) {
                Ok(workspace_root) => {
                    let session_name = session_name_entry.text().to_string();
                    let preset = build_launch_preset(LaunchPresetDraft {
                        selected: &selected,
                        templates: &templates_ref,
                        presets: &presets,
                        layout: &active_layout.borrow().clone(),
                        session_name: &session_name,
                        workspace_root: Some(workspace_root.clone()),
                        theme: chosen_theme.get(),
                        density: chosen_density.get(),
                    });
                    logging::info(format!(
                        "launching preset '{}' with {} tiles",
                        preset.name,
                        preset.tile_count()
                    ));
                    launch_callback(preset, workspace_root);
                }
                Err(msg) => {
                    logging::error(format!("Cannot launch: {}", msg));
                }
            }
        });
    }

    go_to_wizard_step(0);

    let sync_board_review = Rc::new({
        let board_name_entry = board_name_entry.clone();
        let board_path_entry = board_path_entry.clone();
        let board_review_name = board_review_name.clone();
        let board_review_path = board_review_path.clone();
        move || {
            let name = board_name_entry.text().trim().to_string();
            board_review_name.set_text(if name.is_empty() {
                "Untitled Kanban Board"
            } else {
                &name
            });
            board_review_path.set_text(&format!(
                "Project directory: {}",
                board_path_entry.text().as_str()
            ));
        }
    });
    sync_board_review();

    let sync_board_wizard_navigation = Rc::new({
        let board_stepper = board_stepper.clone();
        let board_step_names = board_step_names.clone();
        let board_step_index = board_step_index.clone();
        let board_back_button = board_back_button.clone();
        let board_next_button = board_next_button.clone();
        let sync_board_review = sync_board_review.clone();
        move || {
            let index = board_step_index
                .get()
                .min(board_step_names.len().saturating_sub(1));
            board_step_index.set(index);
            board_back_button.set_sensitive(index > 0);
            if index + 1 == board_step_names.len() {
                sync_board_review();
                icons::set_button_icon_label(
                    &board_next_button,
                    "Open Kanban Board",
                    icon_name::LAUNCH,
                );
            } else {
                icons::set_button_icon_label(&board_next_button, "Next", icon_name::NEXT);
            }
            for (step_index, label) in board_stepper.steps.iter().enumerate() {
                label.remove_css_class("is-active");
                label.remove_css_class("is-complete");
                if step_index == index {
                    label.add_css_class("is-active");
                } else if step_index < index {
                    label.add_css_class("is-complete");
                }
            }
        }
    });

    let go_to_board_step: Rc<dyn Fn(usize)> = Rc::new({
        let board_steps = board_steps.clone();
        let board_step_names = board_step_names.clone();
        let board_step_index = board_step_index.clone();
        let sync_board_wizard_navigation = sync_board_wizard_navigation.clone();
        move |target_index| {
            let last_index = board_step_names.len().saturating_sub(1);
            let current_index = board_step_index.get().min(last_index);
            let next_index = target_index.min(last_index);
            let transition = if next_index > current_index {
                gtk::StackTransitionType::SlideLeft
            } else {
                gtk::StackTransitionType::SlideRight
            };

            board_step_index.set(next_index);
            if next_index == current_index {
                sync_board_wizard_navigation();
                return;
            }
            board_steps.set_visible_child_full(board_step_names[next_index], transition);
            sync_board_wizard_navigation();
        }
    });

    for (step_index, step_button) in board_stepper.steps.iter().enumerate() {
        let go_to_board_step = go_to_board_step.clone();
        step_button.connect_clicked(move |_| {
            go_to_board_step(step_index);
        });
    }

    {
        let board_step_index = board_step_index.clone();
        let go_to_board_step = go_to_board_step.clone();
        board_back_button.connect_clicked(move |_| {
            let index = board_step_index.get();
            if index > 0 {
                go_to_board_step(index - 1);
            }
        });
    }

    {
        let board_step_index = board_step_index.clone();
        let board_step_names = board_step_names.clone();
        let go_to_board_step = go_to_board_step.clone();
        let board_name_entry = board_name_entry.clone();
        let board_path_entry = board_path_entry.clone();
        let editing_board_id = editing_board_id.clone();
        let board_workspace_store = board_workspace_store.clone();
        let board_launch_callback = board_launch_callback.clone();
        board_next_button.connect_clicked(move |_| {
            let index = board_step_index.get();
            if index + 1 < board_step_names.len() {
                go_to_board_step(index + 1);
                return;
            }

            if board_workspace_store.is_none() || board_launch_callback.is_none() {
                return;
            }

            match build_board_launch_request(
                &board_name_entry,
                &board_path_entry,
                editing_board_id.borrow().clone(),
                default_theme,
                default_density,
            ) {
                Ok(request) => {
                    if let Some(store) = board_workspace_store.as_ref()
                        && let Err(error) = store.upsert_from_launch_request(request.clone())
                    {
                        logging::error(format!("Failed to save Kanban shortcut: {error}"));
                    }
                    if let Some(callback) = board_launch_callback.as_ref() {
                        callback(request);
                    }
                }
                Err(message) => {
                    logging::error(format!("Cannot open Kanban board: {message}"));
                }
            }
        });
    }
    go_to_board_step(0);

    let show_new_workspace_wizard: Rc<dyn Fn()> = Rc::new({
        let selected = selected.clone();
        let summary = summary.clone();
        let template_buttons = template_buttons.clone();
        let preset_buttons = preset_buttons.clone();
        let active_layout = active_layout.clone();
        let tile_editor = tile_editor.clone();
        let theme_preview_callback = theme_preview_callback.clone();
        let session_name_entry = session_name_entry.clone();
        let chosen_theme = chosen_theme.clone();
        let chosen_density = chosen_density.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let density_preview_callback = density_preview_callback.clone();
        let assets = assets.clone();
        let mode_stack = mode_stack.clone();
        let go_to_wizard_step = go_to_wizard_step.clone();
        move || {
            selected.set(Selection::Template(0));
            if let Some(template) = templates.first() {
                summary.name_label.set_text(template.label);
                summary.subtitle_label.set_text(template.subtitle);
                session_name_entry.set_text(template.label);
                *active_layout.borrow_mut() = generate_layout(template.tile_count);
                tile_editor.tile_count.set_value(template.tile_count as f64);
            }
            chosen_theme.set(default_theme);
            chosen_density.set(default_density);
            theme_preview_callback(default_theme);
            density_preview_callback(default_density);
            sync_theme_strip_active(&theme_strip, default_theme);
            sync_density_strip_active(&density_strip, default_density);
            refresh_tile_editor(&tile_editor, &active_layout, &assets);

            for (index, btn) in template_buttons.borrow().iter().enumerate() {
                if index == 0 {
                    btn.add_css_class("is-selected");
                } else {
                    btn.remove_css_class("is-selected");
                }
            }
            for btn in preset_buttons.borrow().iter() {
                btn.remove_css_class("is-selected");
            }

            go_to_wizard_step(0);
            mode_stack.set_visible_child_name("wizard");
        }
    });

    let show_new_board_wizard: Option<Rc<dyn Fn()>> = if board_launch_supported {
        let mode_stack = mode_stack.clone();
        let go_to_board_step = go_to_board_step.clone();
        let board_path_entry = board_path_entry.clone();
        let board_name_entry = board_name_entry.clone();
        let editing_board_id = editing_board_id.clone();
        let board_default_dir = current_dir.clone();
        Some(Rc::new(move || {
            *editing_board_id.borrow_mut() = None;
            board_path_entry.set_text(&board_default_dir.display().to_string());
            board_name_entry.set_text("Project Kanban");
            go_to_board_step(0);
            mode_stack.set_visible_child_name("board-wizard");
        }) as Rc<dyn Fn()>)
    } else {
        None
    };

    let edit_board_from_dashboard: Option<Rc<dyn Fn(usize)>> = if board_launch_supported {
        let board_workspaces = board_workspaces.clone();
        let mode_stack = mode_stack.clone();
        let go_to_board_step = go_to_board_step.clone();
        let board_path_entry = board_path_entry.clone();
        let board_name_entry = board_name_entry.clone();
        let editing_board_id = editing_board_id.clone();
        Some(Rc::new(move |idx: usize| {
            let Some(boards): Option<&Rc<Vec<BoardWorkspace>>> = board_workspaces.as_ref() else {
                return;
            };
            let boards: &Vec<BoardWorkspace> = Rc::as_ref(boards);
            let Some(board) = boards.get(idx) else {
                return;
            };
            *editing_board_id.borrow_mut() = Some(board.id.clone());
            board_path_entry.set_text(&board.project_root.display().to_string());
            board_name_entry.set_text(&board.name);
            go_to_board_step(0);
            mode_stack.set_visible_child_name("board-wizard");
        }) as Rc<dyn Fn(usize)>)
    } else {
        None
    };

    let open_board_from_dashboard: Option<Rc<dyn Fn(usize)>> = if board_launch_supported {
        let board_workspaces = board_workspaces.clone();
        let board_launch_callback = board_launch_callback.clone();
        let edit_board_from_dashboard = edit_board_from_dashboard.clone();
        Some(Rc::new(move |idx: usize| {
            let Some(boards): Option<&Rc<Vec<BoardWorkspace>>> = board_workspaces.as_ref() else {
                return;
            };
            let boards: &Vec<BoardWorkspace> = Rc::as_ref(boards);
            let Some(board) = boards.get(idx).cloned() else {
                return;
            };
            match resolve_workspace_root(&board.project_root) {
                Ok(project_root) if project_root.is_dir() => {
                    if let Some(callback) = board_launch_callback.as_ref() {
                        logging::info(format!(
                            "opening saved Kanban board '{}' from dashboard root='{}'",
                            board.name,
                            project_root.display()
                        ));
                        callback(BoardLaunchRequest {
                            id: Some(board.id),
                            name: board.name,
                            project_root,
                            theme: board.theme,
                            density: board.density,
                        });
                    }
                }
                Ok(project_root) => {
                    logging::error(format!(
                        "Saved Kanban project root is not a directory: {}",
                        project_root.display()
                    ));
                    if let Some(edit) = edit_board_from_dashboard.as_ref() {
                        edit(idx);
                    }
                }
                Err(error) => {
                    logging::error(format!(
                        "Cannot open saved Kanban board '{}': {}",
                        board.name, error
                    ));
                    if let Some(edit) = edit_board_from_dashboard.as_ref() {
                        edit(idx);
                    }
                }
            }
        }) as Rc<dyn Fn(usize)>)
    } else {
        None
    };

    let edit_workspace_from_dashboard: Rc<dyn Fn(usize)> = Rc::new({
        let selected = selected.clone();
        let summary = summary.clone();
        let template_buttons = template_buttons.clone();
        let preset_buttons = preset_buttons.clone();
        let presets = presets.clone();
        let active_layout = active_layout.clone();
        let tile_editor = tile_editor.clone();
        let theme_preview_callback = theme_preview_callback.clone();
        let session_name_entry = session_name_entry.clone();
        let path_entry = path_entry.clone();
        let chosen_theme = chosen_theme.clone();
        let chosen_density = chosen_density.clone();
        let theme_strip = theme_strip.clone();
        let density_strip = density_strip.clone();
        let density_preview_callback = density_preview_callback.clone();
        let assets = assets.clone();
        let mode_stack = mode_stack.clone();
        let go_to_wizard_step = go_to_wizard_step.clone();
        move |idx| {
            let Some(p) = presets.get(idx) else {
                return;
            };
            selected.set(Selection::Preset(idx));
            summary.name_label.set_text(&p.name);
            summary
                .subtitle_label
                .set_text(&format!("{} - {}", p.template_badge(), p.description));
            session_name_entry.set_text(&p.name);
            if let Some(workspace_root) = p.workspace_root.as_ref() {
                path_entry.set_text(&workspace_root.display().to_string());
            }
            chosen_theme.set(p.theme);
            chosen_density.set(p.density);
            theme_preview_callback(p.theme);
            sync_theme_strip_active(&theme_strip, p.theme);
            density_preview_callback(p.density);
            sync_density_strip_active(&density_strip, p.density);
            *active_layout.borrow_mut() = p.layout.clone();
            tile_editor.tile_count.set_value(p.tile_count() as f64);
            refresh_tile_editor(&tile_editor, &active_layout, &assets);

            for btn in template_buttons.borrow().iter() {
                btn.remove_css_class("is-selected");
            }
            for (index, btn) in preset_buttons.borrow().iter().enumerate() {
                if index == idx {
                    btn.add_css_class("is-selected");
                } else {
                    btn.remove_css_class("is-selected");
                }
            }

            go_to_wizard_step(0);
            mode_stack.set_visible_child_name("wizard");
        }
    });

    let open_workspace_from_dashboard: Rc<dyn Fn(usize)> = Rc::new({
        let presets = presets.clone();
        let launch_callback = launch_callback.clone();
        let current_dir = current_dir.clone();
        let edit_workspace_from_dashboard = edit_workspace_from_dashboard.clone();
        move |idx| {
            let Some(preset) = presets.get(idx).cloned() else {
                return;
            };
            let requested_root = preset
                .workspace_root
                .clone()
                .unwrap_or_else(|| current_dir.clone());
            match resolve_workspace_root(&requested_root) {
                Ok(workspace_root) if workspace_root.is_dir() => {
                    let mut launch_preset = preset.clone();
                    launch_preset.workspace_root = Some(workspace_root.clone());
                    logging::info(format!(
                        "opening saved workspace '{}' from dashboard root='{}'",
                        launch_preset.name,
                        workspace_root.display()
                    ));
                    launch_callback(launch_preset, workspace_root);
                }
                Ok(workspace_root) => {
                    logging::error(format!(
                        "Saved workspace root is not a directory: {}",
                        workspace_root.display()
                    ));
                    edit_workspace_from_dashboard(idx);
                }
                Err(error) => {
                    logging::error(format!(
                        "Cannot open saved workspace '{}': {}",
                        preset.name, error
                    ));
                    edit_workspace_from_dashboard(idx);
                }
            }
        }
    });

    let saved_board_count = board_workspaces
        .as_ref()
        .map(|boards| boards.len())
        .unwrap_or(0);
    dashboard.append(&build_dashboard_intro(
        presets.len(),
        saved_board_count,
        {
            let show_new_workspace_wizard = show_new_workspace_wizard.clone();
            move || show_new_workspace_wizard()
        },
        show_new_board_wizard.clone(),
    ));

    if presets.is_empty() && saved_board_count == 0 {
        dashboard.append(&build_dashboard_empty_state());
    } else {
        if let (Some(boards), Some(open_board), Some(edit_board), Some(store)) = (
            board_workspaces.as_ref(),
            open_board_from_dashboard.as_ref(),
            edit_board_from_dashboard.as_ref(),
            board_workspace_store.as_ref(),
        ) && !boards.is_empty()
        {
            let boards_panel = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(12)
                .css_classes(["config-panel", "saved-boards-panel"])
                .build();
            boards_panel.append(&build_section_header(
                "Saved Kanban boards",
                "Open or edit a project board",
                "Board cards are launch-deck bookmarks. Deleting one does not remove .terminaltiler/board.json.",
            ));

            let board_cards = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .row_spacing(12)
                .column_spacing(12)
                .min_children_per_line(1)
                .max_children_per_line(4)
                .homogeneous(true)
                .hexpand(true)
                .css_classes(["saved-workspace-grid", "saved-board-grid"])
                .build();
            for (index, board) in boards.iter().enumerate() {
                let open_board = open_board.clone();
                let edit_board = edit_board.clone();
                board_cards.insert(
                    &build_saved_board_card(
                        board,
                        index,
                        store,
                        &on_presets_changed,
                        move |idx| open_board(idx),
                        move |idx| edit_board(idx),
                    ),
                    -1,
                );
            }
            boards_panel.append(&board_cards);
            dashboard.append(&boards_panel);
        }

        if !presets.is_empty() {
            let saved_panel = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(12)
                .css_classes(["config-panel", "saved-workspaces-panel"])
                .build();
            saved_panel.append(&build_section_header(
                "Saved workspaces",
                "Load or edit an existing workspace",
                "Open a saved layout immediately, or edit it in the wizard before launching.",
            ));

            let cards = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .row_spacing(12)
                .column_spacing(12)
                .min_children_per_line(1)
                .max_children_per_line(4)
                .homogeneous(true)
                .hexpand(true)
                .css_classes(["saved-workspace-grid"])
                .build();
            for (index, preset) in presets.iter().enumerate() {
                cards.insert(
                    &build_saved_workspace_card(
                        preset,
                        index,
                        &preset_store,
                        &on_presets_changed,
                        {
                            let open_workspace_from_dashboard =
                                open_workspace_from_dashboard.clone();
                            move |idx| open_workspace_from_dashboard(idx)
                        },
                        {
                            let edit_workspace_from_dashboard =
                                edit_workspace_from_dashboard.clone();
                            move |idx| edit_workspace_from_dashboard(idx)
                        },
                    ),
                    -1,
                );
            }
            saved_panel.append(&cards);
            dashboard.append(&saved_panel);
        }
    }

    mode_stack.set_visible_child_name("dashboard");

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(320)
        .min_content_height(320)
        .child(&root)
        .css_classes(["launch-scroller"])
        .build();

    scroller.upcast()
}

fn build_wizard_stepper() -> WizardStepper {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["wizard-stepper", "config-panel"])
        .build();

    let mut steps = Vec::new();
    for (index, (label, icon)) in [
        ("Setup", icon_name::FOLDER),
        ("Appearance", icon_name::THEME),
        ("Layout", icon_name::LAYOUT),
        ("Review", icon_name::APPLY),
    ]
    .iter()
    .enumerate()
    {
        let step = build_wizard_step_button(index + 1, label, icon);
        root.append(&step);
        steps.push(step);
    }

    WizardStepper { root, steps }
}

fn build_board_wizard_stepper() -> WizardStepper {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["wizard-stepper", "config-panel", "board-wizard-stepper"])
        .build();

    let mut steps = Vec::new();
    for (index, (label, icon)) in [
        ("Project", icon_name::FOLDER),
        ("Agents", icon_name::TERMINAL),
        ("Open", icon_name::APPLY),
    ]
    .iter()
    .enumerate()
    {
        let step = build_wizard_step_button(index + 1, label, icon);
        root.append(&step);
        steps.push(step);
    }

    WizardStepper { root, steps }
}

fn build_wizard_step_button(index: usize, label: &str, icon_name: &str) -> gtk::Button {
    let step = gtk::Button::builder()
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .tooltip_text(format!("Go to step {index}: {label}"))
        .css_classes(["wizard-step-chip"])
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["wizard-step-chip-content"])
        .build();

    content.append(
        &gtk::Label::builder()
            .label(index.to_string())
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["wizard-step-index"])
            .build(),
    );

    let icon = icons::image(icon_name);
    icon.set_pixel_size(14);
    icon.set_valign(gtk::Align::Center);
    icon.add_css_class("wizard-step-icon");
    content.append(&icon);

    content.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .css_classes(["wizard-step-label"])
            .build(),
    );

    step.set_child(Some(&content));
    step
}

fn build_dashboard_intro<F>(
    saved_count: usize,
    saved_board_count: usize,
    on_new_workspace: F,
    on_new_board: Option<Rc<dyn Fn()>>,
) -> gtk::Widget
where
    F: Fn() + 'static,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .css_classes(["config-panel", "launch-dashboard-hero"])
        .build();

    let copy = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .hexpand(true)
        .build();
    copy.append(
        &gtk::Label::builder()
            .label("Launch dashboard")
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "launch-dashboard-kicker"])
            .build(),
    );
    copy.append(
        &gtk::Label::builder()
            .label("Saved workspace quick launch")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["hero-title", "launch-dashboard-title"])
            .build(),
    );
    copy.append(
        &gtk::Label::builder()
            .label(if saved_count == 0 && saved_board_count == 0 {
                "No saved workspaces yet. Use the workspace wizard to choose a folder and layout, or create a Kanban board directly from a project directory."
            } else {
                "Open a known workspace or Kanban board immediately, edit a saved setup, or start fresh."
            })
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["hero-body", "launch-dashboard-copy"])
            .build(),
    );
    let meta = gtk::Label::builder()
        .label(format!(
            "{} workspaces • {} boards",
            saved_count, saved_board_count
        ))
        .halign(gtk::Align::Start)
        .css_classes(["status-chip", "launch-dashboard-count"])
        .build();
    copy.append(&meta);
    card.append(&copy);

    let new_button = icons::labeled_button(
        "New Workspace Layout",
        icon_name::LAYOUT,
        &[
            "pill-button",
            "primary-cta-button",
            "new-workspace-layout-button",
        ],
    );
    new_button.set_halign(gtk::Align::End);
    new_button.set_valign(gtk::Align::Center);
    new_button.connect_clicked(move |_| on_new_workspace());
    card.append(&new_button);

    if let Some(on_new_board) = on_new_board {
        let board_button = icons::labeled_button(
            "New Kanban Board",
            icon_name::TERMINAL,
            &["pill-button", "secondary-button", "new-kanban-board-button"],
        );
        board_button.set_halign(gtk::Align::End);
        board_button.set_valign(gtk::Align::Center);
        board_button.connect_clicked(move |_| on_new_board());
        card.append(&board_button);
    }

    card.upcast()
}

fn build_dashboard_empty_state() -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "saved-workspaces-empty"])
        .build();
    card.append(
        &gtk::Label::builder()
            .label("Your saved workspaces will appear here")
            .halign(gtk::Align::Start)
            .css_classes(["section-title"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label("After you save a workspace preset, this dashboard becomes your quick launcher and editing hub.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );
    card.upcast()
}

fn build_saved_workspace_card<FOpen, FEdit>(
    preset: &WorkspacePreset,
    index: usize,
    preset_store: &Rc<PresetStore>,
    on_presets_changed: &Rc<dyn Fn()>,
    on_open: FOpen,
    on_edit: FEdit,
) -> gtk::Widget
where
    FOpen: Fn(usize) + 'static,
    FEdit: Fn(usize) + 'static,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .hexpand(false)
        .css_classes(["preset-card-compact", "saved-workspace-card"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    let name = gtk::Label::builder()
        .label(&preset.name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["card-title"])
        .build();
    header.append(&name);

    let tile_count = gtk::Label::builder()
        .label(format!("{} tiles", preset.tile_count()))
        .halign(gtk::Align::End)
        .css_classes(["status-chip", "saved-workspace-tile-chip"])
        .build();
    header.append(&tile_count);
    card.append(&header);

    // The tile-count chip already states the count, so the description line
    // carries only the human summary (and stays hidden when there is none)
    // rather than redundantly repeating "{N} tiles •".
    let detail = gtk::Label::builder()
        .label(&preset.description)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(48)
        .css_classes(["card-meta"])
        .build();
    detail.set_visible(!preset.description.trim().is_empty());
    card.append(&detail);

    // Absorbs slack so the footer (path + actions) pins to the card bottom,
    // keeping action rows aligned across a row of varying-length descriptions.
    let footer_spacer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    card.append(&footer_spacer);

    let root_label = preset
        .workspace_root
        .as_ref()
        .map(|root| root.display().to_string())
        .unwrap_or_else(|| "Uses current folder when opened".into());
    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["saved-workspace-footer"])
        .build();

    let root = gtk::Label::builder()
        .label(root_label)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(36)
        .css_classes(["field-hint", "saved-workspace-root"])
        .build();
    footer.append(&root);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .css_classes(["saved-workspace-actions"])
        .build();
    let open_button = icons::labeled_button(
        "Open",
        icon_name::OPEN,
        &[
            "pill-button",
            "primary-cta-button",
            "compact-action-button",
            "saved-workspace-open-button",
        ],
    );
    open_button.connect_clicked(move |_| on_open(index));
    actions.append(&open_button);

    let edit_button = icons::labeled_button(
        "Edit",
        icon_name::EDIT,
        &[
            "pill-button",
            "secondary-button",
            "compact-action-button",
            "saved-workspace-edit-button",
        ],
    );
    edit_button.connect_clicked(move |_| on_edit(index));
    actions.append(&edit_button);

    let delete_button = icons::icon_button(
        icon_name::DELETE,
        "Delete saved workspace",
        &[
            "pill-button",
            "destructive-button",
            "compact-icon-button",
            "saved-workspace-delete-button",
        ],
    );
    connect_delete_preset_button(&delete_button, preset, preset_store, on_presets_changed);
    actions.append(&delete_button);

    footer.append(&actions);
    card.append(&footer);

    card.upcast()
}

fn build_saved_board_card<FOpen, FEdit>(
    board: &BoardWorkspace,
    index: usize,
    board_workspace_store: &Rc<BoardWorkspaceStore>,
    on_boards_changed: &Rc<dyn Fn()>,
    on_open: FOpen,
    on_edit: FEdit,
) -> gtk::Widget
where
    FOpen: Fn(usize) + 'static,
    FEdit: Fn(usize) + 'static,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .hexpand(false)
        .css_classes([
            "preset-card-compact",
            "saved-workspace-card",
            "saved-board-card",
        ])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    header.append(
        &gtk::Label::builder()
            .label(&board.name)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["card-title"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label("Kanban")
            .halign(gtk::Align::End)
            .css_classes(["status-chip", "saved-board-kind-chip"])
            .build(),
    );
    card.append(&header);

    card.append(
        &gtk::Label::builder()
            .label("Per-project task board for humans and MCP-connected agents.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(48)
            .css_classes(["card-meta"])
            .build(),
    );

    let footer_spacer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    card.append(&footer_spacer);

    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["saved-workspace-footer", "saved-board-footer"])
        .build();
    footer.append(
        &gtk::Label::builder()
            .label(board.project_label())
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .max_width_chars(36)
            .css_classes(["field-hint", "saved-workspace-root", "saved-board-root"])
            .build(),
    );

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .css_classes(["saved-workspace-actions", "saved-board-actions"])
        .build();
    let open_button = icons::labeled_button(
        "Open",
        icon_name::OPEN,
        &[
            "pill-button",
            "primary-cta-button",
            "compact-action-button",
            "saved-board-open-button",
        ],
    );
    open_button.connect_clicked(move |_| on_open(index));
    actions.append(&open_button);

    let edit_button = icons::labeled_button(
        "Edit",
        icon_name::EDIT,
        &[
            "pill-button",
            "secondary-button",
            "compact-action-button",
            "saved-board-edit-button",
        ],
    );
    edit_button.connect_clicked(move |_| on_edit(index));
    actions.append(&edit_button);

    let delete_button = icons::icon_button(
        icon_name::DELETE,
        "Delete saved Kanban shortcut",
        &[
            "pill-button",
            "destructive-button",
            "compact-icon-button",
            "saved-board-delete-button",
        ],
    );
    connect_delete_board_button(
        &delete_button,
        board,
        board_workspace_store,
        on_boards_changed,
    );
    actions.append(&delete_button);
    footer.append(&actions);
    card.append(&footer);

    card.upcast()
}

fn connect_delete_board_button(
    button: &gtk::Button,
    board: &BoardWorkspace,
    board_workspace_store: &Rc<BoardWorkspaceStore>,
    on_boards_changed: &Rc<dyn Fn()>,
) {
    let board_id = board.id.clone();
    let board_name = board.name.clone();
    let board_workspace_store = board_workspace_store.clone();
    let on_boards_changed = on_boards_changed.clone();

    button.connect_clicked(move |button| {
        let window = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        present_delete_board_confirmation(
            window.as_ref(),
            board_id.clone(),
            board_name.clone(),
            board_workspace_store.clone(),
            on_boards_changed.clone(),
        );
    });
}

fn present_delete_board_confirmation(
    window: Option<&gtk::Window>,
    board_id: String,
    board_name: String,
    board_workspace_store: Rc<BoardWorkspaceStore>,
    on_boards_changed: Rc<dyn Fn()>,
) {
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .heading("Delete Kanban Shortcut?")
        .body(format!(
            "\"{}\" will be removed from the launch deck. The project board file stays on disk.",
            board_name
        ))
        .build();

    if let Some(win) = window {
        dialog.set_transient_for(Some(win));
        dialog_chrome::sync_dialog_chrome_classes(win, &dialog, "launch-delete-board-dialog");
    }

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete Shortcut");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dialog, response| {
        if response == "delete" {
            if let Err(err) = board_workspace_store.delete(&board_id) {
                logging::error(format!("Failed to delete Kanban shortcut: {}", err));
            } else {
                on_boards_changed();
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn build_header(default_restore_mode: RestoreLaunchMode) -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Fill)
        .css_classes(["launch-header", "config-panel", "launch-overview"])
        .build();

    let icon = gtk::Box::builder()
        .width_request(36)
        .height_request(36)
        .valign(gtk::Align::Center)
        .css_classes(["launch-overview-icon"])
        .build();
    icon.add_css_class("is-brand-logo");
    icon.append(&build_terminaltiler_logo_image());
    card.append(&icon);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["launch-overview-body"])
        .build();
    body.append(
        &gtk::Label::builder()
            .label("Workspace Launch Deck")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["hero-title", "config-title", "launch-overview-title"])
            .build(),
    );
    body.append(
        &gtk::Label::builder()
            .label("Open saved workspaces or create guided terminal layouts.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["hero-body", "config-subtitle", "launch-overview-copy"])
            .build(),
    );
    card.append(&body);

    let meta = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .css_classes(["launch-overview-meta"])
        .build();
    meta.append(&build_launch_meta_chip("Core"));
    meta.append(&build_launch_meta_chip("Wizard"));
    meta.append(&build_launch_meta_chip(default_restore_mode.label()));
    card.append(&meta);

    card.upcast()
}

fn build_terminaltiler_logo_image() -> gtk::Image {
    let logo_path = gtk_resource_path("terminaltiler.svg").unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/terminaltiler.svg")
    });
    let launch_icon = if logo_path.exists() {
        gtk::Image::from_file(logo_path)
    } else {
        gtk::Image::from_icon_name("terminaltiler")
    };
    launch_icon.set_valign(gtk::Align::Center);
    launch_icon.set_halign(gtk::Align::Center);
    launch_icon.set_pixel_size(28);
    launch_icon.set_size_request(28, 28);
    launch_icon.add_css_class("launch-overview-logo-image");
    launch_icon
}

fn gtk_resource_path(file_name: &str) -> Option<PathBuf> {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(file_name);
    if manifest_path.exists() {
        return Some(manifest_path);
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(app_root) = exe.parent()
    {
        let portable_path = app_root.join("share").join(file_name);
        if portable_path.exists() {
            return Some(portable_path);
        }
        if let Some(parent) = app_root.parent() {
            return Some(parent.join("share").join(file_name));
        }
    }

    Some(PathBuf::from("/usr/share/terminaltiler").join(file_name))
}

fn build_section_header(kicker: &str, title: &str, body: &str) -> gtk::Widget {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .css_classes(["section-header"])
        .build();

    let kicker_label = gtk::Label::builder()
        .label(kicker)
        .halign(gtk::Align::Start)
        .css_classes(["eyebrow"])
        .build();
    container.append(&kicker_label);

    let title_label = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["section-title"])
        .build();
    container.append(&title_label);

    let body_label = gtk::Label::builder()
        .label(body)
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    container.append(&body_label);

    container.upcast()
}

fn build_template_button<F>(template: &LayoutTemplate, index: usize, on_select: F) -> gtk::Widget
where
    F: Fn(usize) + 'static,
{
    let button = gtk::Button::builder()
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .css_classes(["preset-card", "template-button", "launch-template-card"])
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();

    let icon = build_template_icon(template.tile_count);
    content.append(&icon);

    let label = gtk::Label::builder()
        .label(template.label)
        .halign(gtk::Align::Center)
        .css_classes(["card-title"])
        .build();
    content.append(&label);

    let meta = gtk::Label::builder()
        .label(format!("{} tiles", template.tile_count))
        .halign(gtk::Align::Center)
        .css_classes(["card-meta"])
        .build();
    content.append(&meta);

    button.set_child(Some(&content));
    button.connect_clicked(move |_| {
        on_select(index);
    });

    button.upcast()
}

fn build_template_icon(tile_count: usize) -> gtk::Widget {
    let grid = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["template-icon-grid"])
        .build();

    let cols = optimal_columns(tile_count);

    for i in 0..tile_count {
        let row = (i / cols) as i32;
        let col = (i % cols) as i32;
        let cell = gtk::Box::builder()
            .width_request(10)
            .height_request(8)
            .css_classes(["template-icon-cell"])
            .build();
        grid.attach(&cell, col, row, 1, 1);
    }

    grid.upcast()
}

fn optimal_columns(tile_count: usize) -> usize {
    match tile_count {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 2,
        5 | 6 => 3,
        7 | 8 => 4,
        9 => 3,
        10 => 5,
        11 | 12 => 4,
        13 | 14 => 4,
        15 | 16 => 4,
        _ => 4,
    }
}

#[derive(Clone)]
struct SelectionSummary {
    root: gtk::Box,
    name_label: gtk::Label,
    subtitle_label: gtk::Label,
}

fn build_selection_summary() -> SelectionSummary {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes([
            "selection-summary",
            "config-panel",
            "launch-selection-summary",
        ])
        .build();

    let icon_box = gtk::Box::builder()
        .width_request(32)
        .height_request(32)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["template-icon-cell"])
        .build();
    root.append(&icon_box);

    let text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let name_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .css_classes(["selection-summary-name"])
        .build();
    let subtitle_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["selection-summary-subtitle"])
        .build();

    text_box.append(&name_label);
    text_box.append(&subtitle_label);
    root.append(&text_box);

    SelectionSummary {
        root,
        name_label,
        subtitle_label,
    }
}

fn build_compact_preset_card<F>(
    preset: &WorkspacePreset,
    index: usize,
    preset_store: &Rc<PresetStore>,
    on_presets_changed: &Rc<dyn Fn()>,
    on_select: F,
) -> gtk::Widget
where
    F: Fn(usize) + 'static,
{
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .css_classes(["preset-card-compact", "launch-preset-card"])
        .build();

    let top_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    shell.append(&top_row);

    let button = gtk::Button::builder()
        .hexpand(true)
        .css_classes(["flat", "preset-card-compact-button"])
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let name = gtk::Label::builder()
        .label(&preset.name)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["card-title"])
        .build();

    let detail = gtk::Label::builder()
        .label(preset.template_badge())
        .halign(gtk::Align::Start)
        .css_classes(["card-meta"])
        .build();

    let tags_text = if preset.tags.is_empty() {
        String::new()
    } else {
        preset
            .tags
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };

    content.append(&name);
    content.append(&detail);

    if !tags_text.is_empty() {
        let tags = gtk::Label::builder()
            .label(&tags_text)
            .halign(gtk::Align::Start)
            .css_classes(["card-meta"])
            .build();
        content.append(&tags);
    }

    button.set_child(Some(&content));
    button.connect_clicked(move |_| {
        on_select(index);
    });
    top_row.append(&button);

    let delete_button = gtk::Button::from_icon_name("window-close-symbolic");
    if let Some(img) = delete_button.first_child() {
        let _ = img.pango_context();
    }
    delete_button.add_css_class("flat");
    delete_button.add_css_class("preset-delete-button");
    delete_button.add_css_class("destructive-button");
    delete_button.set_valign(gtk::Align::Start);
    delete_button.set_tooltip_text(Some("Delete preset"));

    connect_delete_preset_button(&delete_button, preset, preset_store, on_presets_changed);

    top_row.append(&delete_button);

    shell.upcast()
}

fn connect_delete_preset_button(
    button: &gtk::Button,
    preset: &WorkspacePreset,
    preset_store: &Rc<PresetStore>,
    on_presets_changed: &Rc<dyn Fn()>,
) {
    let preset_id = preset.id.clone();
    let preset_name = preset.name.clone();
    let is_builtin = is_builtin_preset_id(&preset.id);
    let preset_store = preset_store.clone();
    let on_presets_changed = on_presets_changed.clone();

    button.connect_clicked(move |button| {
        let window = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        present_delete_preset_confirmation(
            window.as_ref(),
            preset_id.clone(),
            preset_name.clone(),
            is_builtin,
            preset_store.clone(),
            on_presets_changed.clone(),
        );
    });
}

fn present_delete_preset_confirmation(
    window: Option<&gtk::Window>,
    preset_id: String,
    preset_name: String,
    is_builtin: bool,
    preset_store: Rc<PresetStore>,
    on_presets_changed: Rc<dyn Fn()>,
) {
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .heading("Delete Preset?")
        .body(if is_builtin {
            format!(
                "\"{}\" will be removed. You can restore the shipped presets from Settings later.",
                preset_name
            )
        } else {
            format!("\"{}\" will be permanently removed.", preset_name)
        })
        .build();

    if let Some(win) = window {
        dialog.set_transient_for(Some(win));
        dialog_chrome::sync_dialog_chrome_classes(win, &dialog, "launch-delete-preset-dialog");
    }

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |dialog, response| {
        if response == "delete" {
            if let Err(err) = preset_store.delete_preset(&preset_id) {
                logging::error(format!("Failed to delete preset: {}", err));
            } else {
                on_presets_changed();
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn build_tile_editor_panel() -> TileEditorPanel {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "tile-editor-panel"])
        .build();

    root.append(&build_section_header(
        "Step 4",
        "Review & launch",
        "Finalize tile behavior, then save or update the preset before launching if you want to reuse it.",
    ));

    let count_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["tile-count-row"])
        .build();

    let count_label = gtk::Label::builder()
        .label("Tiles")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["section-title"])
        .build();
    count_row.append(&count_label);

    let status_label = gtk::Label::builder()
        .halign(gtk::Align::End)
        .css_classes(["card-meta"])
        .build();
    count_row.append(&status_label);

    let tile_count = gtk::SpinButton::with_range(1.0, 16.0, 1.0);
    tile_count.set_numeric(true);
    tile_count.set_width_chars(3);
    tile_count.add_css_class("tile-count-input");
    count_row.append(&tile_count);
    root.append(&count_row);

    let rows = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .min_content_height(168)
        .max_content_height(400)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["tile-editor-scroll"])
        .child(&rows)
        .build();
    root.append(&scroller);

    TileEditorPanel {
        root,
        tile_count,
        status_label,
        rows,
        scroller,
    }
}

fn build_launch_control_row(
    title: &str,
    note: &str,
    control: &impl IsA<gtk::Widget>,
) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .css_classes(["launch-setting-row"])
        .build();

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["launch-setting-copy"])
        .build();
    text.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["eyebrow", "launch-setting-title"])
            .build(),
    );
    text.append(
        &gtk::Label::builder()
            .label(note)
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint", "launch-setting-note"])
            .build(),
    );
    row.append(&text);
    row.append(control);
    row.upcast()
}

fn build_launch_meta_chip(label: &str) -> gtk::Widget {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::End)
        .css_classes(["status-chip", "launch-meta-chip"])
        .build()
        .upcast()
}

fn refresh_tile_editor(
    panel: &TileEditorPanel,
    layout_state: &Rc<RefCell<LayoutNode>>,
    assets: &Rc<WorkspaceAssets>,
) {
    while let Some(child) = panel.rows.first_child() {
        panel.rows.remove(&child);
    }

    let tile_specs = layout_state.borrow().tile_specs();
    panel
        .status_label
        .set_text(&format!("{} configured", tile_specs.len()));

    let clamped_rows = tile_specs.len().clamp(1, 4) as i32;
    let desired_height = 36 + (clamped_rows * 88);
    panel
        .scroller
        .set_min_content_height(desired_height.clamp(148, 388));
    panel
        .scroller
        .set_max_content_height((desired_height + 24).clamp(196, 428));
    panel
        .scroller
        .set_vscrollbar_policy(if tile_specs.len() > 4 {
            gtk::PolicyType::Automatic
        } else {
            gtk::PolicyType::Never
        });

    for (index, tile) in tile_specs.iter().enumerate() {
        panel.rows.append(&build_tile_editor_row(
            index,
            tile,
            panel,
            layout_state,
            assets,
        ));
    }
}

fn rebuild_suggestion_panel<F>(
    section: &gtk::Box,
    row: &gtk::Box,
    cards: &Rc<RefCell<Vec<gtk::Widget>>>,
    workspace_root: &Path,
    assets: &Rc<WorkspaceAssets>,
    on_select: F,
) where
    F: Fn(crate::model::assets::ProjectSuggestion) + Clone + 'static,
{
    for card in cards.borrow_mut().drain(..) {
        row.remove(&card);
    }

    let suggestions = detect_project_suggestions(workspace_root);
    let role_names_by_id = assets
        .role_templates
        .iter()
        .map(|role| (role.id.as_str(), role.name.as_str()))
        .collect::<HashMap<_, _>>();
    section.set_visible(!suggestions.is_empty());

    for suggestion in suggestions {
        let button = gtk::Button::builder()
            .css_classes(["preset-card", "template-button", "launch-template-card"])
            .hexpand(true)
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Start)
            .build();
        content.append(
            &gtk::Label::builder()
                .label(&suggestion.title)
                .halign(gtk::Align::Start)
                .css_classes(["card-title"])
                .build(),
        );
        content.append(
            &gtk::Label::builder()
                .label(&suggestion.description)
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["card-meta"])
                .build(),
        );
        let role_names = suggestion
            .role_ids
            .iter()
            .filter_map(|role_id| role_names_by_id.get(role_id.as_str()).copied())
            .collect::<Vec<_>>()
            .join(", ");
        content.append(
            &gtk::Label::builder()
                .label(format!(
                    "{} tiles  •  {}",
                    suggestion.tile_count, role_names
                ))
                .halign(gtk::Align::Start)
                .css_classes(["card-meta"])
                .build(),
        );
        button.set_child(Some(&content));
        let on_select = on_select.clone();
        button.connect_clicked(move |_| {
            on_select(suggestion.clone());
        });
        cards.borrow_mut().push(button.clone().upcast());
        row.append(&button);
    }
}

fn apply_project_suggestion(
    suggestion: &crate::model::assets::ProjectSuggestion,
    summary: &SelectionSummary,
    layout_state: &Rc<RefCell<LayoutNode>>,
    tile_editor: &TileEditorPanel,
    assets: &Rc<WorkspaceAssets>,
    session_name_entry: &gtk::Entry,
) {
    let layout = apply_suggestion_to_layout(&layout_state.borrow(), suggestion, assets);
    *layout_state.borrow_mut() = layout;
    tile_editor
        .tile_count
        .set_value(suggestion.tile_count as f64);
    refresh_tile_editor(tile_editor, layout_state, assets);
    summary.name_label.set_text(&suggestion.title);
    summary.subtitle_label.set_text(&suggestion.description);
    session_name_entry.set_text(&suggestion.title);
}

struct LaunchPresetDraft<'a> {
    selected: &'a Rc<Cell<Selection>>,
    templates: &'a [LayoutTemplate],
    presets: &'a [WorkspacePreset],
    layout: &'a LayoutNode,
    session_name: &'a str,
    workspace_root: Option<PathBuf>,
    theme: ThemeMode,
    density: ApplicationDensity,
}

fn build_launch_preset(draft: LaunchPresetDraft<'_>) -> WorkspacePreset {
    let custom_name = if draft.session_name.is_empty() {
        None
    } else {
        Some(draft.session_name.to_string())
    };

    match draft.selected.get() {
        Selection::Template(idx) => {
            let template = &draft.templates[idx];
            WorkspacePreset {
                id: format!("session-{}", template.tile_count),
                name: custom_name.unwrap_or_else(|| template.label.to_string()),
                description: String::new(),
                tags: Vec::new(),
                root_label: "Workspace root".into(),
                workspace_root: draft.workspace_root,
                theme: draft.theme,
                density: draft.density,
                layout: draft.layout.clone(),
            }
        }
        Selection::Preset(idx) => {
            let mut preset = draft.presets[idx].clone();
            if let Some(name) = custom_name {
                preset.name = name;
            }
            preset.theme = draft.theme;
            preset.density = draft.density;
            preset.workspace_root = draft.workspace_root;
            preset.layout = draft.layout.clone();
            preset
        }
    }
}

struct FinalPresetAction<'a> {
    button: &'a gtk::Button,
    selected: &'a Rc<Cell<Selection>>,
    templates: &'a [LayoutTemplate],
    presets: &'a [WorkspacePreset],
    preset_store: &'a Rc<PresetStore>,
    on_presets_changed: &'a Rc<dyn Fn()>,
    layout: &'a LayoutNode,
    session_name: &'a str,
    workspace_root: Option<PathBuf>,
    theme: ThemeMode,
    density: ApplicationDensity,
}

fn final_preset_action_label(
    selected: &Rc<Cell<Selection>>,
    presets: &[WorkspacePreset],
) -> &'static str {
    match selected.get() {
        Selection::Template(_) => "Save as Preset",
        Selection::Preset(index) => presets
            .get(index)
            .map(|preset| {
                if is_builtin_preset_id(&preset.id) {
                    "Save Copy"
                } else {
                    "Update Preset"
                }
            })
            .unwrap_or("Save as Preset"),
    }
}

fn handle_final_preset_action(action: FinalPresetAction<'_>) {
    match action.selected.get() {
        Selection::Template(_) => {
            let preset = build_action_preset(&action);
            let default_name = default_new_preset_name(
                action.selected,
                action.templates,
                action.presets,
                action.session_name,
            );
            prompt_save_preset(
                action.button,
                default_name,
                preset,
                "Failed to save preset",
                action.preset_store,
                action.on_presets_changed,
            );
        }
        Selection::Preset(index) => {
            let Some(existing) = action.presets.get(index) else {
                return;
            };

            let mut preset = build_action_preset(&action);
            if is_builtin_preset_id(&existing.id) {
                let default_name = default_copy_preset_name(existing, action.session_name);
                prompt_save_preset(
                    action.button,
                    default_name,
                    preset,
                    "Failed to save preset copy",
                    action.preset_store,
                    action.on_presets_changed,
                );
            } else {
                preset.id = existing.id.clone();
                if let Err(err) = action.preset_store.upsert_preset(preset) {
                    logging::error(format!("Failed to update preset: {}", err));
                } else {
                    (action.on_presets_changed)();
                }
            }
        }
    }
}

fn build_action_preset(action: &FinalPresetAction<'_>) -> WorkspacePreset {
    build_launch_preset(LaunchPresetDraft {
        selected: action.selected,
        templates: action.templates,
        presets: action.presets,
        layout: action.layout,
        session_name: action.session_name,
        workspace_root: action.workspace_root.clone(),
        theme: action.theme,
        density: action.density,
    })
}

fn default_new_preset_name(
    selected: &Rc<Cell<Selection>>,
    templates: &[LayoutTemplate],
    presets: &[WorkspacePreset],
    session_name: &str,
) -> String {
    let trimmed = session_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    match selected.get() {
        Selection::Template(index) => templates
            .get(index)
            .map(|template| template.label.to_string())
            .unwrap_or_else(|| "New Preset".into()),
        Selection::Preset(index) => presets
            .get(index)
            .map(|preset| preset.name.clone())
            .unwrap_or_else(|| "New Preset".into()),
    }
}

fn default_copy_preset_name(existing: &WorkspacePreset, session_name: &str) -> String {
    let trimmed = session_name.trim();
    if trimmed.is_empty() {
        format!("{} Copy", existing.name)
    } else {
        trimmed.to_string()
    }
}

fn prompt_save_preset(
    button: &gtk::Button,
    default_name: String,
    base_preset: WorkspacePreset,
    error_context: &'static str,
    preset_store: &Rc<PresetStore>,
    on_presets_changed: &Rc<dyn Fn()>,
) {
    let window = button
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok());
    let preset_store = preset_store.clone();
    let on_presets_changed = on_presets_changed.clone();

    prompt_preset_name(window.as_ref(), &default_name, move |name| {
        let mut preset = base_preset.clone();
        preset.id = unique_preset_id(&name);
        preset.name = name;

        if let Err(err) = preset_store.upsert_preset(preset) {
            logging::error(format!("{}: {}", error_context, err));
        } else {
            on_presets_changed();
        }
    });
}

fn sync_theme_strip_active(strip: &gtk::Box, active_theme: ThemeMode) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_theme.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}

fn sync_density_strip_active(strip: &gtk::Box, active_density: ApplicationDensity) {
    let mut child = strip.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        widget.remove_css_class("is-active");
        if let Ok(button) = widget.clone().downcast::<gtk::Button>()
            && button.label().as_deref() == Some(active_density.label())
        {
            button.add_css_class("is-active");
        }
        child = next;
    }
}

fn build_tile_editor_row(
    index: usize,
    tile: &TileSpec,
    panel: &TileEditorPanel,
    layout_state: &Rc<RefCell<LayoutNode>>,
    assets: &Rc<WorkspaceAssets>,
) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["tile-editor-row"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let label = gtk::Label::builder()
        .label(format!("Tile {}", index + 1))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .css_classes(["card-title"])
        .build();
    header.append(&label);

    let directory = gtk::Label::builder()
        .label(tile.working_directory.short_label())
        .halign(gtk::Align::End)
        .css_classes(["status-chip", "muted-chip"])
        .build();

    let split_horizontal = gtk::Button::builder()
        .icon_name("view-split-left-right-symbolic")
        .tooltip_text("Split tile horizontally")
        .css_classes(["flat", "surface-button", "surface-button-icon"])
        .build();
    // Prime the internal GtkImage's Pango context before attachment to the
    // realized tree.  gtk_image_get_baseline_align() computes ascent/(ascent+
    // descent) and returns NaN when both metrics are 0 — this happens because
    // gtk_widget_real_css_changed only updates the Pango context when
    // peek_pango_context() is non-NULL ("has_text" gate).  A freshly-created
    // GtkImage never calls get_pango_context() until its first measure, so the
    // gate is false during the initial CSS-change on parent attachment and the
    // context ends up with stale/zero metrics.  Calling pango_context() now
    // creates the context early; the subsequent CSS change then sees has_text=
    // true, updates the context with valid inherited font metrics, and no NaN
    // is produced.  Affects all icon buttons appended to an already-realized
    // GtkBox.
    if let Some(img) = split_horizontal.first_child() {
        let _ = img.pango_context();
    }
    let split_vertical = gtk::Button::builder()
        .icon_name("view-split-top-bottom-symbolic")
        .tooltip_text("Split tile vertically")
        .css_classes(["flat", "surface-button", "surface-button-icon"])
        .build();
    if let Some(img) = split_vertical.first_child() {
        let _ = img.pango_context();
    }
    let clone_tile = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Clone tile")
        .css_classes(["flat", "surface-button", "surface-button-icon"])
        .build();
    if let Some(img) = clone_tile.first_child() {
        let _ = img.pango_context();
    }
    let close_tile_button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Close tile")
        .css_classes(["flat", "surface-button", "surface-button-icon"])
        .sensitive(layout_state.borrow().tile_count() > 1)
        .build();
    if let Some(img) = close_tile_button.first_child() {
        let _ = img.pango_context();
    }

    header.append(&split_horizontal);
    header.append(&split_vertical);
    header.append(&clone_tile);
    header.append(&close_tile_button);
    header.append(&directory);
    row.append(&header);

    let kind_combo = gtk::ComboBoxText::new();
    kind_combo.add_css_class("surface-select-control");
    kind_combo.append(Some("terminal"), TileKind::Terminal.label());
    kind_combo.append(Some("web-view"), TileKind::WebView.label());
    kind_combo.set_active_id(Some(match tile.tile_kind {
        TileKind::Terminal => "terminal",
        TileKind::WebView => "web-view",
    }));
    row.append(&build_launch_control_row(
        "Tile kind",
        "Terminal tiles run commands. Web View tiles open a browser pane.",
        &kind_combo,
    ));

    let details = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let title_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(&tile.title)
        .placeholder_text("Tile title")
        .build();
    title_entry.add_css_class("tile-editor-input");
    details.append(&title_entry);

    let agent_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(&tile.agent_label)
        .placeholder_text("Agent label")
        .build();
    agent_entry.add_css_class("tile-editor-input");
    details.append(&agent_entry);
    row.append(&details);

    let command_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(tile.startup_command.as_deref().unwrap_or(""))
        .placeholder_text("Startup command, for example: codex --approval-mode auto")
        .build();
    command_entry.add_css_class("tile-editor-input");
    row.append(&command_entry);

    let routing = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let role_combo = gtk::ComboBoxText::new();
    role_combo.add_css_class("surface-select-control");
    role_combo.append(Some(""), "No role");
    for role in &assets.role_templates {
        role_combo.append(Some(&role.id), &role.name);
    }
    role_combo.set_active_id(tile.applied_role_id.as_deref());
    routing.append(&role_combo);

    let connection_combo = gtk::ComboBoxText::new();
    connection_combo.add_css_class("surface-select-control");
    connection_combo.append(Some("__local__"), "Local");
    for profile in &assets.connection_profiles {
        connection_combo.append(Some(&profile.id), &profile.name);
    }
    connection_combo.set_active_id(Some(match &tile.connection_target {
        crate::model::assets::TileConnectionTarget::Local => "__local__",
        crate::model::assets::TileConnectionTarget::Profile(profile_id) => profile_id.as_str(),
    }));
    routing.append(&connection_combo);

    let groups_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(tile.pane_groups.join(", "))
        .placeholder_text("Pane groups, for example: delivery, ops")
        .build();
    groups_entry.add_css_class("tile-editor-input");
    routing.append(&groups_entry);
    row.append(&routing);

    let web_settings = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let url_entry = gtk::Entry::builder()
        .hexpand(true)
        .text(tile.url.as_deref().unwrap_or(""))
        .placeholder_text("https://example.com")
        .build();
    url_entry.add_css_class("tile-editor-input");
    web_settings.append(&url_entry);

    let auto_refresh = gtk::SpinButton::with_range(0.0, 3600.0, 5.0);
    auto_refresh.set_numeric(true);
    auto_refresh.set_width_chars(6);
    auto_refresh.add_css_class("tile-count-input");
    auto_refresh.set_tooltip_text(Some(
        "Auto-refresh in seconds, 0 disables automatic reload.",
    ));
    auto_refresh.set_value(tile.auto_refresh_seconds.unwrap_or_default() as f64);
    web_settings.append(&auto_refresh);

    let web_settings_row = build_launch_control_row(
        "Web settings",
        "Set the initial URL and optional auto-refresh interval for this browser tile.",
        &web_settings,
    );
    row.append(&web_settings_row);

    let directory_hint = gtk::Label::builder()
        .label(tile_editor_hint(tile, assets))
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    row.append(&directory_hint);

    let sync_visibility = Rc::new({
        let command_entry = command_entry.clone();
        let routing = routing.clone();
        let web_settings_row = web_settings_row.clone();
        move |tile_kind: TileKind| {
            let is_terminal = tile_kind == TileKind::Terminal;
            command_entry.set_visible(is_terminal);
            routing.set_visible(is_terminal);
            web_settings_row.set_visible(!is_terminal);
        }
    });
    sync_visibility(tile.tile_kind);

    let refresh_hint = Rc::new({
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let directory_hint = directory_hint.clone();
        move || {
            if let Some(tile) = layout_state.borrow().tile_spec_at(index) {
                directory_hint.set_text(&tile_editor_hint(tile, &assets));
            }
        }
    });

    {
        let panel = panel.clone();
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let tile_id = tile.id.clone();
        split_horizontal.connect_clicked(move |_| {
            if let Some(next_layout) = split_tile(
                &layout_state.borrow(),
                &tile_id,
                SplitAxis::Horizontal,
                false,
            ) {
                *layout_state.borrow_mut() = next_layout;
                panel
                    .tile_count
                    .set_value(layout_state.borrow().tile_count() as f64);
                refresh_tile_editor(&panel, &layout_state, &assets);
            }
        });
    }

    {
        let panel = panel.clone();
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let tile_id = tile.id.clone();
        split_vertical.connect_clicked(move |_| {
            if let Some(next_layout) =
                split_tile(&layout_state.borrow(), &tile_id, SplitAxis::Vertical, false)
            {
                *layout_state.borrow_mut() = next_layout;
                panel
                    .tile_count
                    .set_value(layout_state.borrow().tile_count() as f64);
                refresh_tile_editor(&panel, &layout_state, &assets);
            }
        });
    }

    {
        let panel = panel.clone();
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let tile_id = tile.id.clone();
        clone_tile.connect_clicked(move |_| {
            if let Some(next_layout) = split_tile(
                &layout_state.borrow(),
                &tile_id,
                SplitAxis::Horizontal,
                true,
            ) {
                *layout_state.borrow_mut() = next_layout;
                panel
                    .tile_count
                    .set_value(layout_state.borrow().tile_count() as f64);
                refresh_tile_editor(&panel, &layout_state, &assets);
            }
        });
    }

    {
        let panel = panel.clone();
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let tile_id = tile.id.clone();
        close_tile_button.connect_clicked(move |_| {
            if let Some(next_layout) = close_tile(&layout_state.borrow(), &tile_id) {
                *layout_state.borrow_mut() = next_layout;
                panel
                    .tile_count
                    .set_value(layout_state.borrow().tile_count() as f64);
                refresh_tile_editor(&panel, &layout_state, &assets);
            }
        });
    }

    {
        let layout_state = layout_state.clone();
        title_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                tile.title = entry.text().to_string();
            });
        });
    }

    {
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let refresh_hint = refresh_hint.clone();
        agent_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                tile.agent_label = entry.text().to_string();
                if tile.applied_role_id.is_none() {
                    tile.accent_class = accent_class_for_agent(&tile.agent_label);
                } else if let Some(role) = resolve_role(&assets, tile.applied_role_id.as_deref()) {
                    tile.accent_class = role.accent_class.clone();
                }
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let refresh_hint = refresh_hint.clone();
        command_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                let value = entry.text().trim().to_string();
                tile.startup_command = if value.is_empty() { None } else { Some(value) };
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let assets = assets.clone();
        let refresh_hint = refresh_hint.clone();
        role_combo.connect_changed(move |combo| {
            update_tile_spec(&layout_state, index, |tile| {
                let active = combo.active_id().map(|value| value.to_string());
                if active.as_deref().is_none_or(|value| value.is_empty()) {
                    tile.applied_role_id = None;
                    return;
                }
                if let Some(role) = resolve_role(&assets, active.as_deref()) {
                    apply_role_to_tile(tile, role);
                }
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let refresh_hint = refresh_hint.clone();
        connection_combo.connect_changed(move |combo| {
            update_tile_spec(&layout_state, index, |tile| {
                tile.connection_target = match combo.active_id().as_deref() {
                    Some("__local__") | None => crate::model::assets::TileConnectionTarget::Local,
                    Some(profile_id) => {
                        crate::model::assets::TileConnectionTarget::Profile(profile_id.to_string())
                    }
                };
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let refresh_hint = refresh_hint.clone();
        groups_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                tile.pane_groups = entry
                    .text()
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let sync_visibility = sync_visibility.clone();
        let refresh_hint = refresh_hint.clone();
        kind_combo.connect_changed(move |combo| {
            let next_kind = match combo.active_id().as_deref() {
                Some("web-view") => TileKind::WebView,
                _ => TileKind::Terminal,
            };
            update_tile_spec(&layout_state, index, |tile| {
                tile.tile_kind = next_kind;
                if tile.tile_kind == TileKind::WebView && tile.url.is_none() {
                    tile.url = Some(DEFAULT_WEB_URL.into());
                }
                if tile.tile_kind == TileKind::WebView {
                    tile.startup_command = None;
                    tile.applied_role_id = None;
                    tile.connection_target = crate::model::assets::TileConnectionTarget::Local;
                    tile.output_helpers.clear();
                }
            });
            sync_visibility(next_kind);
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let refresh_hint = refresh_hint.clone();
        url_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                let value = entry.text().trim().to_string();
                tile.url = if value.is_empty() {
                    None
                } else {
                    Some(normalize_web_url(&value))
                };
            });
            refresh_hint();
        });
    }

    {
        let layout_state = layout_state.clone();
        let refresh_hint = refresh_hint.clone();
        auto_refresh.connect_value_changed(move |spinner| {
            update_tile_spec(&layout_state, index, |tile| {
                let seconds = spinner.value_as_int().max(0) as u32;
                tile.auto_refresh_seconds = (seconds > 0).then_some(seconds);
            });
            refresh_hint();
        });
    }

    row.upcast()
}

fn update_tile_spec<F>(layout_state: &Rc<RefCell<LayoutNode>>, index: usize, update: F)
where
    F: FnOnce(&mut TileSpec),
{
    if let Some(tile) = layout_state.borrow_mut().tile_spec_mut_at(index) {
        update(tile);
    }
}

fn tile_editor_hint(tile: &TileSpec, assets: &WorkspaceAssets) -> String {
    if tile.tile_kind == TileKind::WebView {
        let auto_refresh = tile
            .auto_refresh_seconds
            .map(|seconds| format!("every {}s", seconds))
            .unwrap_or_else(|| "off".into());
        return format!(
            "Tile kind: {}  •  URL: {}  •  Auto refresh: {}",
            tile.tile_kind.label(),
            tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL),
            auto_refresh,
        );
    }

    let role_label = tile
        .applied_role_id
        .as_deref()
        .and_then(|role_id| assets.role_templates.iter().find(|role| role.id == role_id))
        .map(|role| role.name.clone())
        .unwrap_or_else(|| "No role".into());
    let connection_label = match &tile.connection_target {
        crate::model::assets::TileConnectionTarget::Local => "Local".into(),
        crate::model::assets::TileConnectionTarget::Profile(profile_id) => assets
            .connection_profiles
            .iter()
            .find(|profile| profile.id == *profile_id)
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| format!("Missing profile: {profile_id}")),
    };
    format!(
        "Working directory: {}  •  Role: {}  •  Connection: {}",
        tile.working_directory.short_label(),
        role_label,
        connection_label
    )
}

fn accent_class_for_agent(agent_label: &str) -> String {
    let label = agent_label.trim().to_ascii_lowercase();

    if label.contains("claude") {
        "accent-amber".into()
    } else if label.contains("gemini") {
        "accent-violet".into()
    } else if label.contains("open") {
        "accent-rose".into()
    } else {
        "accent-cyan".into()
    }
}

fn prompt_for_workspace_directory(path_entry: &gtk::Entry) {
    let entry = path_entry.clone();
    let window = entry.root().and_then(|r| r.downcast::<gtk::Window>().ok());
    let dialog = gtk::FileChooserDialog::new(
        Some("Select Working Directory"),
        window.as_ref(),
        gtk::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Select", gtk::ResponseType::Accept),
        ],
    );
    if let Some(win) = window.as_ref() {
        dialog_chrome::sync_dialog_chrome_classes(win, &dialog, "launch-folder-picker-dialog");
    }

    let initial = PathBuf::from(entry.text().as_str());
    if initial.is_dir() {
        let _ = dialog.set_current_folder(Some(&gio::File::for_path(&initial)));
    }

    logging::info(format!(
        "opening workspace folder picker from {}",
        initial.display()
    ));

    dialog.connect_response(move |dialog: &gtk::FileChooserDialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(folder) = dialog.file()
            && let Some(path) = folder.path()
        {
            logging::info(format!(
                "workspace folder picker accepted {}",
                path.display()
            ));
            entry.set_text(&path.display().to_string());
        } else if response == gtk::ResponseType::Cancel {
            logging::info("workspace folder picker cancelled");
        } else {
            logging::info(format!(
                "workspace folder picker closed with response {:?}",
                response
            ));
        }

        dialog.close();
    });
    dialog.present();
}

fn validate_workspace_path(path_entry: &gtk::Entry) -> Result<PathBuf, String> {
    let text = path_entry.text();
    validate_workspace_path_text(text.as_str())
}

fn preset_workspace_root(path_entry: &gtk::Entry) -> Option<PathBuf> {
    let raw_path = path_entry.text().trim().to_string();
    match validate_workspace_path(path_entry) {
        Ok(workspace_root) => Some(workspace_root),
        Err(message) => {
            logging::error(format!(
                "Could not save workspace root with preset: {}",
                message
            ));
            (!raw_path.is_empty()).then(|| PathBuf::from(raw_path))
        }
    }
}

fn build_board_launch_request(
    name_entry: &gtk::Entry,
    path_entry: &gtk::Entry,
    id: Option<String>,
    theme: ThemeMode,
    density: ApplicationDensity,
) -> Result<BoardLaunchRequest, String> {
    let project_root = validate_workspace_path(path_entry)?;
    let name = name_entry.text().trim().to_string();
    let name = if name.is_empty() {
        project_root
            .file_name()
            .map(|name| format!("{} Kanban", name.to_string_lossy()))
            .unwrap_or_else(|| "Project Kanban".into())
    } else {
        name
    };

    Ok(BoardLaunchRequest {
        id,
        name,
        project_root,
        theme,
        density,
    })
}

fn validate_workspace_path_text(text: &str) -> Result<PathBuf, String> {
    if text.is_empty() {
        return Err("Workspace path is empty".into());
    }
    let path = PathBuf::from(text.trim());
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", path.display()));
    }
    resolve_workspace_root(&path).map_err(|error| {
        format!(
            "Could not resolve workspace path '{}': {}",
            path.display(),
            error
        )
    })
}

fn prompt_preset_name<F>(window: Option<&gtk::Window>, default_name: &str, on_submit: F)
where
    F: Fn(String) + 'static,
{
    let dialog = adw::MessageDialog::builder()
        .modal(true)
        .heading("Save Workspace Preset")
        .body("Enter the name to show on the Workspaces dashboard.")
        .build();

    if let Some(win) = window {
        dialog.set_transient_for(Some(win));
        dialog_chrome::sync_dialog_chrome_classes(win, &dialog, "launch-save-preset-dialog");
    }

    let entry = gtk::Entry::builder()
        .hexpand(true)
        .text(default_name)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");

    let on_submit = Rc::new(on_submit);
    dialog.connect_response(None, move |dialog, response| {
        if response == "save" {
            let name = entry.text().trim().to_string();
            if !name.is_empty() {
                on_submit(name);
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn unique_preset_id(name: &str) -> String {
    let slug = slugify(name);
    let slug = if slug.is_empty() {
        "preset".to_string()
    } else {
        slug
    };
    format!("{}-{}", slug, Uuid::new_v4().simple())
}
