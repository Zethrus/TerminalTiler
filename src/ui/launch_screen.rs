use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use uuid::Uuid;

use crate::logging;
use crate::model::layout::{
    LayoutNode, LayoutTemplate, TileSpec, builtin_templates, generate_layout,
};
use crate::model::preset::{ApplicationDensity, ThemeMode, WorkspacePreset, is_builtin_preset_id};
use crate::platform::{home_dir, resolve_workspace_root};
use crate::storage::preset_store::PresetStore;

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

pub fn build<F, G, H, I, C>(
    load_warning: Option<String>,
    presets: &[WorkspacePreset],
    default_theme: ThemeMode,
    default_density: ApplicationDensity,
    preset_store: PresetStore,
    on_theme_preview: H,
    on_density_preview: I,
    on_launch: F,
    on_cancel: C,
    on_presets_changed: G,
) -> gtk::Widget
where
    F: Fn(WorkspacePreset, PathBuf) + 'static,
    G: Fn() + 'static,
    H: Fn(ThemeMode) + 'static,
    I: Fn(ApplicationDensity) + 'static,
    C: Fn() + 'static,
{
    let current_dir = std::env::current_dir()
        .ok()
        .or_else(home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let templates = builtin_templates();
    let presets = Rc::new(presets.to_vec());
    let launch_callback = Rc::new(on_launch);
    let theme_preview_callback = Rc::new(on_theme_preview);
    let density_preview_callback = Rc::new(on_density_preview);
    let preset_store = Rc::new(preset_store);
    let on_presets_changed: Rc<dyn Fn()> = Rc::new(on_presets_changed);
    let selected: Rc<Cell<Selection>> = Rc::new(Cell::new(Selection::Template(0)));
    let chosen_theme: Rc<Cell<ThemeMode>> = Rc::new(Cell::new(default_theme));
    let chosen_density: Rc<Cell<ApplicationDensity>> = Rc::new(Cell::new(default_density));
    let active_layout = Rc::new(RefCell::new(generate_layout(
        templates
            .first()
            .map(|template| template.tile_count)
            .unwrap_or(1),
    )));
    let edit_preset_button_handle: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["launch-shell"])
        .build();

    let stage = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Start)
        .css_classes(["launch-stage"])
        .build();
    root.append(&stage);

    let header = build_header();
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

    let directory_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "directory-panel", "setup-panel"])
        .build();
    stage.append(&directory_panel);
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
        .css_classes(["workspace-path-row"])
        .build();
    path_row.append(&path_entry);

    let browse_button = gtk::Button::builder()
        .label("Browse")
        .css_classes(["pill-button", "secondary-button", "workspace-browse-button"])
        .valign(gtk::Align::Center)
        .build();
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
        .css_classes(["config-panel", "appearance-panel"])
        .build();
    stage.append(&options_panel);
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
        let theme_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let theme_label = gtk::Label::builder()
            .label("Theme")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["eyebrow"])
            .build();
        theme_row.append(&theme_label);

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
        theme_row.append(&theme_strip);
        options_panel.append(&theme_row);
    }

    let density_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["control-strip"])
        .build();

    {
        let density_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let density_label = gtk::Label::builder()
            .label("Density")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["eyebrow"])
            .build();
        density_row.append(&density_label);

        let density_hint = gtk::Label::builder()
            .label("Density changes panel spacing, titlebars, and terminal shell size.")
            .halign(gtk::Align::Start)
            .css_classes(["field-hint"])
            .build();

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
        density_row.append(&density_strip);
        options_panel.append(&density_row);
        options_panel.append(&density_hint);
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
    stage.append(&layout_panel);
    layout_panel.append(&build_section_header(
        "Step 3",
        "Choose a layout",
        "Start from a template or load a saved preset, then tune the tiles below.",
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

    let tile_editor = build_tile_editor_panel();
    tile_editor
        .tile_count
        .set_value(active_layout.borrow().tile_count() as f64);
    refresh_tile_editor(&tile_editor, &active_layout);

    let template_buttons: Rc<std::cell::RefCell<Vec<gtk::Widget>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let preset_buttons: Rc<std::cell::RefCell<Vec<gtk::Widget>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    {
        let tile_editor = tile_editor.clone();
        let active_layout = active_layout.clone();
        let summary = summary.clone();
        let tile_count = tile_editor.tile_count.clone();
        tile_count.connect_value_changed(move |spinner| {
            let requested = spinner.value_as_int().max(1) as usize;
            let next_layout = resize_layout(&active_layout.borrow(), requested);
            *active_layout.borrow_mut() = next_layout;
            refresh_tile_editor(&tile_editor, &active_layout);
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
            let edit_preset_button_handle = edit_preset_button_handle.clone();
            let density_preview_callback = density_preview_callback.clone();
            let default_theme = default_theme;
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
                refresh_tile_editor(&tile_editor, &active_layout);

                if let Some(button) = edit_preset_button_handle.borrow().as_ref() {
                    button.set_visible(false);
                }

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
            .css_classes(["config-panel", "presets-section"])
            .build();
        stage.append(&presets_section);
        presets_section.append(&build_section_header(
            "Saved presets",
            "Reuse a preset",
            "Load an existing setup or save the one you just configured.",
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
                    let chosen_theme = chosen_theme.clone();
                    let chosen_density = chosen_density.clone();
                    let theme_strip = theme_strip.clone();
                    let density_strip = density_strip.clone();
                    let edit_preset_button_handle = edit_preset_button_handle.clone();
                    let density_preview_callback = density_preview_callback.clone();

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
                        chosen_theme.set(p.theme);
                        chosen_density.set(p.density);
                        theme_preview_callback(p.theme);
                        sync_theme_strip_active(&theme_strip, p.theme);
                        density_preview_callback(p.density);
                        sync_density_strip_active(&density_strip, p.density);
                        *active_layout.borrow_mut() = p.layout.clone();
                        tile_editor.tile_count.set_value(p.tile_count() as f64);
                        refresh_tile_editor(&tile_editor, &active_layout);

                        if let Some(button) = edit_preset_button_handle.borrow().as_ref() {
                            button.set_visible(true);
                            button.set_label(if is_builtin_preset_id(&p.id) {
                                "Save Copy"
                            } else {
                                "Update Preset"
                            });
                        }

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

        let preset_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .halign(gtk::Align::Fill)
            .css_classes(["preset-actions"])
            .build();
        presets_section.append(&preset_actions);

        let save_preset_button = gtk::Button::builder()
            .label("Save as Preset")
            .css_classes(["pill-button", "secondary-button", "new-preset-button"])
            .valign(gtk::Align::Center)
            .build();
        {
            let selected = selected.clone();
            let templates_ref = builtin_templates();
            let presets = presets.clone();
            let preset_store = preset_store.clone();
            let on_presets_changed = on_presets_changed.clone();
            let session_name_entry = session_name_entry.clone();
            let chosen_theme = chosen_theme.clone();
            let chosen_density = chosen_density.clone();
            let active_layout = active_layout.clone();

            save_preset_button.connect_clicked(move |button| {
                let selected = selected.clone();
                let templates_ref_inner = builtin_templates();
                let presets = presets.clone();
                let preset_store = preset_store.clone();
                let on_presets_changed = on_presets_changed.clone();
                let session_name = session_name_entry.text().to_string();
                let theme = chosen_theme.get();
                let density = chosen_density.get();
                let layout = active_layout.borrow().clone();

                let default_name = if session_name.trim().is_empty() {
                    match selected.get() {
                        Selection::Template(idx) => templates_ref
                            .get(idx)
                            .map(|t| t.label.to_string())
                            .unwrap_or_else(|| "New Preset".into()),
                        Selection::Preset(idx) => presets
                            .get(idx)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "New Preset".into()),
                    }
                } else {
                    session_name.trim().to_string()
                };

                let window = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());

                prompt_preset_name(window.as_ref(), &default_name, move |name| {
                    let mut preset = build_launch_preset(
                        &selected,
                        &templates_ref_inner,
                        &presets,
                        &layout,
                        &session_name,
                        theme,
                        density,
                    );
                    preset.id = unique_preset_id(&name);
                    preset.name = name;

                    if let Err(err) = preset_store.upsert_preset(preset) {
                        logging::error(format!("Failed to save preset: {}", err));
                    } else {
                        on_presets_changed();
                    }
                });
            });
        }
        preset_actions.append(&save_preset_button);

        let edit_preset_button = gtk::Button::builder()
            .label("Update Preset")
            .css_classes(["pill-button", "secondary-button"])
            .visible(false)
            .build();
        {
            let selected = selected.clone();
            let templates_ref = builtin_templates();
            let presets = presets.clone();
            let preset_store = preset_store.clone();
            let on_presets_changed = on_presets_changed.clone();
            let session_name_entry = session_name_entry.clone();
            let chosen_theme = chosen_theme.clone();
            let chosen_density = chosen_density.clone();
            let active_layout = active_layout.clone();

            edit_preset_button.connect_clicked(move |button| {
                let Selection::Preset(index) = selected.get() else {
                    return;
                };

                let Some(existing) = presets.get(index) else {
                    return;
                };

                let layout = active_layout.borrow().clone();
                let session_name = session_name_entry.text().to_string();
                let theme = chosen_theme.get();
                let density = chosen_density.get();

                if is_builtin_preset_id(&existing.id) {
                    let default_name = if session_name.trim().is_empty() {
                        format!("{} Copy", existing.name)
                    } else {
                        session_name.trim().to_string()
                    };

                    let window = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                    let selected = selected.clone();
                    let templates_ref_inner = builtin_templates();
                    let presets = presets.clone();
                    let preset_store = preset_store.clone();
                    let on_presets_changed = on_presets_changed.clone();

                    prompt_preset_name(window.as_ref(), &default_name, move |name| {
                        let mut preset = build_launch_preset(
                            &selected,
                            &templates_ref_inner,
                            &presets,
                            &layout,
                            &session_name,
                            theme,
                            density,
                        );
                        preset.id = unique_preset_id(&name);
                        preset.name = name;

                        if let Err(err) = preset_store.upsert_preset(preset) {
                            logging::error(format!("Failed to save preset copy: {}", err));
                        } else {
                            on_presets_changed();
                        }
                    });
                } else {
                    let mut preset = build_launch_preset(
                        &selected,
                        &templates_ref,
                        &presets,
                        &layout,
                        &session_name,
                        theme,
                        density,
                    );
                    preset.id = existing.id.clone();

                    if let Err(err) = preset_store.upsert_preset(preset) {
                        logging::error(format!("Failed to update preset: {}", err));
                    } else {
                        on_presets_changed();
                    }
                }
            });
        }
        *edit_preset_button_handle.borrow_mut() = Some(edit_preset_button.clone());
        preset_actions.append(&edit_preset_button);
    }

    stage.append(&tile_editor.root);

    let action_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .css_classes(["action-bar-bottom"])
        .build();
    stage.append(&action_bar);

    let cancel_button = gtk::Button::with_label("Cancel");
    cancel_button.add_css_class("pill-button");
    cancel_button.add_css_class("ghost-link-button");
    cancel_button.connect_clicked(move |_| on_cancel());
    action_bar.append(&cancel_button);

    let spacer = gtk::Box::builder().hexpand(true).build();
    action_bar.append(&spacer);

    let configure_button = gtk::Button::with_label("Launch Workspace");
    configure_button.add_css_class("pill-button");
    configure_button.add_css_class("primary-cta-button");
    action_bar.append(&configure_button);

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

        configure_button.connect_clicked(move |_| match validate_workspace_path(&path_entry) {
            Ok(workspace_root) => {
                let session_name = session_name_entry.text().to_string();
                let preset = build_launch_preset(
                    &selected,
                    &templates_ref,
                    &presets,
                    &active_layout.borrow().clone(),
                    &session_name,
                    chosen_theme.get(),
                    chosen_density.get(),
                );
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
        });
    }

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

fn build_header() -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .css_classes(["launch-header"])
        .build();

    let title = gtk::Label::builder()
        .label("Configure Layout")
        .halign(gtk::Align::Center)
        .wrap(true)
        .css_classes(["hero-title", "config-title"])
        .build();
    let body = gtk::Label::builder()
        .label("Pick a starting layout, then tune the tiles you want to open.")
        .halign(gtk::Align::Center)
        .wrap(true)
        .css_classes(["hero-body", "config-subtitle"])
        .build();

    card.append(&title);
    card.append(&body);
    card.upcast()
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
        .css_classes(["preset-card", "template-button"])
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
        .css_classes(["selection-summary", "config-panel"])
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
        .css_classes(["preset-card-compact"])
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

    if !is_builtin_preset_id(&preset.id) {
        let delete_button = gtk::Button::from_icon_name("window-close-symbolic");
        delete_button.add_css_class("flat");
        delete_button.add_css_class("preset-delete-button");
        delete_button.add_css_class("destructive-button");
        delete_button.set_valign(gtk::Align::Start);
        delete_button.set_tooltip_text(Some("Delete preset"));

        let preset_id = preset.id.clone();
        let preset_name = preset.name.clone();
        let preset_store = preset_store.clone();
        let on_presets_changed = on_presets_changed.clone();

        delete_button.connect_clicked(move |button| {
            let window = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());

            let preset_id = preset_id.clone();
            let preset_store = preset_store.clone();
            let on_presets_changed = on_presets_changed.clone();

            let dialog = adw::MessageDialog::builder()
                .modal(true)
                .heading("Delete Preset?")
                .body(format!("\"{}\" will be permanently removed.", preset_name))
                .build();

            if let Some(ref win) = window {
                dialog.set_transient_for(Some(win));
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
        });

        top_row.append(&delete_button);
    }

    shell.upcast()
}

fn build_tile_editor_panel() -> TileEditorPanel {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["config-panel", "tile-editor-panel"])
        .build();

    root.append(&build_section_header(
        "Step 4",
        "Tile setup",
        "Set how many terminals to open, then name each tile and add an optional startup command.",
    ));

    let count_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["tile-count-row"])
        .build();

    let count_label = gtk::Label::builder()
        .label("Terminal tiles")
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

fn refresh_tile_editor(panel: &TileEditorPanel, layout_state: &Rc<RefCell<LayoutNode>>) {
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
        panel
            .rows
            .append(&build_tile_editor_row(index, tile, layout_state));
    }
}

fn build_launch_preset(
    selected: &Rc<Cell<Selection>>,
    templates: &[LayoutTemplate],
    presets: &[WorkspacePreset],
    layout: &LayoutNode,
    session_name: &str,
    theme: ThemeMode,
    density: ApplicationDensity,
) -> WorkspacePreset {
    let custom_name = if session_name.is_empty() {
        None
    } else {
        Some(session_name.to_string())
    };

    match selected.get() {
        Selection::Template(idx) => {
            let template = &templates[idx];
            WorkspacePreset {
                id: format!("session-{}", template.tile_count),
                name: custom_name.unwrap_or_else(|| template.label.to_string()),
                description: String::new(),
                tags: Vec::new(),
                root_label: "Workspace root".into(),
                theme,
                density,
                layout: layout.clone(),
            }
        }
        Selection::Preset(idx) => {
            let mut preset = presets[idx].clone();
            if let Some(name) = custom_name {
                preset.name = name;
            }
            preset.theme = theme;
            preset.density = density;
            preset.layout = layout.clone();
            preset
        }
    }
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
    layout_state: &Rc<RefCell<LayoutNode>>,
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
    header.append(&directory);
    row.append(&header);

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

    let directory_hint = gtk::Label::builder()
        .label(format!(
            "Working directory: {}",
            tile.working_directory.short_label()
        ))
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    row.append(&directory_hint);

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
        agent_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                tile.agent_label = entry.text().to_string();
                tile.accent_class = accent_class_for_agent(&tile.agent_label);
            });
        });
    }

    {
        let layout_state = layout_state.clone();
        command_entry.connect_changed(move |entry| {
            update_tile_spec(&layout_state, index, |tile| {
                let value = entry.text().trim().to_string();
                tile.startup_command = if value.is_empty() { None } else { Some(value) };
            });
        });
    }

    row.upcast()
}

fn update_tile_spec<F>(layout_state: &Rc<RefCell<LayoutNode>>, index: usize, update: F)
where
    F: FnOnce(&mut TileSpec),
{
    let current_layout = layout_state.borrow().clone();
    let mut tile_specs = current_layout.tile_specs();

    if let Some(tile) = tile_specs.get_mut(index) {
        update(tile);
        *layout_state.borrow_mut() = current_layout.with_tile_specs(&tile_specs);
    }
}

fn resize_layout(current_layout: &LayoutNode, tile_count: usize) -> LayoutNode {
    let next_layout = generate_layout(tile_count);
    let current_tiles = current_layout.tile_specs();
    let mut next_tiles = next_layout.tile_specs();

    for (index, tile) in next_tiles.iter_mut().enumerate() {
        if let Some(existing) = current_tiles.get(index) {
            tile.id = existing.id.clone();
            tile.title = existing.title.clone();
            tile.agent_label = existing.agent_label.clone();
            tile.accent_class = existing.accent_class.clone();
            tile.working_directory = existing.working_directory.clone();
            tile.startup_command = existing.startup_command.clone();
        }
    }

    next_layout.with_tile_specs(&next_tiles)
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
    let dialog = gtk::FileChooserNative::new(
        Some("Select Working Directory"),
        window.as_ref(),
        gtk::FileChooserAction::SelectFolder,
        Some("Select"),
        Some("Cancel"),
    );

    let initial = PathBuf::from(entry.text().as_str());
    if initial.is_dir() {
        let _ = dialog.set_file(&gio::File::for_path(&initial));
    }

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(folder) = dialog.file()
            && let Some(path) = folder.path()
        {
            entry.set_text(&path.display().to_string());
        }

        dialog.destroy();
    });
    dialog.show();
}

fn validate_workspace_path(path_entry: &gtk::Entry) -> Result<PathBuf, String> {
    let text = path_entry.text();
    validate_workspace_path_text(text.as_str())
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
        .heading("Save as Preset")
        .body("Enter a name for the new preset.")
        .build();

    if let Some(win) = window {
        dialog.set_transient_for(Some(win));
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
