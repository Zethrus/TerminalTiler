use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;

use crate::model::assets::{
    AgentRoleTemplate, ConnectionKind, ConnectionProfile, InventoryGroup, InventoryHost,
    OutputHelperRule, OutputSeverity, Runbook, RunbookConfirmPolicy, RunbookStep, RunbookTarget,
    RunbookVariable, WorkspaceAssets,
};
use crate::model::layout::ReconnectPolicy;
use crate::model::workspace_config::ConfigScope;
use crate::services::assets_editor::{
    AssetItemSource, AssetSection, AssetValidationIssue, connection_source,
    effective_assets_for_scope, group_source, host_source, role_source, runbook_source,
    validate_assets,
};
use crate::storage::asset_store::AssetStore;

#[derive(Clone)]
struct AssetsManagerState {
    scope: ConfigScope,
    workspace_root: Option<PathBuf>,
    global_assets: WorkspaceAssets,
    current_assets: WorkspaceAssets,
    loaded_assets: WorkspaceAssets,
    raw_toml: String,
    raw_error: Option<String>,
    info_text: String,
    warning_text: Option<String>,
}

struct AssetsPages {
    overview: gtk::Box,
    connections: gtk::Box,
    hosts: gtk::Box,
    groups: gtk::Box,
    roles: gtk::Box,
    runbooks: gtk::Box,
    raw_text_view: gtk::TextView,
}

type RefreshHandle = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

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
        .default_width(1120)
        .default_height(760)
        .resizable(true)
        .build();
    dialog.add_css_class("assets-manager-window");
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_response(gtk::ResponseType::Close);

    let content = dialog.content_area();
    content.set_spacing(12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let scope = if workspace_root.is_some() {
        ConfigScope::Workspace
    } else {
        ConfigScope::Global
    };
    let initial_state = load_scope_state(&asset_store, scope, workspace_root.clone());
    let state = Rc::new(RefCell::new(initial_state));
    let refresh_token = Rc::new(Cell::new(0u64));

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["assets-manager-hero"])
        .build();
    header.append(
        &gtk::Label::builder()
            .label("Edit connections, hosts, groups, roles, and runbooks without touching raw structure.")
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["card-title"])
            .build(),
    );

    let scope_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let global_button = gtk::Button::builder()
        .label("Global defaults")
        .css_classes(["pill-button"])
        .build();
    let workspace_button = gtk::Button::builder()
        .label("Workspace overrides")
        .css_classes(["pill-button"])
        .sensitive(workspace_root.is_some())
        .build();
    scope_row.append(&global_button);
    scope_row.append(&workspace_button);
    header.append(&scope_row);

    let info_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    let warning_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint", "assets-warning"])
        .visible(false)
        .build();
    header.append(&info_label);
    header.append(&warning_label);
    content.append(&header);

    let issue_banner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["assets-issue-banner"])
        .visible(false)
        .build();
    let issue_title = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes(["card-title"])
        .build();
    let issue_body = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build();
    issue_banner.append(&issue_title);
    issue_banner.append(&issue_body);
    content.append(&issue_banner);

    let stack = gtk::Stack::builder()
        .hexpand(true)
        .vexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.add_css_class("assets-sidebar");

    let overview_page = make_page_shell();
    let connections_page = make_page_shell();
    let hosts_page = make_page_shell();
    let groups_page = make_page_shell();
    let roles_page = make_page_shell();
    let runbooks_page = make_page_shell();
    let raw_page = make_page_shell();

    stack.add_titled(
        &overview_page.0,
        Some("overview"),
        AssetSection::Overview.title(),
    );
    stack.add_titled(
        &connections_page.0,
        Some("connections"),
        AssetSection::Connections.title(),
    );
    stack.add_titled(&hosts_page.0, Some("hosts"), AssetSection::Hosts.title());
    stack.add_titled(&groups_page.0, Some("groups"), AssetSection::Groups.title());
    stack.add_titled(&roles_page.0, Some("roles"), AssetSection::Roles.title());
    stack.add_titled(
        &runbooks_page.0,
        Some("runbooks"),
        AssetSection::Runbooks.title(),
    );

    let raw_text_view = gtk::TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .hexpand(true)
        .vexpand(true)
        .build();
    raw_page.1.append(
        &gtk::Label::builder()
            .label(AssetSection::RawToml.description())
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );
    let raw_scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    raw_scroller.set_child(Some(&raw_text_view));
    raw_page.1.append(&raw_scroller);
    stack.add_titled(&raw_page.0, Some("raw"), AssetSection::RawToml.title());

    let shell = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .wide_handle(true)
        .position(220)
        .build();
    shell.set_start_child(Some(&sidebar));
    shell.set_end_child(Some(&stack));
    content.append(&shell);

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

    let pages = Rc::new(AssetsPages {
        overview: overview_page.1,
        connections: connections_page.1,
        hosts: hosts_page.1,
        groups: groups_page.1,
        roles: roles_page.1,
        runbooks: runbooks_page.1,
        raw_text_view,
    });

    let refresh_status: Rc<dyn Fn()> = {
        let state = state.clone();
        let global_button = global_button.clone();
        let workspace_button = workspace_button.clone();
        let info_label = info_label.clone();
        let warning_label = warning_label.clone();
        let issue_banner = issue_banner.clone();
        let issue_title = issue_title.clone();
        let issue_body = issue_body.clone();
        let save_button = save_button.clone();
        let raw_text_view = pages.raw_text_view.clone();
        Rc::new(move || {
            let snapshot = state.borrow().clone();
            sync_scope_buttons(&global_button, &workspace_button, snapshot.scope);
            info_label.set_text(&snapshot.info_text);
            if let Some(warning) = snapshot.warning_text.as_deref() {
                warning_label.set_visible(true);
                warning_label.set_text(warning);
            } else {
                warning_label.set_visible(false);
                warning_label.set_text("");
            }

            let issues = validate_assets(
                snapshot.scope,
                &snapshot.current_assets,
                &snapshot.global_assets,
            );
            if let Some(raw_error) = snapshot.raw_error.as_deref() {
                issue_banner.set_visible(true);
                issue_title.set_text("Raw TOML has errors");
                issue_body.set_text(raw_error);
            } else if !issues.is_empty() {
                issue_banner.set_visible(true);
                issue_title.set_text("Fix validation issues before saving");
                issue_body.set_text(&format_issue_summary(&issues));
            } else {
                issue_banner.set_visible(false);
                issue_title.set_text("");
                issue_body.set_text("");
            }

            save_button.set_sensitive(
                snapshot.raw_error.is_none() && issues.is_empty() && is_dirty(&snapshot),
            );
            sync_raw_buffer(&raw_text_view, &snapshot.raw_toml);
        })
    };

    let refresh_pages_handle: RefreshHandle = Rc::new(RefCell::new(None));
    let refresh_pages: Rc<dyn Fn()> = {
        let state = state.clone();
        let pages = pages.clone();
        let refresh_status = refresh_status.clone();
        let refresh_token = refresh_token.clone();
        let dialog = dialog.clone();
        let refresh_pages_handle = refresh_pages_handle.clone();
        Rc::new(move || {
            refresh_token.set(refresh_token.get().wrapping_add(1));
            let token = refresh_token.get();

            refresh_status();

            clear_box(&pages.overview);
            clear_box(&pages.connections);
            clear_box(&pages.hosts);
            clear_box(&pages.groups);
            clear_box(&pages.roles);
            clear_box(&pages.runbooks);

            {
                let snapshot = state.borrow().clone();
                render_overview_page(&pages.overview, &snapshot);
                render_connections_page(
                    &pages.connections,
                    &state,
                    token,
                    &refresh_token,
                    &refresh_status,
                    &refresh_pages_handle,
                    &dialog,
                );
                render_hosts_page(
                    &pages.hosts,
                    &state,
                    token,
                    &refresh_token,
                    &refresh_status,
                    &refresh_pages_handle,
                    &dialog,
                );
                render_groups_page(
                    &pages.groups,
                    &state,
                    token,
                    &refresh_token,
                    &refresh_status,
                    &refresh_pages_handle,
                    &dialog,
                );
                render_roles_page(
                    &pages.roles,
                    &state,
                    token,
                    &refresh_token,
                    &refresh_status,
                    &refresh_pages_handle,
                    &dialog,
                );
                render_runbooks_page(
                    &pages.runbooks,
                    &state,
                    token,
                    &refresh_token,
                    &refresh_status,
                    &refresh_pages_handle,
                    &dialog,
                );
            }
            sync_raw_buffer(&pages.raw_text_view, &state.borrow().raw_toml);
        })
    };
    *refresh_pages_handle.borrow_mut() = Some(refresh_pages.clone());

    refresh_pages();

    {
        let refresh_pages = refresh_pages.clone();
        stack.connect_notify_local(Some("visible-child-name"), move |_, _| {
            refresh_pages();
        });
    }

    {
        let state = state.clone();
        let refresh_status = refresh_status.clone();
        pages.raw_text_view.buffer().connect_changed(move |buffer| {
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            let raw = buffer.text(&start, &end, true).to_string();
            let mut snapshot = state.borrow_mut();
            snapshot.raw_toml = raw.clone();
            match toml::from_str::<WorkspaceAssets>(&raw) {
                Ok(assets) => {
                    snapshot.current_assets = assets;
                    snapshot.raw_error = None;
                }
                Err(error) => {
                    snapshot.raw_error = Some(format!("TOML parse error: {error}"));
                }
            }
            refresh_status();
        });
    }

    {
        let state = state.clone();
        let asset_store = asset_store.clone();
        let refresh_pages = refresh_pages.clone();
        let dialog = dialog.clone();
        global_button.connect_clicked(move |_| {
            if state.borrow().scope == ConfigScope::Global {
                return;
            }
            let state_for_prompt = state.clone();
            let asset_store = asset_store.clone();
            let refresh_pages = refresh_pages.clone();
            let workspace_root = state.borrow().workspace_root.clone();
            maybe_discard_unsaved(&dialog, &state.borrow(), move || {
                *state_for_prompt.borrow_mut() =
                    load_scope_state(&asset_store, ConfigScope::Global, workspace_root.clone());
                refresh_pages();
            });
        });
    }
    {
        let state = state.clone();
        let asset_store = asset_store.clone();
        let refresh_pages = refresh_pages.clone();
        let dialog = dialog.clone();
        workspace_button.connect_clicked(move |_| {
            if state.borrow().scope == ConfigScope::Workspace
                || state.borrow().workspace_root.is_none()
            {
                return;
            }
            let state_for_prompt = state.clone();
            let asset_store = asset_store.clone();
            let refresh_pages = refresh_pages.clone();
            let workspace_root = state.borrow().workspace_root.clone();
            maybe_discard_unsaved(&dialog, &state.borrow(), move || {
                *state_for_prompt.borrow_mut() =
                    load_scope_state(&asset_store, ConfigScope::Workspace, workspace_root.clone());
                refresh_pages();
            });
        });
    }
    {
        let state = state.clone();
        let asset_store = asset_store.clone();
        let refresh_pages = refresh_pages.clone();
        let dialog = dialog.clone();
        reload_button.connect_clicked(move |_| {
            let state_for_prompt = state.clone();
            let asset_store = asset_store.clone();
            let refresh_pages = refresh_pages.clone();
            let scope = state.borrow().scope;
            let workspace_root = state.borrow().workspace_root.clone();
            maybe_discard_unsaved(&dialog, &state.borrow(), move || {
                *state_for_prompt.borrow_mut() =
                    load_scope_state(&asset_store, scope, workspace_root.clone());
                refresh_pages();
            });
        });
    }
    {
        let state = state.clone();
        let asset_store = asset_store.clone();
        let refresh_pages = refresh_pages.clone();
        let on_saved = on_saved.clone();
        save_button.connect_clicked(move |_| {
            let mut snapshot = state.borrow_mut();
            if snapshot.raw_error.is_some() {
                return;
            }
            let issues = validate_assets(snapshot.scope, &snapshot.current_assets, &snapshot.global_assets);
            if !issues.is_empty() {
                return;
            }
            match asset_store.save_assets_for_scope(
                &snapshot.current_assets,
                snapshot.scope,
                snapshot.workspace_root.as_deref(),
            ) {
                Ok(()) => {
                    snapshot.global_assets = asset_store.load_assets();
                    snapshot.loaded_assets = snapshot.current_assets.clone();
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.warning_text = None;
                    snapshot.info_text = match snapshot.scope {
                        ConfigScope::Global => String::from(
                            "Editing global defaults from ~/.config/TerminalTiler/workspace-assets.toml. These assets are shared by every workspace.",
                        ),
                        ConfigScope::Workspace => format!(
                            "Editing workspace overrides from {}/.terminaltiler/workspace.toml. Matching IDs shadow the global definitions in this workspace only.",
                            snapshot
                                .workspace_root
                                .as_ref()
                                .map(|root| root.display().to_string())
                                .unwrap_or_else(|| ".".into())
                        ),
                    };
                    drop(snapshot);
                    on_saved();
                    refresh_pages();
                }
                Err(error) => {
                    snapshot.warning_text = Some(format!("Failed to save assets: {error}"));
                    drop(snapshot);
                    refresh_pages();
                }
            }
        });
    }

    {
        let state = state.clone();
        let dialog_for_prompt = dialog.clone();
        dialog.connect_response(move |dialog, response| {
            if response != gtk::ResponseType::Close {
                return;
            }
            maybe_discard_unsaved(&dialog_for_prompt, &state.borrow(), {
                let dialog = dialog.clone();
                move || dialog.close()
            });
        });
    }

    dialog.present();
}

fn load_scope_state(
    asset_store: &AssetStore,
    scope: ConfigScope,
    workspace_root: Option<PathBuf>,
) -> AssetsManagerState {
    let global_load = asset_store.load_assets_with_status();
    let global_assets = global_load.assets.clone();
    let (current_assets, info_text, warning_text) = match scope {
        ConfigScope::Global => (
            global_assets.clone(),
            String::from(
                "Editing global defaults from ~/.config/TerminalTiler/workspace-assets.toml. These assets are shared by every workspace.",
            ),
            global_load.warning,
        ),
        ConfigScope::Workspace => {
            if let Some(root) = workspace_root.as_ref() {
                let effective = asset_store.load_assets_for_workspace_root(root);
                (
                    asset_store.load_workspace_config(root).assets,
                    format!(
                        "Editing workspace overrides from {}/.terminaltiler/workspace.toml. Matching IDs shadow the global definitions in this workspace only.",
                        root.display()
                    ),
                    effective.warning,
                )
            } else {
                (
                    WorkspaceAssets::default(),
                    String::from(
                        "Workspace overrides are unavailable until a workspace root is selected.",
                    ),
                    Some(String::from(
                        "Workspace scope is disabled because no active workspace root was detected.",
                    )),
                )
            }
        }
    };
    let raw_toml = serialize_assets(&current_assets);
    AssetsManagerState {
        scope,
        workspace_root,
        global_assets,
        current_assets: current_assets.clone(),
        loaded_assets: current_assets,
        raw_toml,
        raw_error: None,
        info_text,
        warning_text,
    }
}

fn make_page_shell() -> (gtk::ScrolledWindow, gtk::Box) {
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    scroller.set_child(Some(&content));
    (scroller, content)
}

fn render_overview_page(container: &gtk::Box, state: &AssetsManagerState) {
    let effective =
        effective_assets_for_scope(state.scope, &state.current_assets, &state.global_assets);
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["config-panel", "asset-card"])
        .build();
    summary.append(
        &gtk::Label::builder()
            .label(AssetSection::Overview.description())
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );
    for line in [
        format!("Connections: {}", effective.connection_profiles.len()),
        format!("Hosts: {}", effective.inventory_hosts.len()),
        format!("Groups: {}", effective.inventory_groups.len()),
        format!("Roles: {}", effective.role_templates.len()),
        format!("Runbooks: {}", effective.runbooks.len()),
    ] {
        summary.append(
            &gtk::Label::builder()
                .label(line)
                .halign(gtk::Align::Start)
                .css_classes(["card-meta"])
                .build(),
        );
    }
    container.append(&summary);
}

fn render_connections_page(
    container: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
) {
    render_section_header(
        container,
        AssetSection::Connections,
        state,
        refresh_pages,
        dialog,
        move |snapshot| {
            snapshot
                .current_assets
                .connection_profiles
                .push(ConnectionProfile {
                    id: String::new(),
                    name: String::new(),
                    kind: ConnectionKind::Local,
                    inventory_host_id: None,
                    tags: Vec::new(),
                    remote_working_directory: None,
                    shell_program: None,
                    startup_prefix: None,
                });
        },
    );

    let snapshot = state.borrow().clone();
    let effective = effective_assets_for_scope(
        snapshot.scope,
        &snapshot.current_assets,
        &snapshot.global_assets,
    );
    let global_host_ids = effective
        .inventory_hosts
        .iter()
        .map(|item| (item.id.clone(), item.name.clone()))
        .collect::<Vec<_>>();

    let current_ids = snapshot
        .current_assets
        .connection_profiles
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    for (index, profile) in snapshot
        .current_assets
        .connection_profiles
        .iter()
        .cloned()
        .enumerate()
    {
        let badge = connection_source(
            snapshot.scope,
            &profile.id,
            &snapshot.current_assets,
            &snapshot.global_assets,
        );
        let remove_label = if badge == AssetItemSource::WorkspaceOverride {
            "Remove override"
        } else {
            "Remove"
        };
        let card = asset_card_shell(
            &profile.name,
            &profile.id,
            badge,
            token,
            refresh_token,
            dialog,
            state,
            move |snapshot| {
                snapshot.current_assets.connection_profiles.remove(index);
            },
            remove_label,
            Some(Rc::new({
                let state = state.clone();
                move || {
                    let mut snapshot = state.borrow_mut();
                    let mut cloned = snapshot.current_assets.connection_profiles[index].clone();
                    cloned.id = String::new();
                    cloned.name = format!("{} copy", cloned.name);
                    snapshot.current_assets.connection_profiles.push(cloned);
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.raw_error = None;
                }
            })),
            refresh_pages,
        );
        append_entry_field(
            &card,
            "ID",
            &profile.id,
            "ssh-prod",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].id = value;
            },
        );
        append_entry_field(
            &card,
            "Name",
            &profile.name,
            "Production SSH",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].name = value;
            },
        );
        append_combo_field(
            &card,
            "Connection kind",
            &[("local", "Local"), ("ssh", "SSH"), ("wsl", "WSL")],
            match profile.kind {
                ConnectionKind::Local => "local",
                ConnectionKind::Ssh => "ssh",
                ConnectionKind::Wsl => "wsl",
            },
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].kind = match value.as_str() {
                    "ssh" => ConnectionKind::Ssh,
                    "wsl" => ConnectionKind::Wsl,
                    _ => ConnectionKind::Local,
                };
            },
        );
        let host_options = {
            let mut options = vec![("__none__".to_string(), "No host".to_string())];
            options.extend(global_host_ids.iter().cloned());
            options
        };
        append_dynamic_combo_field(
            &card,
            "Linked host",
            &host_options,
            profile
                .inventory_host_id
                .clone()
                .unwrap_or_else(|| "__none__".into()),
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].inventory_host_id =
                    if value == "__none__" {
                        None
                    } else {
                        Some(value)
                    };
            },
        );
        append_entry_field(
            &card,
            "Remote working directory",
            profile.remote_working_directory.as_deref().unwrap_or(""),
            "/srv/app",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].remote_working_directory =
                    none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Shell program",
            profile.shell_program.as_deref().unwrap_or(""),
            "bash",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].shell_program =
                    none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Startup prefix",
            profile.startup_prefix.as_deref().unwrap_or(""),
            "source ~/.profile",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].startup_prefix =
                    none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Tags",
            &profile.tags.join(", "),
            "prod, ssh",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.connection_profiles[index].tags = parse_csv(&value);
            },
        );
        container.append(&card);
    }

    if snapshot.scope == ConfigScope::Workspace {
        for profile in effective
            .connection_profiles
            .iter()
            .filter(|item| !current_ids.contains(&item.id))
            .cloned()
        {
            let readonly = readonly_card(
                profile.name.as_str(),
                profile.id.as_str(),
                AssetItemSource::Global,
                "Inherited from global defaults. Override it here to customize this workspace.",
            );
            attach_readonly_connection_details(&readonly, &profile);
            append_override_button(
                readonly.upcast_ref(),
                state,
                refresh_pages,
                dialog,
                move |snapshot| {
                    snapshot
                        .current_assets
                        .connection_profiles
                        .push(profile.clone());
                },
            );
            container.append(&readonly);
        }
    }
}

fn render_hosts_page(
    container: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
) {
    render_section_header(
        container,
        AssetSection::Hosts,
        state,
        refresh_pages,
        dialog,
        move |snapshot| {
            snapshot.current_assets.inventory_hosts.push(InventoryHost {
                id: String::new(),
                name: String::new(),
                host: String::new(),
                group_ids: Vec::new(),
                tags: Vec::new(),
                provider: String::new(),
                main_ip: String::new(),
                user: String::new(),
                port: 22,
                price_per_month_usd_cents: 0,
                password_secret_ref: None,
                ssh_key_path: None,
            });
        },
    );

    let snapshot = state.borrow().clone();
    let effective = effective_assets_for_scope(
        snapshot.scope,
        &snapshot.current_assets,
        &snapshot.global_assets,
    );
    let current_ids = snapshot
        .current_assets
        .inventory_hosts
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let groups = effective.inventory_groups.clone();

    for (index, host) in snapshot
        .current_assets
        .inventory_hosts
        .iter()
        .cloned()
        .enumerate()
    {
        let badge = host_source(
            snapshot.scope,
            &host.id,
            &snapshot.current_assets,
            &snapshot.global_assets,
        );
        let remove_label = if badge == AssetItemSource::WorkspaceOverride {
            "Remove override"
        } else {
            "Remove"
        };
        let card = asset_card_shell(
            &host.name,
            &host.id,
            badge,
            token,
            refresh_token,
            dialog,
            state,
            move |snapshot| {
                snapshot.current_assets.inventory_hosts.remove(index);
            },
            remove_label,
            Some(Rc::new({
                let state = state.clone();
                move || {
                    let mut snapshot = state.borrow_mut();
                    let mut cloned = snapshot.current_assets.inventory_hosts[index].clone();
                    cloned.id = String::new();
                    cloned.name = format!("{} copy", cloned.name);
                    snapshot.current_assets.inventory_hosts.push(cloned);
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.raw_error = None;
                }
            })),
            refresh_pages,
        );
        append_entry_field(
            &card,
            "ID",
            &host.id,
            "prod-1",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].id = value;
            },
        );
        append_entry_field(
            &card,
            "Name",
            &host.name,
            "Production 1",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].name = value;
            },
        );
        append_entry_field(
            &card,
            "Hostname or address",
            &host.host,
            "prod.internal",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].host = value;
            },
        );
        append_entry_field(
            &card,
            "SSH user",
            &host.user,
            "deploy",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].user = value;
            },
        );
        append_entry_field(
            &card,
            "Provider",
            &host.provider,
            "aws",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].provider = value;
            },
        );
        append_entry_field(
            &card,
            "Main IP",
            &host.main_ip,
            "10.0.0.12",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].main_ip = value;
            },
        );
        append_entry_field(
            &card,
            "Port",
            &host.port.to_string(),
            "22",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].port =
                    value.parse::<u16>().unwrap_or(22);
            },
        );
        append_entry_field(
            &card,
            "Monthly cost (USD cents)",
            &host.price_per_month_usd_cents.to_string(),
            "0",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].price_per_month_usd_cents =
                    value.parse::<u64>().unwrap_or(0);
            },
        );
        append_entry_field(
            &card,
            "Password secret ref",
            host.password_secret_ref.as_deref().unwrap_or(""),
            "secret/prod-password",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].password_secret_ref =
                    none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "SSH key path",
            host.ssh_key_path.as_deref().unwrap_or(""),
            "~/.ssh/id_ed25519",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].ssh_key_path = none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Tags",
            &host.tags.join(", "),
            "prod, api",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_hosts[index].tags = parse_csv(&value);
            },
        );

        let groups_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        groups_box.append(
            &gtk::Label::builder()
                .label("Groups")
                .halign(gtk::Align::Start)
                .css_classes(["card-meta"])
                .build(),
        );
        if groups.is_empty() {
            groups_box.append(
                &gtk::Label::builder()
                    .label("No groups exist yet. Add a group first, then assign this host.")
                    .halign(gtk::Align::Start)
                    .wrap(true)
                    .css_classes(["field-hint"])
                    .build(),
            );
        } else {
            for group in &groups {
                let toggle = gtk::CheckButton::builder()
                    .label(&group.name)
                    .active(host.group_ids.contains(&group.id))
                    .build();
                let group_id = group.id.clone();
                let state = state.clone();
                let refresh_status = refresh_status.clone();
                toggle.connect_toggled(move |button| {
                    let mut snapshot = state.borrow_mut();
                    let groups = &mut snapshot.current_assets.inventory_hosts[index].group_ids;
                    if button.is_active() {
                        if groups.iter().all(|item| item != &group_id) {
                            groups.push(group_id.clone());
                        }
                    } else {
                        groups.retain(|item| item != &group_id);
                    }
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    refresh_status();
                });
                groups_box.append(&toggle);
            }
        }
        card.append(&groups_box);
        container.append(&card);
    }

    if snapshot.scope == ConfigScope::Workspace {
        for host in effective
            .inventory_hosts
            .iter()
            .filter(|item| !current_ids.contains(&item.id))
            .cloned()
        {
            let readonly = readonly_card(
                host.name.as_str(),
                host.id.as_str(),
                AssetItemSource::Global,
                "Inherited from global defaults. Override it here to adjust connection details for this workspace.",
            );
            readonly.append(&readonly_line(format!("{} as {}", host.host, host.user)));
            if !host.group_ids.is_empty() {
                readonly.append(&readonly_line(format!(
                    "Groups: {}",
                    host.group_ids.join(", ")
                )));
            }
            append_override_button(
                readonly.upcast_ref(),
                state,
                refresh_pages,
                dialog,
                move |snapshot| {
                    snapshot.current_assets.inventory_hosts.push(host.clone());
                },
            );
            container.append(&readonly);
        }
    }
}

fn render_groups_page(
    container: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
) {
    render_section_header(
        container,
        AssetSection::Groups,
        state,
        refresh_pages,
        dialog,
        move |snapshot| {
            snapshot
                .current_assets
                .inventory_groups
                .push(InventoryGroup {
                    id: String::new(),
                    name: String::new(),
                    tags: Vec::new(),
                });
        },
    );

    let snapshot = state.borrow().clone();
    let effective = effective_assets_for_scope(
        snapshot.scope,
        &snapshot.current_assets,
        &snapshot.global_assets,
    );
    let current_ids = snapshot
        .current_assets
        .inventory_groups
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    for (index, group) in snapshot
        .current_assets
        .inventory_groups
        .iter()
        .cloned()
        .enumerate()
    {
        let badge = group_source(
            snapshot.scope,
            &group.id,
            &snapshot.current_assets,
            &snapshot.global_assets,
        );
        let remove_label = if badge == AssetItemSource::WorkspaceOverride {
            "Remove override"
        } else {
            "Remove"
        };
        let card = asset_card_shell(
            &group.name,
            &group.id,
            badge,
            token,
            refresh_token,
            dialog,
            state,
            move |snapshot| {
                snapshot.current_assets.inventory_groups.remove(index);
            },
            remove_label,
            Some(Rc::new({
                let state = state.clone();
                move || {
                    let mut snapshot = state.borrow_mut();
                    let mut cloned = snapshot.current_assets.inventory_groups[index].clone();
                    cloned.id = String::new();
                    cloned.name = format!("{} copy", cloned.name);
                    snapshot.current_assets.inventory_groups.push(cloned);
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.raw_error = None;
                }
            })),
            refresh_pages,
        );
        append_entry_field(
            &card,
            "ID",
            &group.id,
            "prod",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_groups[index].id = value;
            },
        );
        append_entry_field(
            &card,
            "Name",
            &group.name,
            "Production",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_groups[index].name = value;
            },
        );
        append_entry_field(
            &card,
            "Tags",
            &group.tags.join(", "),
            "ops, critical",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.inventory_groups[index].tags = parse_csv(&value);
            },
        );
        container.append(&card);
    }

    if snapshot.scope == ConfigScope::Workspace {
        for group in effective
            .inventory_groups
            .iter()
            .filter(|item| !current_ids.contains(&item.id))
            .cloned()
        {
            let readonly = readonly_card(
                group.name.as_str(),
                group.id.as_str(),
                AssetItemSource::Global,
                "Inherited from global defaults. Override it here to rename or retag this group for the current workspace.",
            );
            append_override_button(
                readonly.upcast_ref(),
                state,
                refresh_pages,
                dialog,
                move |snapshot| {
                    snapshot.current_assets.inventory_groups.push(group.clone());
                },
            );
            container.append(&readonly);
        }
    }
}

fn render_roles_page(
    container: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
) {
    render_section_header(
        container,
        AssetSection::Roles,
        state,
        refresh_pages,
        dialog,
        move |snapshot| {
            snapshot
                .current_assets
                .role_templates
                .push(AgentRoleTemplate {
                    id: String::new(),
                    name: String::new(),
                    description: String::new(),
                    accent_class: String::from("accent-cyan"),
                    default_title: None,
                    default_agent_label: None,
                    default_startup_command: None,
                    default_connection_profile_id: None,
                    default_pane_groups: Vec::new(),
                    default_reconnect_policy: ReconnectPolicy::Manual,
                    default_output_helpers: Vec::new(),
                });
        },
    );

    let snapshot = state.borrow().clone();
    let effective = effective_assets_for_scope(
        snapshot.scope,
        &snapshot.current_assets,
        &snapshot.global_assets,
    );
    let current_ids = snapshot
        .current_assets
        .role_templates
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let connection_options = {
        let mut options = vec![("__none__".to_string(), "No default connection".to_string())];
        options.extend(
            effective
                .connection_profiles
                .iter()
                .map(|profile| (profile.id.clone(), profile.name.clone())),
        );
        options
    };

    for (index, role) in snapshot
        .current_assets
        .role_templates
        .iter()
        .cloned()
        .enumerate()
    {
        let badge = role_source(
            snapshot.scope,
            &role.id,
            &snapshot.current_assets,
            &snapshot.global_assets,
        );
        let remove_label = if badge == AssetItemSource::WorkspaceOverride {
            "Remove override"
        } else if badge == AssetItemSource::BuiltIn {
            "Reset to built-in"
        } else {
            "Remove"
        };
        let card = asset_card_shell(
            &role.name,
            &role.id,
            badge,
            token,
            refresh_token,
            dialog,
            state,
            move |snapshot| {
                snapshot.current_assets.role_templates.remove(index);
            },
            remove_label,
            Some(Rc::new({
                let state = state.clone();
                move || {
                    let mut snapshot = state.borrow_mut();
                    let mut cloned = snapshot.current_assets.role_templates[index].clone();
                    cloned.id = String::new();
                    cloned.name = format!("{} copy", cloned.name);
                    snapshot.current_assets.role_templates.push(cloned);
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.raw_error = None;
                }
            })),
            refresh_pages,
        );
        append_entry_field(
            &card,
            "ID",
            &role.id,
            "planner",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].id = value;
            },
        );
        append_entry_field(
            &card,
            "Name",
            &role.name,
            "Planner",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].name = value;
            },
        );
        append_entry_field(
            &card,
            "Description",
            &role.description,
            "Plan-first role for design work.",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].description = value;
            },
        );
        append_entry_field(
            &card,
            "Accent class",
            &role.accent_class,
            "accent-violet",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].accent_class = value;
            },
        );
        append_entry_field(
            &card,
            "Default title",
            role.default_title.as_deref().unwrap_or(""),
            "Planner",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_title = none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Agent label",
            role.default_agent_label.as_deref().unwrap_or(""),
            "Lead",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_agent_label =
                    none_if_empty(value);
            },
        );
        append_entry_field(
            &card,
            "Startup command",
            role.default_startup_command.as_deref().unwrap_or(""),
            "codex",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_startup_command =
                    none_if_empty(value);
            },
        );
        append_dynamic_combo_field(
            &card,
            "Default connection",
            &connection_options,
            role.default_connection_profile_id
                .clone()
                .unwrap_or_else(|| "__none__".into()),
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_connection_profile_id =
                    if value == "__none__" {
                        None
                    } else {
                        Some(value)
                    };
            },
        );
        append_combo_field(
            &card,
            "Reconnect policy",
            &[
                ("manual", "Manual"),
                ("abnormal", "On abnormal exit"),
                ("always", "Always"),
            ],
            match role.default_reconnect_policy {
                ReconnectPolicy::Manual => "manual",
                ReconnectPolicy::OnAbnormalExit => "abnormal",
                ReconnectPolicy::Always => "always",
            },
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_reconnect_policy =
                    match value.as_str() {
                        "abnormal" => ReconnectPolicy::OnAbnormalExit,
                        "always" => ReconnectPolicy::Always,
                        _ => ReconnectPolicy::Manual,
                    };
            },
        );
        append_entry_field(
            &card,
            "Pane groups",
            &role.default_pane_groups.join(", "),
            "planning, review",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[index].default_pane_groups =
                    parse_csv(&value);
            },
        );

        append_output_helpers_editor(
            &card,
            state,
            refresh_status,
            refresh_pages,
            index,
            &role.default_output_helpers,
        );
        container.append(&card);
    }

    if snapshot.scope == ConfigScope::Workspace {
        for role in effective
            .role_templates
            .iter()
            .filter(|item| !current_ids.contains(&item.id))
            .cloned()
        {
            let readonly = readonly_card(
                role.name.as_str(),
                role.id.as_str(),
                role_source(
                    snapshot.scope,
                    &role.id,
                    &snapshot.current_assets,
                    &snapshot.global_assets,
                ),
                "Inherited from built-in or global defaults. Override it here to tune behavior for this workspace.",
            );
            attach_readonly_role_details(&readonly, &role);
            append_override_button(
                readonly.upcast_ref(),
                state,
                refresh_pages,
                dialog,
                move |snapshot| {
                    snapshot.current_assets.role_templates.push(role.clone());
                },
            );
            container.append(&readonly);
        }
    }
}

fn render_runbooks_page(
    container: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
) {
    render_section_header(
        container,
        AssetSection::Runbooks,
        state,
        refresh_pages,
        dialog,
        move |snapshot| {
            snapshot.current_assets.runbooks.push(Runbook {
                id: String::new(),
                name: String::new(),
                description: String::new(),
                tags: Vec::new(),
                target: RunbookTarget::AllPanes,
                variables: Vec::new(),
                steps: Vec::new(),
                confirm_policy: RunbookConfirmPolicy::MultiPaneOrRemote,
            });
        },
    );

    let snapshot = state.borrow().clone();
    let effective = effective_assets_for_scope(
        snapshot.scope,
        &snapshot.current_assets,
        &snapshot.global_assets,
    );
    let current_ids = snapshot
        .current_assets
        .runbooks
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let role_options = effective
        .role_templates
        .iter()
        .map(|role| (role.id.clone(), role.name.clone()))
        .collect::<Vec<_>>();
    let connection_options = effective
        .connection_profiles
        .iter()
        .map(|profile| (profile.id.clone(), profile.name.clone()))
        .collect::<Vec<_>>();

    for (index, runbook) in snapshot.current_assets.runbooks.iter().cloned().enumerate() {
        let badge = runbook_source(
            snapshot.scope,
            &runbook.id,
            &snapshot.current_assets,
            &snapshot.global_assets,
        );
        let remove_label = if badge == AssetItemSource::WorkspaceOverride {
            "Remove override"
        } else {
            "Remove"
        };
        let card = asset_card_shell(
            &runbook.name,
            &runbook.id,
            badge,
            token,
            refresh_token,
            dialog,
            state,
            move |snapshot| {
                snapshot.current_assets.runbooks.remove(index);
            },
            remove_label,
            Some(Rc::new({
                let state = state.clone();
                move || {
                    let mut snapshot = state.borrow_mut();
                    let mut cloned = snapshot.current_assets.runbooks[index].clone();
                    cloned.id = String::new();
                    cloned.name = format!("{} copy", cloned.name);
                    snapshot.current_assets.runbooks.push(cloned);
                    snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                    snapshot.raw_error = None;
                }
            })),
            refresh_pages,
        );
        append_entry_field(
            &card,
            "ID",
            &runbook.id,
            "deploy",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[index].id = value;
            },
        );
        append_entry_field(
            &card,
            "Name",
            &runbook.name,
            "Deploy",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[index].name = value;
            },
        );
        append_entry_field(
            &card,
            "Description",
            &runbook.description,
            "Publish the latest build.",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[index].description = value;
            },
        );
        append_entry_field(
            &card,
            "Tags",
            &runbook.tags.join(", "),
            "release, prod",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[index].tags = parse_csv(&value);
            },
        );
        append_combo_field(
            &card,
            "Confirm policy",
            &[
                ("always", RunbookConfirmPolicy::Always.label()),
                ("multi", RunbookConfirmPolicy::MultiPaneOrRemote.label()),
                ("never", RunbookConfirmPolicy::Never.label()),
            ],
            match runbook.confirm_policy {
                RunbookConfirmPolicy::Always => "always",
                RunbookConfirmPolicy::Never => "never",
                RunbookConfirmPolicy::MultiPaneOrRemote => "multi",
            },
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[index].confirm_policy = match value.as_str() {
                    "always" => RunbookConfirmPolicy::Always,
                    "never" => RunbookConfirmPolicy::Never,
                    _ => RunbookConfirmPolicy::MultiPaneOrRemote,
                };
            },
        );

        append_runbook_target_editor(
            &card,
            state,
            refresh_status,
            index,
            &runbook.target,
            &role_options,
            &connection_options,
        );
        append_runbook_variables_editor(
            &card,
            state,
            refresh_status,
            refresh_pages,
            index,
            &runbook.variables,
        );
        append_runbook_steps_editor(
            &card,
            state,
            refresh_status,
            refresh_pages,
            index,
            &runbook.steps,
        );
        container.append(&card);
    }

    if snapshot.scope == ConfigScope::Workspace {
        for runbook in effective
            .runbooks
            .iter()
            .filter(|item| !current_ids.contains(&item.id))
            .cloned()
        {
            let readonly = readonly_card(
                runbook.name.as_str(),
                runbook.id.as_str(),
                AssetItemSource::Global,
                "Inherited from global defaults. Override it here to customize commands for this workspace.",
            );
            readonly.append(&readonly_line(runbook.target.label()));
            if !runbook.description.trim().is_empty() {
                readonly.append(&readonly_line(runbook.description.clone()));
            }
            append_override_button(
                readonly.upcast_ref(),
                state,
                refresh_pages,
                dialog,
                move |snapshot| {
                    snapshot.current_assets.runbooks.push(runbook.clone());
                },
            );
            container.append(&readonly);
        }
    }
}

fn render_section_header<F>(
    container: &gtk::Box,
    section: AssetSection,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
    on_add: F,
) where
    F: Fn(&mut AssetsManagerState) + Clone + 'static,
{
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["config-panel", "asset-section-header"])
        .build();
    let copy = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();
    copy.append(
        &gtk::Label::builder()
            .label(section.title())
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    copy.append(
        &gtk::Label::builder()
            .label(section.description())
            .halign(gtk::Align::Start)
            .wrap(true)
            .css_classes(["field-hint"])
            .build(),
    );
    row.append(&copy);
    let add_button = gtk::Button::builder()
        .label(format!("New {}", section.title().trim_end_matches('s')))
        .css_classes(["pill-button"])
        .build();
    let state = state.clone();
    let dialog = dialog.clone();
    let refresh_pages = refresh_pages.clone();
    add_button.connect_clicked(move |_| {
        let state_for_prompt = state.clone();
        let on_add = on_add.clone();
        let refresh_pages = refresh_pages.clone();
        maybe_discard_invalid_raw(&dialog, &state.borrow(), move || {
            let mut snapshot = state_for_prompt.borrow_mut();
            on_add(&mut snapshot);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            if let Some(refresh) = refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
    });
    row.append(&add_button);
    container.append(&row);
}

fn asset_card_shell<F>(
    title: &str,
    id: &str,
    badge: AssetItemSource,
    token: u64,
    refresh_token: &Rc<Cell<u64>>,
    dialog: &gtk::Dialog,
    state: &Rc<RefCell<AssetsManagerState>>,
    on_remove: F,
    remove_label: &str,
    on_duplicate: Option<Rc<dyn Fn()>>,
    refresh_pages: &RefreshHandle,
) -> gtk::Box
where
    F: Fn(&mut AssetsManagerState) + Clone + 'static,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "asset-card"])
        .build();
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let copy = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    copy.append(
        &gtk::Label::builder()
            .label(if title.trim().is_empty() {
                "Untitled item"
            } else {
                title
            })
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    copy.append(
        &gtk::Label::builder()
            .label(if id.trim().is_empty() {
                "No ID yet"
            } else {
                id
            })
            .halign(gtk::Align::Start)
            .css_classes(["card-meta"])
            .build(),
    );
    header.append(&copy);
    header.append(&source_badge(badge));

    if let Some(on_duplicate) = on_duplicate {
        let button = gtk::Button::builder()
            .label("Duplicate")
            .css_classes(["flat"])
            .build();
        let state = state.clone();
        let dialog = dialog.clone();
        let refresh_pages = refresh_pages.clone();
        button.connect_clicked(move |_| {
            let state_for_prompt = state.clone();
            let on_duplicate = on_duplicate.clone();
            let refresh_pages = refresh_pages.clone();
            maybe_discard_invalid_raw(&dialog, &state.borrow(), move || {
                on_duplicate();
                let mut snapshot = state_for_prompt.borrow_mut();
                snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
                snapshot.raw_error = None;
                if let Some(refresh) = refresh_pages.borrow().as_ref() {
                    refresh();
                }
            });
        });
        header.append(&button);
    }

    let remove_button = gtk::Button::builder()
        .label(remove_label)
        .css_classes(["flat", "destructive-button"])
        .build();
    let state = state.clone();
    let dialog = dialog.clone();
    let refresh_token = refresh_token.clone();
    let refresh_pages = refresh_pages.clone();
    remove_button.connect_clicked(move |_| {
        if refresh_token.get() != token {
            return;
        }
        let state_for_prompt = state.clone();
        let on_remove = on_remove.clone();
        let refresh_pages = refresh_pages.clone();
        maybe_discard_invalid_raw(&dialog, &state.borrow(), move || {
            let mut snapshot = state_for_prompt.borrow_mut();
            on_remove(&mut snapshot);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            if let Some(refresh) = refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
    });
    header.append(&remove_button);
    card.append(&header);
    card
}

fn readonly_card(title: &str, id: &str, source: AssetItemSource, detail: &str) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["config-panel", "asset-card", "asset-card-readonly"])
        .build();
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let copy = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    copy.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    copy.append(
        &gtk::Label::builder()
            .label(id)
            .halign(gtk::Align::Start)
            .css_classes(["card-meta"])
            .build(),
    );
    header.append(&copy);
    header.append(&source_badge(source));
    card.append(&header);
    card.append(&readonly_line(detail.to_string()));
    card
}

fn append_override_button<F>(
    card: &gtk::Widget,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_pages: &RefreshHandle,
    dialog: &gtk::Dialog,
    on_override: F,
) where
    F: Fn(&mut AssetsManagerState) + Clone + 'static,
{
    let button = gtk::Button::builder()
        .label("Override in workspace")
        .css_classes(["pill-button"])
        .halign(gtk::Align::Start)
        .build();
    let state = state.clone();
    let dialog = dialog.clone();
    let refresh_pages = refresh_pages.clone();
    button.connect_clicked(move |_| {
        let state_for_prompt = state.clone();
        let on_override = on_override.clone();
        let refresh_pages = refresh_pages.clone();
        maybe_discard_invalid_raw(&dialog, &state.borrow(), move || {
            let mut snapshot = state_for_prompt.borrow_mut();
            on_override(&mut snapshot);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            if let Some(refresh) = refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
    });
    if let Ok(card) = card.clone().downcast::<gtk::Box>() {
        card.append(&button);
    }
}

fn append_entry_field<F>(
    card: &gtk::Box,
    label: &str,
    value: &str,
    placeholder: &str,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    on_change: F,
) where
    F: Fn(&mut AssetsManagerState, String) + 'static,
{
    let row = labeled_row(label);
    let entry = gtk::Entry::builder()
        .hexpand(true)
        .text(value)
        .placeholder_text(placeholder)
        .build();
    let state = state.clone();
    let refresh_status = refresh_status.clone();
    entry.connect_changed(move |entry| {
        let mut snapshot = state.borrow_mut();
        on_change(&mut snapshot, entry.text().to_string());
        snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
        snapshot.raw_error = None;
        refresh_status();
    });
    row.append(&entry);
    card.append(&row);
}

fn append_combo_field<F>(
    card: &gtk::Box,
    label: &str,
    options: &[(&str, &str)],
    active: &str,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    on_change: F,
) where
    F: Fn(&mut AssetsManagerState, String) + 'static,
{
    let dynamic = options
        .iter()
        .map(|(id, title)| ((*id).to_string(), (*title).to_string()))
        .collect::<Vec<_>>();
    append_dynamic_combo_field(
        card,
        label,
        &dynamic,
        active.to_string(),
        state,
        refresh_status,
        on_change,
    );
}

fn append_dynamic_combo_field<F>(
    card: &gtk::Box,
    label: &str,
    options: &[(String, String)],
    active: String,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    on_change: F,
) where
    F: Fn(&mut AssetsManagerState, String) + 'static,
{
    let row = labeled_row(label);
    let combo = gtk::ComboBoxText::new();
    combo.add_css_class("surface-select-control");
    for (id, title) in options {
        combo.append(Some(id), title);
    }
    combo.set_active_id(Some(&active));
    let state = state.clone();
    let refresh_status = refresh_status.clone();
    combo.connect_changed(move |combo| {
        if let Some(id) = combo.active_id() {
            let mut snapshot = state.borrow_mut();
            on_change(&mut snapshot, id.to_string());
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            refresh_status();
        }
    });
    row.append(&combo);
    card.append(&row);
}

fn append_output_helpers_editor(
    card: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    role_index: usize,
    helpers: &[OutputHelperRule],
) {
    let section = nested_section("Output helpers");
    let add = gtk::Button::builder()
        .label("Add helper")
        .css_classes(["flat"])
        .halign(gtk::Align::Start)
        .build();
    let state_add = state.clone();
    let refresh_status_add = refresh_status.clone();
    let refresh_pages_add = refresh_pages.clone();
    add.connect_clicked(move |_| {
        let mut snapshot = state_add.borrow_mut();
        snapshot.current_assets.role_templates[role_index]
            .default_output_helpers
            .push(OutputHelperRule {
                id: String::new(),
                label: String::new(),
                regex: String::new(),
                severity: OutputSeverity::Warning,
                toast_on_match: true,
            });
        snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
        snapshot.raw_error = None;
        refresh_status_add();
        if let Some(refresh) = refresh_pages_add.borrow().as_ref() {
            refresh();
        }
    });
    section.append(&add);

    for (helper_index, helper) in helpers.iter().cloned().enumerate() {
        let helper_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["asset-nested-card"])
            .build();
        append_entry_field(
            &helper_row,
            "Helper ID",
            &helper.id,
            "compile-error",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[role_index].default_output_helpers
                    [helper_index]
                    .id = value;
            },
        );
        append_entry_field(
            &helper_row,
            "Label",
            &helper.label,
            "Compile error",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[role_index].default_output_helpers
                    [helper_index]
                    .label = value;
            },
        );
        append_entry_field(
            &helper_row,
            "Regex",
            &helper.regex,
            "(?i)error",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[role_index].default_output_helpers
                    [helper_index]
                    .regex = value;
            },
        );
        append_combo_field(
            &helper_row,
            "Severity",
            &[("info", "Info"), ("warning", "Warning"), ("error", "Error")],
            match helper.severity {
                OutputSeverity::Info => "info",
                OutputSeverity::Warning => "warning",
                OutputSeverity::Error => "error",
            },
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.role_templates[role_index].default_output_helpers
                    [helper_index]
                    .severity = match value.as_str() {
                    "info" => OutputSeverity::Info,
                    "error" => OutputSeverity::Error,
                    _ => OutputSeverity::Warning,
                };
            },
        );
        let toggle_row = labeled_row("Toast on match");
        let toggle = gtk::Switch::builder().active(helper.toast_on_match).build();
        let toggle_state = state.clone();
        let toggle_refresh_status = refresh_status.clone();
        toggle.connect_state_set(move |_, active| {
            let mut snapshot = toggle_state.borrow_mut();
            snapshot.current_assets.role_templates[role_index].default_output_helpers
                [helper_index]
                .toast_on_match = active;
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            toggle_refresh_status();
            false.into()
        });
        toggle_row.append(&toggle);
        helper_row.append(&toggle_row);

        let remove_button = gtk::Button::builder()
            .label("Remove helper")
            .css_classes(["flat", "destructive-button"])
            .halign(gtk::Align::Start)
            .build();
        let remove_state = state.clone();
        let remove_refresh_status = refresh_status.clone();
        let remove_refresh_pages = refresh_pages.clone();
        remove_button.connect_clicked(move |_| {
            let mut snapshot = remove_state.borrow_mut();
            snapshot.current_assets.role_templates[role_index]
                .default_output_helpers
                .remove(helper_index);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            remove_refresh_status();
            if let Some(refresh) = remove_refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
        helper_row.append(&remove_button);
        section.append(&helper_row);
    }

    card.append(&section);
}

fn append_runbook_target_editor(
    card: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    runbook_index: usize,
    target: &RunbookTarget,
    role_options: &[(String, String)],
    connection_options: &[(String, String)],
) {
    let section = nested_section("Target");
    append_combo_field(
        &section,
        "Target type",
        &[
            ("all", "All panes"),
            ("group", "Pane group"),
            ("role", "Role"),
            ("connection", "Connection"),
        ],
        match target {
            RunbookTarget::AllPanes => "all",
            RunbookTarget::PaneGroup(_) => "group",
            RunbookTarget::Role(_) => "role",
            RunbookTarget::ConnectionProfile(_) => "connection",
        },
        state,
        refresh_status,
        move |snapshot, value| {
            snapshot.current_assets.runbooks[runbook_index].target = match value.as_str() {
                "group" => RunbookTarget::PaneGroup(String::new()),
                "role" => RunbookTarget::Role(String::new()),
                "connection" => RunbookTarget::ConnectionProfile(String::new()),
                _ => RunbookTarget::AllPanes,
            };
        },
    );
    match target {
        RunbookTarget::PaneGroup(value) => append_entry_field(
            &section,
            "Pane group",
            value,
            "delivery",
            state,
            refresh_status,
            move |snapshot, next| {
                snapshot.current_assets.runbooks[runbook_index].target =
                    RunbookTarget::PaneGroup(next);
            },
        ),
        RunbookTarget::Role(value) => {
            let mut options = vec![("__none__".to_string(), "Choose a role".to_string())];
            options.extend(role_options.iter().cloned());
            append_dynamic_combo_field(
                &section,
                "Role",
                &options,
                if value.is_empty() {
                    "__none__".into()
                } else {
                    value.clone()
                },
                state,
                refresh_status,
                move |snapshot, next| {
                    snapshot.current_assets.runbooks[runbook_index].target =
                        RunbookTarget::Role(if next == "__none__" {
                            String::new()
                        } else {
                            next
                        });
                },
            );
        }
        RunbookTarget::ConnectionProfile(value) => {
            let mut options = vec![("__none__".to_string(), "Choose a connection".to_string())];
            options.extend(connection_options.iter().cloned());
            append_dynamic_combo_field(
                &section,
                "Connection",
                &options,
                if value.is_empty() {
                    "__none__".into()
                } else {
                    value.clone()
                },
                state,
                refresh_status,
                move |snapshot, next| {
                    snapshot.current_assets.runbooks[runbook_index].target =
                        RunbookTarget::ConnectionProfile(if next == "__none__" {
                            String::new()
                        } else {
                            next
                        });
                },
            );
        }
        RunbookTarget::AllPanes => {}
    }
    card.append(&section);
}

fn append_runbook_variables_editor(
    card: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    runbook_index: usize,
    variables: &[RunbookVariable],
) {
    let section = nested_section("Variables");
    let add = gtk::Button::builder()
        .label("Add variable")
        .css_classes(["flat"])
        .halign(gtk::Align::Start)
        .build();
    let state_add = state.clone();
    let refresh_status_add = refresh_status.clone();
    let refresh_pages_add = refresh_pages.clone();
    add.connect_clicked(move |_| {
        let mut snapshot = state_add.borrow_mut();
        snapshot.current_assets.runbooks[runbook_index]
            .variables
            .push(RunbookVariable {
                id: String::new(),
                label: String::new(),
                description: String::new(),
                default_value: None,
                required: true,
            });
        snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
        snapshot.raw_error = None;
        refresh_status_add();
        if let Some(refresh) = refresh_pages_add.borrow().as_ref() {
            refresh();
        }
    });
    section.append(&add);

    for (variable_index, variable) in variables.iter().cloned().enumerate() {
        let variable_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["asset-nested-card"])
            .build();
        append_entry_field(
            &variable_row,
            "Variable ID",
            &variable.id,
            "service",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].variables[variable_index].id =
                    value;
            },
        );
        append_entry_field(
            &variable_row,
            "Label",
            &variable.label,
            "Service name",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].variables[variable_index].label =
                    value;
            },
        );
        append_entry_field(
            &variable_row,
            "Description",
            &variable.description,
            "Used to target one service.",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].variables[variable_index]
                    .description = value;
            },
        );
        append_entry_field(
            &variable_row,
            "Default value",
            variable.default_value.as_deref().unwrap_or(""),
            "api",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].variables[variable_index]
                    .default_value = none_if_empty(value);
            },
        );
        let toggle_row = labeled_row("Required");
        let toggle = gtk::Switch::builder().active(variable.required).build();
        let toggle_state = state.clone();
        let toggle_refresh_status = refresh_status.clone();
        toggle.connect_state_set(move |_, active| {
            let mut snapshot = toggle_state.borrow_mut();
            snapshot.current_assets.runbooks[runbook_index].variables[variable_index].required =
                active;
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            toggle_refresh_status();
            false.into()
        });
        toggle_row.append(&toggle);
        variable_row.append(&toggle_row);
        let remove = gtk::Button::builder()
            .label("Remove variable")
            .css_classes(["flat", "destructive-button"])
            .halign(gtk::Align::Start)
            .build();
        let remove_state = state.clone();
        let remove_refresh_status = refresh_status.clone();
        let remove_refresh_pages = refresh_pages.clone();
        remove.connect_clicked(move |_| {
            let mut snapshot = remove_state.borrow_mut();
            snapshot.current_assets.runbooks[runbook_index]
                .variables
                .remove(variable_index);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            remove_refresh_status();
            if let Some(refresh) = remove_refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
        variable_row.append(&remove);
        section.append(&variable_row);
    }

    card.append(&section);
}

fn append_runbook_steps_editor(
    card: &gtk::Box,
    state: &Rc<RefCell<AssetsManagerState>>,
    refresh_status: &Rc<dyn Fn()>,
    refresh_pages: &RefreshHandle,
    runbook_index: usize,
    steps: &[RunbookStep],
) {
    let section = nested_section("Steps");
    let add = gtk::Button::builder()
        .label("Add step")
        .css_classes(["flat"])
        .halign(gtk::Align::Start)
        .build();
    let state_add = state.clone();
    let refresh_status_add = refresh_status.clone();
    let refresh_pages_add = refresh_pages.clone();
    add.connect_clicked(move |_| {
        let mut snapshot = state_add.borrow_mut();
        snapshot.current_assets.runbooks[runbook_index]
            .steps
            .push(RunbookStep {
                id: String::new(),
                label: String::new(),
                command: String::new(),
                append_newline: true,
            });
        snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
        snapshot.raw_error = None;
        refresh_status_add();
        if let Some(refresh) = refresh_pages_add.borrow().as_ref() {
            refresh();
        }
    });
    section.append(&add);

    for (step_index, step) in steps.iter().cloned().enumerate() {
        let step_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["asset-nested-card"])
            .build();
        append_entry_field(
            &step_row,
            "Step ID",
            &step.id,
            "restart",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].steps[step_index].id = value;
            },
        );
        append_entry_field(
            &step_row,
            "Label",
            &step.label,
            "Restart service",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].steps[step_index].label = value;
            },
        );
        append_entry_field(
            &step_row,
            "Command",
            &step.command,
            "systemctl restart app",
            state,
            refresh_status,
            move |snapshot, value| {
                snapshot.current_assets.runbooks[runbook_index].steps[step_index].command = value;
            },
        );
        let toggle_row = labeled_row("Append newline");
        let toggle = gtk::Switch::builder().active(step.append_newline).build();
        let toggle_state = state.clone();
        let toggle_refresh_status = refresh_status.clone();
        toggle.connect_state_set(move |_, active| {
            let mut snapshot = toggle_state.borrow_mut();
            snapshot.current_assets.runbooks[runbook_index].steps[step_index].append_newline =
                active;
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            toggle_refresh_status();
            false.into()
        });
        toggle_row.append(&toggle);
        step_row.append(&toggle_row);
        let remove = gtk::Button::builder()
            .label("Remove step")
            .css_classes(["flat", "destructive-button"])
            .halign(gtk::Align::Start)
            .build();
        let remove_state = state.clone();
        let remove_refresh_status = refresh_status.clone();
        let remove_refresh_pages = refresh_pages.clone();
        remove.connect_clicked(move |_| {
            let mut snapshot = remove_state.borrow_mut();
            snapshot.current_assets.runbooks[runbook_index]
                .steps
                .remove(step_index);
            snapshot.raw_toml = serialize_assets(&snapshot.current_assets);
            snapshot.raw_error = None;
            remove_refresh_status();
            if let Some(refresh) = remove_refresh_pages.borrow().as_ref() {
                refresh();
            }
        });
        step_row.append(&remove);
        section.append(&step_row);
    }

    card.append(&section);
}

fn labeled_row(label: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .css_classes(["card-meta"])
            .build(),
    );
    row
}

fn nested_section(title: &str) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["asset-nested-section"])
        .build();
    section.append(
        &gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    section
}

fn source_badge(source: AssetItemSource) -> gtk::Widget {
    gtk::Label::builder()
        .label(source.label())
        .halign(gtk::Align::End)
        .css_classes(["status-chip", "muted-chip"])
        .build()
        .upcast()
}

fn readonly_line(text: String) -> gtk::Widget {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["field-hint"])
        .build()
        .upcast()
}

fn attach_readonly_connection_details(card: &gtk::Box, profile: &ConnectionProfile) {
    card.append(&readonly_line(format!("{:?}", profile.kind)));
    if let Some(host_id) = profile.inventory_host_id.as_deref() {
        card.append(&readonly_line(format!("Host: {host_id}")));
    }
    if let Some(path) = profile.remote_working_directory.as_deref() {
        card.append(&readonly_line(format!("Remote dir: {path}")));
    }
}

fn attach_readonly_role_details(card: &gtk::Box, role: &AgentRoleTemplate) {
    if !role.description.trim().is_empty() {
        card.append(&readonly_line(role.description.clone()));
    }
    if let Some(command) = role.default_startup_command.as_deref() {
        card.append(&readonly_line(format!("Startup: {command}")));
    }
    if !role.default_pane_groups.is_empty() {
        card.append(&readonly_line(format!(
            "Pane groups: {}",
            role.default_pane_groups.join(", ")
        )));
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
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

fn sync_raw_buffer(text_view: &gtk::TextView, raw: &str) {
    let buffer = text_view.buffer();
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    if buffer.text(&start, &end, true).as_str() != raw {
        buffer.set_text(raw);
    }
}

fn serialize_assets(assets: &WorkspaceAssets) -> String {
    toml::to_string_pretty(assets).unwrap_or_else(|_| String::from("# serialization failed\n"))
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn none_if_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn is_dirty(state: &AssetsManagerState) -> bool {
    state.current_assets != state.loaded_assets
        || state.raw_error.is_some()
        || state.raw_toml != serialize_assets(&state.current_assets)
}

fn format_issue_summary(issues: &[AssetValidationIssue]) -> String {
    issues
        .iter()
        .take(6)
        .map(|issue| {
            if let Some(id) = issue.item_id.as_deref() {
                format!("{} / {}: {}", issue.section.title(), id, issue.message)
            } else {
                format!("{}: {}", issue.section.title(), issue.message)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn maybe_discard_invalid_raw<F>(dialog: &gtk::Dialog, state: &AssetsManagerState, on_confirm: F)
where
    F: Fn() + 'static,
{
    if state.raw_error.is_none() {
        on_confirm();
        return;
    }
    let prompt = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(dialog)
        .heading("Discard invalid raw TOML changes?")
        .body("The Raw TOML page contains parse errors. Continue and discard the invalid text, or stay and fix it.")
        .build();
    prompt.add_response("cancel", "Stay Here");
    prompt.add_response("discard", "Discard Invalid Text");
    prompt.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    prompt.set_default_response(Some("cancel"));
    prompt.set_close_response("cancel");
    prompt.connect_response(None, move |prompt, response| {
        if response == "discard" {
            on_confirm();
        }
        prompt.close();
    });
    prompt.present();
}

fn maybe_discard_unsaved<F>(dialog: &gtk::Dialog, state: &AssetsManagerState, on_confirm: F)
where
    F: Fn() + 'static,
{
    if !is_dirty(state) {
        on_confirm();
        return;
    }
    let prompt = adw::MessageDialog::builder()
        .modal(true)
        .transient_for(dialog)
        .heading("Discard unsaved assets changes?")
        .body("You have unsaved edits in this scope. Continue and discard them, or cancel to keep editing.")
        .build();
    prompt.add_response("cancel", "Keep Editing");
    prompt.add_response("discard", "Discard Changes");
    prompt.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    prompt.set_default_response(Some("cancel"));
    prompt.set_close_response("cancel");
    prompt.connect_response(None, move |prompt, response| {
        if response == "discard" {
            on_confirm();
        }
        prompt.close();
    });
    prompt.present();
}
