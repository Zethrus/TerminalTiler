use std::cell::RefCell;
use std::rc::Rc;

use gdk::prelude::StaticType;
use gtk::glib;
use gtk::prelude::*;

use webkit6::prelude::*;

use crate::model::assets::WorkspaceAssets;
use crate::model::layout::TileSpec;
use crate::model::preset::ApplicationDensity;

pub struct WebTileView {
    pub widget: gtk::Widget,
    pub web_view: webkit6::WebView,
    pub tile: TileSpec,
    pub refresh_source_id: Rc<RefCell<Option<glib::SourceId>>>,
}

pub fn build(
    tile: &TileSpec,
    assets: &WorkspaceAssets,
    use_dark_palette: bool,
    _density: ApplicationDensity,
    on_swap: Rc<dyn Fn(String, String)>,
) -> WebTileView {
    let web_view = webkit6::WebView::new();

    if use_dark_palette {
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&web_view) {
            settings.set_enable_developer_extras(false);
        }
    }

    let url = tile
        .url
        .as_deref()
        .unwrap_or("about:blank");

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
    header.set_tooltip_text(Some("Drag this header to swap tile positions"));

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

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

    header.append(&left);
    header.append(&status);
    shell.append(&header);

    // Update title from page title
    {
        let title_label = title.clone();
        let web_view = web_view.clone();
        web_view.connect_title_notify(move |wv| {
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
        web_view.connect_uri_notify(move |wv| {
            if let Some(uri) = wv.uri() {
                status.set_text(&domain_from_url(uri.as_str()));
            }
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
    header.add_controller(drag_source);

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
    }
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
