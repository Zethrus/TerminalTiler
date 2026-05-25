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
use crate::ui::tile_chrome::{
    GetWebTileSettings, TileHeaderInput, WEB_HEADER_BADGE_MAX_CHARS, append_web_tile_action_chrome,
    bind_web_tile_settings_popover, build_tile_frame, build_tile_header_chrome, build_tile_shell,
    build_web_tile_action_chrome, domain_from_url, make_shrinkable,
};
use crate::ui::tile_drag::TileDragPayload;
use crate::ui::web_context_menu::{self, WebContextMenuInput};

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

    let initial_domain = domain_from_url(&url);
    let shell = build_tile_shell(tile);
    let header = build_tile_header_chrome(TileHeaderInput {
        tile,
        badge_text: "🌐",
        badge_tooltip: "Web tile",
        badge_max_chars: WEB_HEADER_BADGE_MAX_CHARS,
        status_text: &initial_domain,
        status_tooltip: &url,
        status_ellipsize: gtk::pango::EllipsizeMode::End,
        drag_tooltip: "Drag this header to swap tile positions",
    });
    let left = header.drag_handle.clone();
    let title = header.title_label.clone();
    let status = header.status_label.clone();

    let tile_actions = build_web_tile_action_chrome(can_close);
    let settings_button = tile_actions.settings_button.clone();
    bind_web_tile_settings_popover(
        &settings_button,
        &tile.id,
        get_settings.clone(),
        on_update_settings.clone(),
        on_reload.clone(),
    );

    let close_button = tile_actions.close_button.clone();
    {
        let tile_id = tile.id.clone();
        let on_close = on_close.clone();
        close_button.connect_clicked(move |_| {
            on_close(tile_id.clone());
        });
    }

    let actions = header.actions.clone();
    append_web_tile_action_chrome(&actions, &tile_actions);

    shell.append(&header.widget);

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

    let web_frame = build_tile_frame("web-tile-frame");
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

fn install_web_context_menu(web_view: &webkit6::WebView, parent: &gtk::Box) {
    web_context_menu::install_right_click(
        parent,
        WebContextMenuInput {
            reload: Rc::new({
                let web_view = web_view.clone();
                move || {
                    web_view.reload();
                }
            }),
            current_url: Rc::new({
                let web_view = web_view.clone();
                move || web_view.uri().map(|uri| uri.to_string())
            }),
            open_error_context: "GTK web tile context",
        },
    );
}
