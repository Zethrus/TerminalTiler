use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gdk::prelude::StaticType;
use gtk::glib;
use gtk::prelude::*;

use webkit6::prelude::*;

use crate::logging;
use crate::model::assets::WorkspaceAssets;
use crate::model::layout::{DEFAULT_WEB_URL, TileSpec};
use crate::model::preset::ApplicationDensity;

pub struct WebTileView {
    pub widget: gtk::Widget,
    pub web_view: webkit6::WebView,
    pub tile: TileSpec,
    pub refresh_source_id: Rc<RefCell<Option<glib::SourceId>>>,
    pub shutdown_flag: Rc<Cell<bool>>,
    pub close_button: gtk::Button,
}

pub fn build(
    tile: &TileSpec,
    _assets: &WorkspaceAssets,
    use_dark_palette: bool,
    _density: ApplicationDensity,
    on_swap: Rc<dyn Fn(String, String)>,
    on_close: Rc<dyn Fn(String)>,
    on_update_settings: Rc<dyn Fn(String, String, Option<u32>)>,
    on_reload: Rc<dyn Fn(String)>,
    get_settings: Rc<dyn Fn(String) -> Option<(String, Option<u32>)>>,
    can_close: bool,
) -> WebTileView {
    let web_view = webkit6::WebView::new();
    let shutdown_flag = Rc::new(Cell::new(false));

    if use_dark_palette {
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
            settings.set_enable_developer_extras(false);
        }
    }

    let url = tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL);

    web_view.load_uri(url);

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .css_classes(["terminal-card", tile.accent_class.as_str()])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["terminal-header"])
        .build();

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    left.set_tooltip_text(Some("Drag this header to swap tile positions"));

    let badge = gtk::Label::builder()
        .label("🌐")
        .halign(gtk::Align::Start)
        .css_classes(["agent-badge"])
        .build();
    let title = gtk::Label::builder()
        .label(&tile.title)
        .halign(gtk::Align::Start)
        .css_classes(["tile-title"])
        .build();

    left.append(&badge);
    left.append(&title);

    let status = gtk::Label::builder()
        .label(domain_from_url(url))
        .css_classes(["status-chip"])
        .build();

    let settings_button = build_header_icon_button(
        "preferences-system-symbolic",
        "Edit URL and refresh settings",
    );
    let settings_popover = gtk::Popover::new();
    settings_popover.add_css_class("web-tile-settings-popover");
    settings_popover.set_autohide(true);
    settings_popover.set_has_arrow(true);
    settings_popover.set_position(gtk::PositionType::Bottom);
    settings_popover.set_parent(&settings_button);

    let settings_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    settings_box.append(&build_settings_label("URL"));

    let url_entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text("https://example.com")
        .css_classes(["workspace-url-entry", "web-tile-settings-entry"])
        .build();
    settings_box.append(&url_entry);

    settings_box.append(&build_settings_label("Auto-refresh (seconds)"));
    let auto_refresh = gtk::SpinButton::with_range(0.0, 3600.0, 5.0);
    auto_refresh.set_numeric(true);
    auto_refresh.set_width_chars(6);
    auto_refresh.add_css_class("tile-count-input");
    auto_refresh.set_tooltip_text(Some(
        "Auto-refresh in seconds, 0 disables automatic reload.",
    ));
    settings_box.append(&auto_refresh);

    let settings_actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let reload_button = gtk::Button::builder()
        .label("Reload")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    let apply_button = gtk::Button::builder()
        .label("Apply")
        .focus_on_click(false)
        .css_classes(["flat", "surface-button"])
        .build();
    settings_actions.append(&reload_button);
    settings_actions.append(&apply_button);
    settings_box.append(&settings_actions);
    settings_popover.set_child(Some(&settings_box));

    let sync_settings_inputs = Rc::new({
        let url_entry = url_entry.clone();
        let auto_refresh = auto_refresh.clone();
        let get_settings = get_settings.clone();
        let tile_id = tile.id.clone();
        move || {
            let (current_url, refresh_seconds) =
                get_settings(tile_id.clone()).unwrap_or_else(|| (DEFAULT_WEB_URL.into(), None));
            url_entry.set_text(&current_url);
            auto_refresh.set_value(refresh_seconds.unwrap_or_default() as f64);
        }
    });
    {
        let sync_settings_inputs = sync_settings_inputs.clone();
        let settings_popover = settings_popover.clone();
        let url_entry = url_entry.clone();
        settings_button.connect_clicked(move |_| {
            sync_settings_inputs();
            if settings_popover.is_visible() {
                settings_popover.popdown();
            } else {
                settings_popover.popup();
                url_entry.grab_focus();
            }
        });
    }

    let apply_settings = Rc::new({
        let url_entry = url_entry.clone();
        let auto_refresh = auto_refresh.clone();
        let on_update_settings = on_update_settings.clone();
        let settings_popover = settings_popover.clone();
        let tile_id = tile.id.clone();
        move || {
            let refresh_seconds = match auto_refresh.value_as_int().max(0) {
                0 => None,
                value => Some(value as u32),
            };
            on_update_settings(
                tile_id.clone(),
                url_entry.text().to_string(),
                refresh_seconds,
            );
            settings_popover.popdown();
        }
    });
    {
        let apply_settings = apply_settings.clone();
        apply_button.connect_clicked(move |_| {
            apply_settings();
        });
    }
    {
        let apply_settings = apply_settings.clone();
        url_entry.connect_activate(move |_| {
            apply_settings();
        });
    }
    {
        let on_reload = on_reload.clone();
        let settings_popover = settings_popover.clone();
        let tile_id = tile.id.clone();
        reload_button.connect_clicked(move |_| {
            on_reload(tile_id.clone());
            settings_popover.popdown();
        });
    }

    let close_button = build_header_icon_button(
        "window-close-symbolic",
        if can_close {
            "Close tile"
        } else {
            "Cannot close the last tile"
        },
    );
    close_button.set_sensitive(can_close);
    {
        let tile_id = tile.id.clone();
        let on_close = on_close.clone();
        close_button.connect_clicked(move |_| {
            on_close(tile_id.clone());
        });
    }

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    actions.append(&status);
    actions.append(&settings_button);
    actions.append(&close_button);

    header.append(&left);
    header.append(&actions);
    shell.append(&header);

    // Update title from page title
    {
        let title_label = title.clone();
        let web_view = web_view.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_title_notify(move |wv| {
            if shutdown_flag.get() {
                return;
            }
            if let Some(new_title) = wv.title() {
                let new_title = new_title.to_string();
                if !new_title.is_empty() {
                    title_label.set_text(&new_title);
                }
            }
        });
    }

    // Update status from URI changes
    {
        let status = status.clone();
        let web_view = web_view.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_uri_notify(move |wv| {
            if shutdown_flag.get() {
                return;
            }
            if let Some(uri) = wv.uri() {
                status.set_text(&domain_from_url(uri.as_str()));
            }
        });
    }

    {
        let tile_id = tile.id.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_load_changed(move |wv, event| {
            if shutdown_flag.get() {
                return;
            }
            logging::info(format!(
                "web tile {} load event {:?} uri='{}'",
                tile_id,
                event,
                wv.uri()
                    .map(|uri| uri.to_string())
                    .unwrap_or_else(|| DEFAULT_WEB_URL.into())
            ));
        });
    }
    {
        let tile_id = tile.id.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_load_failed(move |_, event, failing_uri, error| {
            if shutdown_flag.get() {
                return false;
            }
            logging::error(format!(
                "web tile {} load failed event={:?} uri='{}' error='{}'",
                tile_id, event, failing_uri, error
            ));
            false
        });
    }
    {
        let tile_id = tile.id.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_load_failed_with_tls_errors(move |_, failing_uri, _, errors| {
            if shutdown_flag.get() {
                return false;
            }
            logging::error(format!(
                "web tile {} TLS load failure uri='{}' errors={:?}",
                tile_id, failing_uri, errors
            ));
            false
        });
    }
    {
        let tile_id = tile.id.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_web_process_terminated(move |wv, reason| {
            if shutdown_flag.get() {
                return;
            }
            logging::error(format!(
                "web tile {} web process terminated reason={:?} uri='{}'",
                tile_id,
                reason,
                wv.uri()
                    .map(|uri| uri.to_string())
                    .unwrap_or_else(|| DEFAULT_WEB_URL.into())
            ));
        });
    }

    let web_frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["web-tile-frame"])
        .build();
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);
    web_frame.append(&web_view);
    shell.append(&web_frame);

    // Context menu
    install_web_context_menu(&web_view, &shell);

    // Drag source on header
    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    {
        let tile_id = tile.id.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&tile_id.to_value()))
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_begin(move |_, _| {
            shell.add_css_class("is-dragging");
        });
    }
    {
        let shell = shell.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            shell.remove_css_class("is-dragging");
        });
    }
    left.add_controller(drag_source);

    // Drop target on shell
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
        let target_id = tile.id.clone();
        let on_swap = on_swap.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            shell.remove_css_class("is-drop-target");
            let Ok(dragged_id) = value.get::<String>() else {
                return false;
            };
            on_swap(dragged_id, target_id.clone());
            true
        });
    }
    shell.add_controller(drop_target);

    // Auto-refresh timer
    let refresh_source_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    if let Some(interval) = tile.auto_refresh_seconds {
        if interval > 0 {
            let wv = web_view.clone();
            let source_id = glib::timeout_add_seconds_local(interval, move || {
                wv.reload();
                glib::ControlFlow::Continue
            });
            *refresh_source_id.borrow_mut() = Some(source_id);
        }
    }

    WebTileView {
        widget: shell.upcast(),
        web_view,
        tile: tile.clone(),
        refresh_source_id,
        shutdown_flag,
        close_button,
    }
}

fn build_header_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .focus_on_click(false)
        .css_classes(["flat", "tile-header-action", "tile-header-close"])
        .build();
    button.set_tooltip_text(Some(tooltip));
    if let Some(img) = button.first_child() {
        let _ = img.pango_context();
    }
    button
}

fn build_settings_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .css_classes(["tile-header-popover-label"])
        .build()
}

fn domain_from_url(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
        .to_string()
}

fn install_web_context_menu(web_view: &webkit6::WebView, parent: &gtk::Box) {
    let popover = gtk::Popover::new();
    popover.add_css_class("terminal-context-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(parent);

    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .css_classes(["terminal-context-menu"])
        .build();

    let reload_button = build_context_button("Reload", Some("F5"));
    {
        let web_view = web_view.clone();
        let popover = popover.clone();
        reload_button.connect_clicked(move |_| {
            web_view.reload();
            popover.popdown();
        });
    }
    menu.append(&reload_button);

    let copy_url_button = build_context_button("Copy URL", None);
    {
        let web_view = web_view.clone();
        let popover = popover.clone();
        let parent = parent.clone();
        copy_url_button.connect_clicked(move |_| {
            if let Some(uri) = web_view.uri() {
                let display = parent.display();
                display.clipboard().set_text(uri.as_str());
            }
            popover.popdown();
        });
    }
    menu.append(&copy_url_button);

    popover.set_child(Some(&menu));

    let right_click = gtk::GestureClick::builder()
        .button(3)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    {
        let parent = parent.clone();
        let popover = popover.clone();
        right_click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            parent.grab_focus();
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            popover.popup();
        });
    }
    parent.add_controller(right_click);
}

fn build_context_button(label: &str, shortcut: Option<&str>) -> gtk::Button {
    let button = gtk::Button::builder()
        .focus_on_click(false)
        .css_classes(["flat", "terminal-context-action"])
        .build();

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["terminal-context-label"])
            .build(),
    );

    if let Some(shortcut) = shortcut {
        row.append(
            &gtk::Label::builder()
                .label(shortcut)
                .halign(gtk::Align::End)
                .css_classes(["terminal-context-shortcut"])
                .build(),
        );
    }

    button.set_child(Some(&row));
    button
}
