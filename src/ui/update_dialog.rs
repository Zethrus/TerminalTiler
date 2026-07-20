//! Shared GTK update experience for the Linux and Windows GTK shells.
//!
//! The updater remains headless: this controller only translates its event
//! stream into a single, observable modal flow and delegates installation or
//! restart handoff back to the platform shell.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;

use crate::ui::dialog_chrome::{ModalAccent, ModalActionRole, PremiumModal};
use crate::ui::icons::name as icon_name;
use crate::update::{ReleaseInfo, UpdateEvent, UpdateService};

type ArtifactHandler = Rc<dyn Fn(ReleaseInfo, PathBuf)>;
type LaterHandler = Rc<dyn Fn()>;

struct ProgressWidgets {
    label: gtk::Label,
    detail: gtk::Label,
    bar: gtk::ProgressBar,
    cancel: gtk::Button,
}

/// Owns one update prompt/progress lifecycle. Both GTK shells retain one of
/// these while an update is actionable, so repeated worker events cannot stack
/// competing modal dialogs.
pub(crate) struct UpdateDialogController {
    window: adw::ApplicationWindow,
    service: UpdateService,
    release: ReleaseInfo,
    on_artifact: RefCell<Option<ArtifactHandler>>,
    on_later: LaterHandler,
    dialog: RefCell<Option<adw::Dialog>>,
    progress: RefCell<Option<ProgressWidgets>>,
    artifact: RefCell<Option<PathBuf>>,
}

impl UpdateDialogController {
    pub(crate) fn new(
        window: &adw::ApplicationWindow,
        service: UpdateService,
        release: ReleaseInfo,
        on_later: LaterHandler,
    ) -> Rc<Self> {
        Rc::new(Self {
            window: window.clone(),
            service,
            release,
            on_artifact: RefCell::new(None),
            on_later,
            dialog: RefCell::new(None),
            progress: RefCell::new(None),
            artifact: RefCell::new(None),
        })
    }

    pub(crate) fn present_release(self: &Rc<Self>) {
        self.close_current();
        let version = self.release.version.to_string();
        let notes = release_notes(&self.release);
        let release_url = release_url(&self.release);
        let weak = Rc::downgrade(self);
        let later = self.on_later.clone();
        let dialog = PremiumModal::new(
            "update-dialog",
            &format!("TerminalTiler {version} is available"),
        )
        .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
        .body(&notes)
        .action("Later", ModalActionRole::Secondary, true, move || later())
        .action("View Release", ModalActionRole::Ghost, false, move || {
            open_release_url(&release_url)
        })
        .action(
            "Install and Restart",
            ModalActionRole::Primary,
            false,
            move || {
                if let Some(controller) = weak.upgrade() {
                    controller.start_download();
                }
            },
        )
        .present_with_handle(Some(&self.window));
        *self.dialog.borrow_mut() = Some(dialog);
    }

    pub(crate) fn handle_event(self: &Rc<Self>, event: &UpdateEvent) {
        if !self.matches(event) {
            return;
        }
        match event {
            UpdateEvent::DownloadStarted { .. } => self.show_downloading(),
            UpdateEvent::DownloadProgress {
                downloaded, total, ..
            } => self.show_progress(*downloaded, *total),
            UpdateEvent::Verifying { .. } => self.show_verifying(),
            UpdateEvent::Downloaded { artifact, .. } => {
                *self.artifact.borrow_mut() = Some(artifact.clone());
            }
            UpdateEvent::DownloadCancelled { .. } => self.present_release(),
            UpdateEvent::DownloadFailed { error, .. } => self.show_download_failure(error),
            UpdateEvent::DebInstallStarted { .. } => self.show_installing(),
            UpdateEvent::DebInstallSucceeded { .. } => self.show_restarting(),
            UpdateEvent::DebInstallFailed { error, .. } => {
                self.show_install_failure(&error.actionable_message())
            }
            UpdateEvent::Available(_) => {}
        }
    }

    pub(crate) fn install_artifact(&self, release: ReleaseInfo, artifact: PathBuf) {
        if let Some(handler) = self.on_artifact.borrow().as_ref() {
            handler(release, artifact);
        }
    }

    pub(crate) fn set_artifact_handler(&self, handler: ArtifactHandler) {
        *self.on_artifact.borrow_mut() = Some(handler);
    }

    pub(crate) fn show_install_request_failure(self: &Rc<Self>, error: &str) {
        self.show_install_failure(&format!(
            "TerminalTiler could not request authorization or start the installer: {error}"
        ));
    }

    /// Post-install restart handoff is not an installation failure. In
    /// particular, Debian must not request PolicyKit or apt a second time once
    /// the package transaction has already completed successfully.
    pub(crate) fn show_restart_handoff_failure(self: &Rc<Self>, error: &str) {
        self.close_current();
        let later = self.on_later.clone();
        let dialog = PremiumModal::new("update-error-dialog", "Update installed, but restart failed")
            .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
            .body(&format!(
                "{error}\n\nTerminalTiler is still running and the update is already installed. Close and reopen TerminalTiler manually to finish."
            ))
            .action("OK", ModalActionRole::Primary, true, move || later())
            .present_with_handle(Some(&self.window));
        *self.dialog.borrow_mut() = Some(dialog);
    }

    pub(crate) fn show_restarting(&self) {
        self.show_atomic_phase(
            "Restarting to finish",
            "TerminalTiler will close, finish the update, and reopen. Keep this window open while the restart handoff begins.",
        );
    }

    /// Remove the final atomic modal before handing control to the normal quit
    /// action. Waiting for `closed` keeps any active-session confirmation in
    /// front, while the once-only continuation prevents duplicate quit requests.
    pub(crate) fn close_for_restart_handoff<F>(&self, continuation: F)
    where
        F: FnOnce() + 'static,
    {
        let Some(dialog) = self.take_current() else {
            continuation();
            return;
        };
        let continuation = Rc::new(RefCell::new(Some(continuation)));
        let continuation_after_close = continuation.clone();
        dialog.connect_closed(move |_| {
            if let Some(continuation) = continuation_after_close.borrow_mut().take() {
                continuation();
            }
        });
        dialog.force_close();
    }

    fn start_download(self: &Rc<Self>) {
        self.show_downloading();
        if let Err(error) = self.service.download(self.release.clone()) {
            self.show_download_failure(&error);
        }
    }

    fn request_cancel(self: &Rc<Self>) {
        if let Some(progress) = self.progress.borrow().as_ref() {
            progress.cancel.set_sensitive(false);
            progress.label.set_label("Cancelling download");
            progress
                .detail
                .set_label("Removing the partial update safely…");
        }
        if let Err(error) = self.service.cancel_download() {
            self.show_download_failure(&error);
        }
    }

    fn show_downloading(self: &Rc<Self>) {
        self.close_current();
        let progress_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["update-progress-content"])
            .build();
        let label = gtk::Label::builder()
            .label("Downloading update")
            .xalign(0.0)
            .css_classes(["update-progress-heading"])
            .build();
        let detail = gtk::Label::builder()
            .label(format!(
                "{} of {}",
                format_bytes(0),
                format_bytes(self.release.size)
            ))
            .xalign(0.0)
            .css_classes(["update-progress-detail"])
            .build();
        let bar = gtk::ProgressBar::builder()
            .show_text(true)
            .css_classes(["update-progress-bar"])
            .build();
        bar.set_fraction(0.0);
        bar.set_text(Some("0%"));
        progress_box.append(&label);
        progress_box.append(&detail);
        progress_box.append(&bar);

        let cancel = gtk::Button::with_label("Cancel");
        cancel.add_css_class("secondary-button");
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.request_cancel();
            }
        });
        let dialog = PremiumModal::new("update-progress-dialog", "Preparing update")
            .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
            .body("TerminalTiler is downloading a verified release.")
            .custom_content(&progress_box)
            .external_action(&cancel)
            .dismissible(false)
            .present_with_handle(Some(&self.window));
        *self.progress.borrow_mut() = Some(ProgressWidgets {
            label,
            detail,
            bar,
            cancel,
        });
        *self.dialog.borrow_mut() = Some(dialog);
    }

    fn show_progress(&self, downloaded: u64, total: u64) {
        let progress_state = self.progress.borrow();
        let Some(progress) = progress_state.as_ref() else {
            return;
        };
        let total = total.max(1);
        let downloaded = downloaded.min(total);
        let percent = downloaded.saturating_mul(100) / total;
        progress.bar.set_fraction(downloaded as f64 / total as f64);
        progress.bar.set_text(Some(&format!("{percent}%")));
        progress.detail.set_label(&format!(
            "{} of {}",
            format_bytes(downloaded),
            format_bytes(total)
        ));
    }

    fn show_verifying(&self) {
        let progress_state = self.progress.borrow();
        let Some(progress) = progress_state.as_ref() else {
            return;
        };
        progress.cancel.set_visible(false);
        progress.label.set_label("Verifying update");
        progress
            .detail
            .set_label("Checking the signed release digest before installation…");
        progress.bar.set_fraction(1.0);
        progress.bar.set_text(Some("100%"));
    }

    fn show_installing(&self) {
        self.show_atomic_phase(
            "Authorizing and installing",
            "TerminalTiler is requesting system authorization. Follow the PolicyKit or password prompt, then keep this window open while installation finishes.",
        );
    }

    fn show_atomic_phase(&self, heading: &str, body: &str) {
        self.close_current();
        let bar = gtk::ProgressBar::builder()
            .css_classes(["update-progress-bar", "update-progress-indeterminate"])
            .build();
        bar.set_pulse_step(0.12);
        bar.pulse();
        let pulse_bar = bar.downgrade();
        gtk::glib::timeout_add_local(Duration::from_millis(160), move || {
            let Some(bar) = pulse_bar.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            bar.pulse();
            gtk::glib::ControlFlow::Continue
        });
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["update-progress-content"])
            .build();
        content.append(&bar);
        let dialog = PremiumModal::new("update-progress-dialog", heading)
            .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
            .body(body)
            .custom_content(&content)
            .dismissible(false)
            .present_with_handle(Some(&self.window));
        *self.dialog.borrow_mut() = Some(dialog);
        *self.progress.borrow_mut() = None;
    }

    fn show_download_failure(self: &Rc<Self>, error: &str) {
        self.show_failure("Update download failed", error, true);
    }

    fn show_install_failure(self: &Rc<Self>, error: &str) {
        self.show_failure("Update not installed", error, false);
    }

    fn show_failure(self: &Rc<Self>, heading: &str, error: &str, redownload: bool) {
        self.close_current();
        let release_url = release_url(&self.release);
        let later = self.on_later.clone();
        let weak = Rc::downgrade(self);
        let retry_label = if redownload { "Retry" } else { "Retry install" };
        let dialog = PremiumModal::new("update-error-dialog", heading)
            .icon(icon_name::DIALOG_INFO, ModalAccent::Amber)
            .body(&format!(
                "{error}\n\nTerminalTiler is still running. You can retry or download {} manually.",
                self.release.version
            ))
            .action("Later", ModalActionRole::Secondary, true, move || later())
            .action("View Release", ModalActionRole::Ghost, false, move || {
                open_release_url(&release_url)
            })
            .action(retry_label, ModalActionRole::Primary, false, move || {
                if let Some(controller) = weak.upgrade() {
                    if redownload {
                        controller.start_download();
                    } else if let Some(artifact) = controller.artifact.borrow().clone() {
                        controller.show_installing();
                        controller.install_artifact(controller.release.clone(), artifact);
                    } else {
                        controller.start_download();
                    }
                }
            })
            .present_with_handle(Some(&self.window));
        *self.dialog.borrow_mut() = Some(dialog);
    }

    fn matches(&self, event: &UpdateEvent) -> bool {
        let release = match event {
            UpdateEvent::Available(release)
            | UpdateEvent::DownloadStarted { release }
            | UpdateEvent::DownloadProgress { release, .. }
            | UpdateEvent::Verifying { release }
            | UpdateEvent::Downloaded { release, .. }
            | UpdateEvent::DownloadCancelled { release }
            | UpdateEvent::DownloadFailed { release, .. }
            | UpdateEvent::DebInstallSucceeded { release }
            | UpdateEvent::DebInstallFailed { release, .. } => release,
            UpdateEvent::DebInstallStarted { version } => return *version == self.release.version,
        };
        release.version == self.release.version
    }

    fn close_current(&self) {
        if let Some(dialog) = self.take_current() {
            // Atomic phases remain non-dismissible to the user, but internal
            // phase changes must always be able to replace their modal.
            dialog.force_close();
        }
    }

    fn take_current(&self) -> Option<adw::Dialog> {
        *self.progress.borrow_mut() = None;
        self.dialog.borrow_mut().take()
    }
}

fn release_notes(release: &ReleaseInfo) -> String {
    if release.notes.trim().is_empty() {
        "This release contains improvements and fixes.".into()
    } else {
        release.notes.chars().take(1200).collect()
    }
}

fn release_url(release: &ReleaseInfo) -> String {
    format!(
        "https://github.com/Zethrus/TerminalTiler/releases/tag/{}",
        release.tag
    )
}

fn open_release_url(url: &str) {
    if url.starts_with("https://github.com/Zethrus/TerminalTiler/") {
        let _ = gtk::gio::AppInfo::launch_default_for_uri(url, None::<&gtk::gio::AppLaunchContext>);
    }
}

/// Human-readable binary byte formatting used by the progress modal.
pub(crate) fn format_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    format!("{:.1} MB", bytes as f64 / MIB as f64)
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_progress_totals_without_zero_or_overflow_surprises() {
        assert_eq!(format_bytes(0), "0.0 MB");
        assert_eq!(format_bytes(1), "0.0 MB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5120.0 MB");
    }
}
