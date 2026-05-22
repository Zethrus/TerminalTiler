use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gdk::prelude::StaticType;
use gtk::glib;
use gtk::prelude::*;

use webkit6::prelude::*;

use crate::logging;
use crate::model::assets::WorkspaceAssets;
use crate::model::layout::{DEFAULT_WEB_URL, TileSpec, normalize_web_url};
use crate::model::preset::ApplicationDensity;
use crate::ui::context_menu;
use crate::ui::icons::{self, name as icon_name};
use crate::ui::tile_chrome::{
    HEADER_STATUS_MAX_CHARS, HEADER_TITLE_MAX_CHARS, WEB_HEADER_BADGE_MAX_CHARS,
    build_header_icon_button, configure_dynamic_header_label, domain_from_url,
};
use crate::ui::tile_drag::TileDragPayload;

type GetWebTileSettings = Rc<dyn Fn(String) -> Option<(String, Option<u32>)>>;

pub struct WebTileView {
    pub widget: gtk::Widget,
    pub web_view: webkit6::WebView,
    pub tile: TileSpec,
    pub refresh_source_id: Rc<RefCell<Option<glib::SourceId>>>,
    pub shutdown_flag: Rc<Cell<bool>>,
    pub close_button: gtk::Button,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    tile: &TileSpec,
    _assets: &WorkspaceAssets,
    use_dark_palette: bool,
    _density: ApplicationDensity,
    on_swap: Rc<dyn Fn(String, String)>,
    on_close: Rc<dyn Fn(String)>,
    on_update_settings: Rc<dyn Fn(String, String, Option<u32>)>,
    on_reload: Rc<dyn Fn(String)>,
    get_settings: GetWebTileSettings,
    can_close: bool,
) -> WebTileView {
    let web_view = webkit6::WebView::new();
    let shutdown_flag = Rc::new(Cell::new(false));

    if use_dark_palette && let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
        settings.set_enable_developer_extras(false);
    }

    let url = normalize_web_url(tile.url.as_deref().unwrap_or(DEFAULT_WEB_URL));

    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["terminal-card", tile.accent_class.as_str()])
        .build();
    make_shrinkable(&shell);

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["terminal-header"])
        .build();
    make_shrinkable(&header);

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    make_shrinkable(&left);
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
    configure_dynamic_header_label(
        &badge,
        "Web tile",
        WEB_HEADER_BADGE_MAX_CHARS,
        gtk::pango::EllipsizeMode::End,
    );
    configure_dynamic_header_label(
        &title,
        &tile.title,
        HEADER_TITLE_MAX_CHARS,
        gtk::pango::EllipsizeMode::End,
    );
    title.set_hexpand(true);

    left.append(&badge);
    left.append(&title);

    let initial_domain = domain_from_url(&url);
    let status = gtk::Label::builder()
        .label(&initial_domain)
        .css_classes(["status-chip"])
        .build();
    configure_dynamic_header_label(
        &status,
        &url,
        HEADER_STATUS_MAX_CHARS,
        gtk::pango::EllipsizeMode::End,
    );

    let settings_button =
        build_header_icon_button(icon_name::SETTINGS, "Edit URL and refresh settings");
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
    let reload_button =
        icons::labeled_button("Reload", icon_name::REFRESH, &["flat", "surface-button"]);
    reload_button.set_focus_on_click(false);
    let apply_button =
        icons::labeled_button("Apply", icon_name::APPLY, &["flat", "surface-button"]);
    apply_button.set_focus_on_click(false);
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
        icon_name::CLOSE,
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
                    title_label.set_tooltip_text(Some(&new_title));
                }
            }
        });
    }

    {
        let status = status.clone();
        let web_view = web_view.clone();
        let shutdown_flag = shutdown_flag.clone();
        web_view.connect_uri_notify(move |wv| {
            if shutdown_flag.get() {
                return;
            }
            if let Some(uri) = wv.uri() {
                let domain = domain_from_url(uri.as_str());
                status.set_text(&domain);
                status.set_tooltip_text(Some(uri.as_str()));
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
    make_shrinkable(&web_frame);
    web_view.set_hexpand(true);
    web_view.set_vexpand(true);
    make_shrinkable(&web_view);
    web_frame.append(&web_view);
    shell.append(&web_frame);
    defer_initial_navigation_until_mapped(&web_view, &url, &tile.id);

    install_web_context_menu(&web_view, &shell);

    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    {
        let tile_id = tile.id.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(
                &TileDragPayload::new(tile_id.clone()).to_value(),
            ))
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

    let drop_target = gtk::DropTarget::new(TileDragPayload::static_type(), gdk::DragAction::MOVE);
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
            let Ok(payload) = value.get::<TileDragPayload>() else {
                return false;
            };
            let dragged_id = payload.into_tile_id();
            on_swap(dragged_id, target_id.clone());
            true
        });
    }
    shell.add_controller(drop_target);

    let refresh_source_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    if let Some(interval) = tile.auto_refresh_seconds
        && interval > 0
    {
        let wv = web_view.clone();
        let source_id = glib::timeout_add_seconds_local(interval, move || {
            wv.reload();
            glib::ControlFlow::Continue
        });
        *refresh_source_id.borrow_mut() = Some(source_id);
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

fn make_shrinkable(widget: &impl IsA<gtk::Widget>) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}

fn defer_initial_navigation_until_mapped(web_view: &webkit6::WebView, url: &str, tile_id: &str) {
    let did_start_navigation = Rc::new(Cell::new(false));
    let url = url.to_string();
    let tile_id = tile_id.to_string();
    web_view.connect_map(move |web_view| {
        if did_start_navigation.replace(true) {
            return;
        }

        let web_view = web_view.clone();
        let url = url.clone();
        let tile_id = tile_id.clone();
        glib::idle_add_local_once(move || {
            logging::info(format!(
                "web tile {} initial navigation after map to {}",
                tile_id, url
            ));
            web_view.load_uri(&url);
        });
    });
}

fn build_settings_label(label: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .css_classes(["tile-header-popover-label"])
        .build()
}

fn install_web_context_menu(web_view: &webkit6::WebView, parent: &gtk::Box) {
    let popover = context_menu::popover(parent);
    let menu = context_menu::menu_box();

    let reload_button = context_menu::action_button("Reload", Some("F5"));
    {
        let web_view = web_view.clone();
        let popover = popover.clone();
        reload_button.connect_clicked(move |_| {
            web_view.reload();
            popover.popdown();
        });
    }
    menu.append(&reload_button);

    let copy_url_button = context_menu::action_button("Copy URL", None);
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
            context_menu::popup_at(&popover, x, y);
        });
    }
    parent.add_controller(right_click);
}
