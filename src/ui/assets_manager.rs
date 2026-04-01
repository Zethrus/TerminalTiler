use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;

use crate::model::workspace_config::ConfigScope;
use crate::storage::asset_store::AssetStore;

#[allow(deprecated)]
pub fn present(
    window: &adw::ApplicationWindow,
    asset_store: Rc<AssetStore>,
    workspace_root: Option<PathBuf>,
    on_saved: Rc<dyn Fn()>,
) {
    let dialog = gtk::Dialog::builder()
        .modal(true)
        .transient_for(window)
        .title("Assets Manager")
        .default_width(820)
        .default_height(640)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let scope = Rc::new(std::cell::Cell::new(ConfigScope::Global));
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    content.append(&header);

    let global_button = gtk::Button::builder()
        .label(ConfigScope::Global.label())
        .css_classes(["pill-button"])
        .build();
    let workspace_button = gtk::Button::builder()
        .label(ConfigScope::Workspace.label())
        .css_classes(["pill-button"])
        .sensitive(workspace_root.is_some())
        .build();
    header.append(&global_button);
    header.append(&workspace_button);

    let info = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    content.append(&info);

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let text_view = gtk::TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .vexpand(true)
        .hexpand(true)
        .build();
    scroller.set_child(Some(&text_view));
    content.append(&scroller);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let reload_button = gtk::Button::builder()
        .label("Reload")
        .css_classes(["flat"])
        .build();
    let save_button = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();
    actions.append(&reload_button);
    actions.append(&save_button);
    content.append(&actions);

    let load_scope: Rc<dyn Fn()> = {
        let asset_store = asset_store.clone();
        let text_view = text_view.clone();
        let info = info.clone();
        let scope = scope.clone();
        let workspace_root = workspace_root.clone();
        let global_button = global_button.clone();
        let workspace_button = workspace_button.clone();
        Rc::new(move || {
            let (assets, info_text) = match scope.get() {
                ConfigScope::Global => (
                    asset_store.load_assets(),
                    String::from(
                        "Editing global assets from ~/.config/TerminalTiler/workspace-assets.toml.",
                    ),
                ),
                ConfigScope::Workspace => {
                    if let Some(workspace_root) = workspace_root.as_ref() {
                        (
                            asset_store.load_workspace_config(workspace_root).assets,
                            format!(
                                "Editing workspace-local overrides from {}/.terminaltiler/workspace.toml. IDs override global items when they match.",
                                workspace_root.display()
                            ),
                        )
                    } else {
                        (
                            crate::model::assets::WorkspaceAssets::default(),
                            String::from(
                                "Workspace scope is unavailable without an active workspace root.",
                            ),
                        )
                    }
                }
            };
            let serialized = toml::to_string_pretty(&assets)
                .unwrap_or_else(|_| String::from("# serialization failed\n"));
            text_view.buffer().set_text(&serialized);
            info.set_text(&info_text);
            sync_scope_buttons(&global_button, &workspace_button, scope.get());
        })
    };
    load_scope();

    {
        let scope = scope.clone();
        let load_scope = load_scope.clone();
        global_button.connect_clicked(move |_| {
            scope.set(ConfigScope::Global);
            load_scope();
        });
    }
    {
        let scope = scope.clone();
        let load_scope = load_scope.clone();
        workspace_button.connect_clicked(move |_| {
            scope.set(ConfigScope::Workspace);
            load_scope();
        });
    }
    {
        let load_scope = load_scope.clone();
        reload_button.connect_clicked(move |_| load_scope());
    }
    {
        let asset_store = asset_store.clone();
        let text_view = text_view.clone();
        let scope = scope.clone();
        let workspace_root = workspace_root.clone();
        let info = info.clone();
        let on_saved = on_saved.clone();
        save_button.connect_clicked(move |_| {
            let buffer = text_view.buffer();
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            let raw = buffer.text(&start, &end, true).to_string();
            match toml::from_str::<crate::model::assets::WorkspaceAssets>(&raw) {
                Ok(assets) => match asset_store.save_assets_for_scope(
                    &assets,
                    scope.get(),
                    workspace_root.as_deref(),
                ) {
                    Ok(_) => {
                        info.set_text("Assets saved successfully.");
                        on_saved();
                    }
                    Err(error) => {
                        info.set_text(&format!("Failed to save assets: {error}"));
                    }
                },
                Err(error) => {
                    info.set_text(&format!("Failed to parse assets TOML: {error}"));
                }
            }
        });
    }

    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

fn sync_scope_buttons(
    global_button: &gtk::Button,
    workspace_button: &gtk::Button,
    scope: ConfigScope,
) {
    global_button.remove_css_class("suggested-action");
    workspace_button.remove_css_class("suggested-action");
    match scope {
        ConfigScope::Global => global_button.add_css_class("suggested-action"),
        ConfigScope::Workspace => workspace_button.add_css_class("suggested-action"),
    }
}
