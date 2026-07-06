use gtk::glib;
use gtk::prelude::*;

use crate::ui::icons::{self, name as icon_name};

pub(crate) struct WorkspaceSummaryInput<'a> {
    pub(crate) name: &'a str,
    pub(crate) path: String,
    pub(crate) pane_groups: Vec<String>,
    pub(crate) controls_sensitive: bool,
    /// Whether this project already has a Kanban board on disk. Gates the "Open Board"
    /// header button so it only appears for projects with a previously set-up board.
    pub(crate) board_available: bool,
}

pub(crate) struct WorkspaceSummaryChrome {
    pub(crate) widget: gtk::Widget,
    pub(crate) alert_button: gtk::Button,
    pub(crate) broadcast_state: gtk::Label,
    pub(crate) broadcast_selector: gtk::ComboBoxText,
    pub(crate) broadcast_entry: gtk::Entry,
    pub(crate) broadcast_button: gtk::Button,
    pub(crate) add_terminal_tile_button: gtk::Button,
    pub(crate) add_web_tile_button: gtk::Button,
    pub(crate) url_entry: gtk::Entry,
    pub(crate) url_reload_button: gtk::Button,
    pub(crate) runbook_selector: gtk::ComboBoxText,
    pub(crate) runbook_button: gtk::Button,
    pub(crate) open_board_button: gtk::Button,
    pub(crate) path_label: gtk::Label,
}

pub(crate) struct WorkspaceAlertSidebarChrome {
    pub(crate) widget: gtk::Widget,
    #[cfg(any(
        target_os = "linux",
        all(target_os = "windows", feature = "windows-gtk-shell")
    ))]
    pub(crate) mark_all_read_button: gtk::Button,
    #[cfg(any(
        target_os = "linux",
        all(target_os = "windows", feature = "windows-gtk-shell")
    ))]
    pub(crate) alert_list: gtk::Box,
    #[cfg(any(
        target_os = "linux",
        all(target_os = "windows", feature = "windows-gtk-shell")
    ))]
    pub(crate) unread_badge: gtk::Label,
}

pub(crate) fn build_workspace_shell_chrome() -> gtk::Box {
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .hexpand(true)
        .vexpand(true)
        .build();
    make_shrinkable(&shell);
    shell
}

pub(crate) fn build_workspace_alert_sidebar_chrome(
    controls_sensitive: bool,
) -> WorkspaceAlertSidebarChrome {
    let mark_all_read_button = icons::labeled_button(
        "Mark All Read",
        icon_name::APPLY,
        &["flat", "surface-button"],
    );
    mark_all_read_button.set_sensitive(controls_sensitive);

    let alert_list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .valign(gtk::Align::Start)
        .build();
    // Fill the sidebar's height so alert rows render in full instead of being
    // clipped to the scroller's short natural height (which hid row detail and
    // the per-row Jump/Mark Read actions).
    let alert_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(320)
        .vexpand(true)
        .build();
    alert_scroller.set_child(Some(&alert_list));

    let alert_sidebar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_start(12)
        .css_classes(["config-panel", "alert-center-panel"])
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["alert-center-header"])
        .build();
    let title = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build();
    title.append(
        &gtk::Label::builder()
            .label("Alert Center")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .css_classes(["card-title"])
            .build(),
    );
    let unread_badge = gtk::Label::builder()
        .valign(gtk::Align::Center)
        .css_classes(["alert-count-badge"])
        .visible(false)
        .build();
    title.append(&unread_badge);
    header.append(&title);
    header.append(&mark_all_read_button);

    alert_sidebar.append(&header);
    alert_sidebar.append(&alert_scroller);

    WorkspaceAlertSidebarChrome {
        widget: alert_sidebar.upcast(),
        #[cfg(any(
            target_os = "linux",
            all(target_os = "windows", feature = "windows-gtk-shell")
        ))]
        mark_all_read_button,
        #[cfg(any(
            target_os = "linux",
            all(target_os = "windows", feature = "windows-gtk-shell")
        ))]
        alert_list,
        #[cfg(any(
            target_os = "linux",
            all(target_os = "windows", feature = "windows-gtk-shell")
        ))]
        unread_badge,
    }
}

pub(crate) fn build_workspace_content_chrome(
    layout_host: &impl glib::object::IsA<gtk::Widget>,
    alert_revealer: &impl glib::object::IsA<gtk::Widget>,
) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .vexpand(true)
        .build();
    content.append(layout_host);
    content.append(alert_revealer);
    content.upcast()
}

pub(crate) fn build_workspace_alert_revealer(
    alert_sidebar: &impl glib::object::IsA<gtk::Widget>,
) -> gtk::Revealer {
    let alert_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .reveal_child(false)
        // Start hidden: a collapsed revealer must request 0 width so the
        // hexpand layout host fills the whole content row. See the
        // child-revealed handler below for why visibility is driven here.
        .visible(false)
        .build();
    alert_revealer.set_child(Some(alert_sidebar));
    // Once the slide-out animation finishes, fully hide the revealer. A
    // `visible=false` widget is excluded from layout, which guarantees the
    // layout host reclaims the space even if the closing animation's final
    // 0-width frame is coalesced away while a busy terminal is repainting
    // (otherwise the revealer can stick at an intermediate width, leaving a
    // blank dead strip on the right until the window is resized).
    alert_revealer.connect_child_revealed_notify(|revealer| {
        if !revealer.is_child_revealed() {
            revealer.set_visible(false);
        }
    });
    alert_revealer
}

/// Toggle the Alert Center revealer. Opening makes it visible before revealing
/// so the slide-in animation runs; closing hides it once the slide-out
/// animation completes (handled in `build_workspace_alert_revealer`).
pub(crate) fn toggle_workspace_alert_revealer(revealer: &gtk::Revealer) {
    if revealer.reveals_child() {
        revealer.set_reveal_child(false);
    } else {
        revealer.set_visible(true);
        revealer.set_reveal_child(true);
    }
}

pub(crate) fn build_workspace_summary_chrome(
    input: WorkspaceSummaryInput<'_>,
) -> WorkspaceSummaryChrome {
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .css_classes(["workspace-summary", "workspace-summary-dense"])
        .build();

    let name_label = gtk::Label::builder()
        .label(input.name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["workspace-summary-name"])
        .build();
    make_shrinkable(&name_label);

    let alert_button = icons::labeled_button(
        "Alerts (0)",
        icon_name::ALERTS,
        &["flat", "surface-button", "workspace-toolbar-alert"],
    );
    alert_button.set_sensitive(input.controls_sensitive);

    let broadcast_state = gtk::Label::builder()
        .label("Broadcast Off")
        .valign(gtk::Align::Center)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(22)
        .css_classes(["status-chip", "muted-chip", "workspace-broadcast-status"])
        .build();

    let broadcast_selector = gtk::ComboBoxText::new();
    broadcast_selector.add_css_class("surface-select-control");
    broadcast_selector.add_css_class("workspace-toolbar-select");
    broadcast_selector.append(Some("off"), "Off");
    broadcast_selector.append(Some("all"), "All");
    for group in input.pane_groups {
        let id = format!("group:{group}");
        broadcast_selector.append(Some(&id), &format!("Group: {group}"));
    }
    broadcast_selector.set_active_id(Some("off"));
    broadcast_selector.set_sensitive(input.controls_sensitive);

    let broadcast_entry = gtk::Entry::builder()
        .placeholder_text("Command…")
        .width_chars(14)
        .css_classes(["workspace-broadcast-entry"])
        .sensitive(input.controls_sensitive)
        .build();
    let broadcast_button = toolbar_icon_button(icon_name::BROADCAST, "Send quick command");
    broadcast_button.set_sensitive(input.controls_sensitive);

    let add_terminal_tile_button = toolbar_icon_button(icon_name::TERMINAL, "Add Terminal Tile");
    add_terminal_tile_button.set_sensitive(input.controls_sensitive);

    let add_web_tile_button = toolbar_icon_button(icon_name::WEB, "Add Web Tile");
    add_web_tile_button.set_sensitive(input.controls_sensitive);

    let url_entry = gtk::Entry::builder()
        .placeholder_text("URL")
        .width_chars(24)
        .hexpand(false)
        .css_classes(["workspace-url-entry"])
        .sensitive(input.controls_sensitive)
        .build();
    let url_reload_button = toolbar_icon_button(icon_name::REFRESH, "Reload");
    url_reload_button.set_sensitive(input.controls_sensitive);
    // URL editing + reload only make sense once a web tile holds focus, so they
    // stay hidden until the runtime reveals them contextually. Keeps the bar
    // uncluttered for terminal-only workspaces.
    url_entry.set_visible(false);
    url_reload_button.set_visible(false);

    let runbook_selector = gtk::ComboBoxText::new();
    runbook_selector.add_css_class("surface-select-control");
    runbook_selector.add_css_class("workspace-toolbar-select");
    runbook_selector.append(Some(""), "Runbook");
    runbook_selector.set_active_id(Some(""));
    runbook_selector.set_sensitive(input.controls_sensitive);
    let runbook_button = toolbar_icon_button(icon_name::RUN, "Run selected runbook");
    runbook_button.set_sensitive(input.controls_sensitive);

    // Jumps to this project's Kanban board. Only shown when a board was previously set
    // up (its `.terminaltiler/board.json` exists), so terminal-only projects stay clean.
    let open_board_button = toolbar_icon_button(icon_name::LAYOUT, "Open Board");
    open_board_button.set_sensitive(input.controls_sensitive);

    let path_label = gtk::Label::builder()
        .label(input.path)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Start)
        .css_classes(["workspace-summary-path"])
        .build();
    make_shrinkable(&path_label);

    let broadcast_group = toolbar_group();
    broadcast_group.append(&broadcast_state);
    broadcast_group.append(&broadcast_selector);
    broadcast_group.append(&broadcast_entry);
    broadcast_group.append(&broadcast_button);

    let tiles_group = toolbar_group();
    tiles_group.append(&add_terminal_tile_button);
    tiles_group.append(&add_web_tile_button);
    tiles_group.append(&url_entry);
    tiles_group.append(&url_reload_button);

    let runbook_group = toolbar_group();
    runbook_group.append(&runbook_selector);
    runbook_group.append(&runbook_button);

    // Own group + leading divider, both gated on board availability so a hidden button
    // never leaves a dangling separator in the bar.
    let board_divider = toolbar_divider();
    board_divider.set_visible(input.board_available);
    let board_group = toolbar_group();
    board_group.append(&open_board_button);
    board_group.set_visible(input.board_available);

    summary.append(&name_label);
    summary.append(&alert_button);
    summary.append(&toolbar_divider());
    summary.append(&broadcast_group);
    summary.append(&toolbar_divider());
    summary.append(&tiles_group);
    summary.append(&toolbar_divider());
    summary.append(&runbook_group);
    summary.append(&board_divider);
    summary.append(&board_group);
    summary.append(&path_label);

    WorkspaceSummaryChrome {
        widget: summary.upcast(),
        alert_button,
        broadcast_state,
        broadcast_selector,
        broadcast_entry,
        broadcast_button,
        add_terminal_tile_button,
        add_web_tile_button,
        url_entry,
        url_reload_button,
        runbook_selector,
        runbook_button,
        open_board_button,
        path_label,
    }
}

fn toolbar_icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = icons::icon_button(
        icon_name,
        tooltip,
        &[
            "flat",
            "surface-button",
            "surface-button-icon",
            "workspace-toolbar-action",
        ],
    );
    if let Some(icon) = button
        .first_child()
        .and_then(|child| child.downcast::<gtk::Image>().ok())
    {
        icon.set_pixel_size(13);
    }
    button
}

/// A compact cluster that visually groups related toolbar controls. The
/// `toolbar-group` surface plus dividers keep hierarchy without reintroducing
/// bulky nested pills inside the dense workspace summary bar.
fn toolbar_group() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .valign(gtk::Align::Center)
        .css_classes(["toolbar-group"])
        .build()
}

/// A thin vertical rule placed between toolbar groups.
fn toolbar_divider() -> gtk::Separator {
    let divider = gtk::Separator::new(gtk::Orientation::Vertical);
    divider.add_css_class("toolbar-divider");
    divider.set_valign(gtk::Align::Center);
    divider
}

fn make_shrinkable<W: glib::object::IsA<gtk::Widget>>(widget: &W) {
    widget.set_size_request(0, 0);
    widget.set_overflow(gtk::Overflow::Hidden);
}
