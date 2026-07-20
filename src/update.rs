//! Core release update service.
//!
//! The updater deliberately keeps release parsing, provenance checks, and
//! artifact verification independent from the desktop shells.  Network and
//! filesystem work runs on a worker thread; the shells only consume events and
//! decide when to ask the user for consent.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::app_paths;
use crate::logging;

const RELEASE_ENDPOINT: &str = "https://api.github.com/repos/Zethrus/TerminalTiler/releases/latest";
const RELEASE_DOWNLOAD_ROOT: &str = "https://github.com/Zethrus/TerminalTiler/releases/download/";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_RELEASE_JSON_BYTES: u64 = 2 * 1024 * 1024;
/// A release larger than this is not a desktop updater payload we should ever
/// download without a product change.  This also bounds malicious metadata.
pub(crate) const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const MAX_READ_CHUNK: usize = 1024 * 1024;
/// Keep diagnostics useful without allowing a package manager to retain an
/// unbounded amount of output in the desktop process.
const MAX_INSTALL_DIAGNOSTIC_BYTES: usize = 8 * 1024;
const PKEXEC_PATH: &str = "/usr/bin/pkexec";
const DEBIAN_LAUNCHER_PATH: &str = "/usr/bin/terminaltiler";
// This binary is installed by the Debian package and owned by root.  Never
// run the per-user copied restart helper through pkexec: that copy exists only
// to survive an application upgrade and is not a privileged trust boundary.
const DEBIAN_PRIVILEGED_UPDATER_PATH: &str = "/opt/terminaltiler/bin/terminaltiler-updater";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallerKind {
    AppImage,
    Deb,
    Nsis,
    Msi,
    PortableExe,
}

impl InstallerKind {
    pub(crate) fn asset_name(self, version: &Version) -> String {
        match self {
            Self::AppImage => format!("TerminalTiler-{version}-x86_64.AppImage"),
            Self::Deb => format!("terminaltiler_{version}_amd64.deb"),
            Self::Nsis => format!("TerminalTiler-setup-{version}-x86_64.exe"),
            Self::Msi => format!("TerminalTiler-setup-{version}-x86_64.msi"),
            Self::PortableExe => format!("TerminalTiler-{version}-portable-x86_64.exe"),
        }
    }

    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Nsis => "nsis",
            Self::Msi => "msi",
            Self::PortableExe => "portable-exe",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Installation {
    pub(crate) kind: InstallerKind,
    /// The file which is replaced by an update (the originating AppImage or
    /// portable wrapper), or the executable used to locate its install root.
    pub(crate) target: PathBuf,
    pub(crate) helper: PathBuf,
}

impl Installation {
    pub(crate) fn target_path(&self) -> &Path {
        &self.target
    }

    pub(crate) fn update_dir(&self) -> Option<PathBuf> {
        app_paths::data_local_dir().map(|path| path.join("updates"))
    }
}

/// Detect only installations with explicit, trustworthy provenance.  A binary
/// launched from a checkout, a portable ZIP, or an embedded host returns None.
pub(crate) fn detect_installation() -> Option<Installation> {
    if !automatic_updates_enabled() {
        return None;
    }

    let current = std::env::current_exe().ok()?;
    let helper_name = if cfg!(windows) {
        "terminaltiler-updater.exe"
    } else {
        "terminaltiler-updater"
    };

    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let target = PathBuf::from(appimage);
        if target.is_absolute()
            && target.is_file()
            && target
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("AppImage"))
        {
            let helper = current.parent()?.join(helper_name);
            return helper.is_file().then_some(Installation {
                kind: InstallerKind::AppImage,
                target,
                helper,
            });
        }
    }

    let parent = current.parent()?;
    // The marker beside the payload is authoritative.  Registry metadata is
    // only a fallback for older installed copies and must still match the
    // executable's installation directory so a manually extracted ZIP cannot
    // inherit a machine-wide installer record.
    let marker = read_marker(parent)
        .or_else(|| read_marker(parent.parent()?))
        .or_else(|| windows_registry_marker(parent));
    let marker = marker.as_deref()?;
    let kind = match marker {
        "deb" => InstallerKind::Deb,
        "nsis" => InstallerKind::Nsis,
        "msi" => InstallerKind::Msi,
        "portable-exe" => InstallerKind::PortableExe,
        _ => return None,
    };
    let target = if kind == InstallerKind::PortableExe {
        portable_wrapper_argument()
            .or_else(|| std::env::var_os("TERMINALTILER_PORTABLE_WRAPPER").map(PathBuf::from))
            .filter(|path| {
                path.is_absolute()
                    && path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            })
            .unwrap_or_else(|| current.clone())
    } else if kind == InstallerKind::Deb {
        // Relaunch the system launcher after apt replaces the payload. It sets
        // the bundled runtime environment before it execs terminaltiler-bin.
        // The adjacent launcher is retained only for older packages that did
        // not install the /usr/bin entry.
        let system_launcher = PathBuf::from(DEBIAN_LAUNCHER_PATH);
        if system_launcher.is_file() {
            system_launcher
        } else {
            current
                .parent()
                .map(|parent| parent.join("terminaltiler"))
                .filter(|path| path.is_file())
                .unwrap_or_else(|| current.clone())
        }
    } else {
        current.clone()
    };
    let helper = parent.join(helper_name);
    helper.is_file().then_some(Installation {
        kind,
        target,
        helper,
    })
}

/// Whether this process may start the automatic-update worker.
///
/// CI smoke tests and embedded callers can opt out before a worker or any
/// network activity is created.  Keep this gate shared with installation
/// detection so every desktop shell applies the same policy.
pub(crate) fn automatic_updates_enabled() -> bool {
    cfg!(target_arch = "x86_64")
        && std::env::var_os("TERMINALTILER_DISABLE_UPDATES").is_none()
        && std::env::var_os("CARGO_MANIFEST_DIR").is_none()
}

fn portable_wrapper_argument() -> Option<PathBuf> {
    std::env::args_os().find_map(|argument| {
        let argument = argument.to_string_lossy();
        argument
            .strip_prefix("--terminaltiler-portable-wrapper=")
            .map(PathBuf::from)
    })
}

fn portable_wrapper_pid() -> Option<u32> {
    std::env::args_os().find_map(|argument| {
        let argument = argument.to_string_lossy();
        argument
            .strip_prefix("--terminaltiler-portable-pid=")
            .and_then(|value| value.parse().ok())
    })
}

fn read_marker(directory: &Path) -> Option<String> {
    let path = directory.join("terminaltiler-install-kind");
    let marker = fs::read_to_string(path).ok()?;
    let marker = marker.trim().to_ascii_lowercase();
    (!marker.is_empty()).then_some(marker)
}

#[cfg(windows)]
fn windows_registry_marker(install_directory: &Path) -> Option<String> {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Zethrus\TerminalTiler"])
        .output()
        .ok()?;
    let output = String::from_utf8_lossy(&output.stdout);
    let install_location = output
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .find("installlocation")
                .map(|index| &line[index..])
        })
        .and_then(|line| line.split_once("REG_SZ"))
        .map(|(_, value)| value.trim())?;
    let expected = install_directory.to_string_lossy().replace('/', "\\");
    if !install_location.eq_ignore_ascii_case(&expected) {
        return None;
    }
    let output = output.to_ascii_lowercase();
    ["nsis", "msi"]
        .into_iter()
        .find(|kind| {
            output.lines().any(|line| {
                line.to_ascii_lowercase().contains("installerkind") && line.contains(kind)
            })
        })
        .map(str::to_string)
}

#[cfg(not(windows))]
fn windows_registry_marker(_install_directory: &Path) -> Option<String> {
    None
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Version {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
}

impl Version {
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.strip_prefix('v').unwrap_or(input);
        let mut parts = input.split('.');
        let parse_component = |part: &str| {
            if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
                return None;
            }
            part.parse().ok()
        };
        let version = Self {
            major: parse_component(parts.next()?)?,
            minor: parse_component(parts.next()?)?,
            patch: parse_component(parts.next()?)?,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReleaseInfo {
    pub(crate) version: Version,
    pub(crate) tag: String,
    pub(crate) title: String,
    pub(crate) notes: String,
    pub(crate) asset_name: String,
    pub(crate) download_url: String,
    pub(crate) digest: String,
    pub(crate) size: u64,
    pub(crate) kind: InstallerKind,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SelectionError {
    InvalidCurrentVersion,
    InvalidReleaseTag,
    DraftOrPrerelease,
    NoExpectedAsset,
    InvalidAssetMetadata,
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{self:?}")
    }
}

/// Select a release without making any network calls.  Keeping this function
/// deterministic makes downgrade, digest, URL, and published-release policy
/// straightforward to test with fixture JSON.
pub(crate) fn select_release(
    current_version: &str,
    installation: &Installation,
    json: &str,
) -> Result<Option<ReleaseInfo>, SelectionError> {
    let current = Version::parse(current_version).ok_or(SelectionError::InvalidCurrentVersion)?;
    let release: GithubRelease =
        serde_json::from_str(json).map_err(|_| SelectionError::InvalidReleaseTag)?;
    if release.draft || release.prerelease {
        return Err(SelectionError::DraftOrPrerelease);
    }
    if !release.tag_name.starts_with('v') {
        return Err(SelectionError::InvalidReleaseTag);
    }
    let Some(version) = Version::parse(&release.tag_name) else {
        return Err(SelectionError::InvalidReleaseTag);
    };
    if version <= current {
        return Ok(None);
    }
    let asset_name = installation.kind.asset_name(&version);
    let Some(asset) = release.assets.iter().find(|asset| asset.name == asset_name) else {
        return Err(SelectionError::NoExpectedAsset);
    };
    let expected_url = format!("{RELEASE_DOWNLOAD_ROOT}{}/{}", release.tag_name, asset_name);
    let valid_api_url = asset
        .url
        .strip_prefix("https://api.github.com/repos/Zethrus/TerminalTiler/releases/assets/")
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
    if asset.browser_download_url != expected_url
        || !asset.browser_download_url.starts_with("https://")
        || !valid_api_url
        || asset.size == 0
        || asset.size >= MAX_ASSET_BYTES
    {
        return Err(SelectionError::InvalidAssetMetadata);
    }
    let Some(digest) = asset.digest.as_deref() else {
        return Err(SelectionError::InvalidAssetMetadata);
    };
    if !valid_digest(digest) {
        return Err(SelectionError::InvalidAssetMetadata);
    }
    Ok(Some(ReleaseInfo {
        version,
        tag: release.tag_name,
        title: if release.name.trim().is_empty() {
            format!("TerminalTiler {version}")
        } else {
            release.name
        },
        notes: release.body,
        asset_name,
        download_url: asset.browser_download_url.clone(),
        digest: digest.to_ascii_lowercase(),
        size: asset.size,
        kind: installation.kind,
    }))
}

fn valid_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug)]
pub(crate) enum DownloadError {
    NoUpdateDirectory,
    Io(io::Error),
    Http(String),
    DigestMismatch,
    TooLarge,
    Cancelled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoUpdateDirectory => write!(output, "no per-user update directory is available"),
            Self::Io(error) => write!(output, "filesystem error: {error}"),
            Self::Http(error) => write!(output, "download failed: {error}"),
            Self::DigestMismatch => write!(output, "download digest did not match GitHub metadata"),
            Self::TooLarge => write!(output, "download exceeded its declared safety limit"),
            Self::Cancelled => write!(output, "download cancelled"),
        }
    }
}

impl From<io::Error> for DownloadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn download_release(
    release: &ReleaseInfo,
    installation: &Installation,
    cancelled: &AtomicBool,
    on_verifying: impl FnOnce(),
    on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, DownloadError> {
    let directory = installation
        .update_dir()
        .ok_or(DownloadError::NoUpdateDirectory)?;
    fs::create_dir_all(&directory)?;
    let final_path = directory.join(&release.asset_name);
    let part_path = final_path.with_extension(format!(
        "{}.part",
        final_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("download")
    ));
    let _ = fs::remove_file(&part_path);

    let agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();
    let mut response = agent
        .get(&release.download_url)
        .header("User-Agent", "TerminalTiler-Core-Updater")
        .call()
        .map_err(|error| DownloadError::Http(error.to_string()))?;
    let result = write_verified_stream_with_progress(
        response.body_mut().as_reader(),
        VerifiedStreamTarget {
            part_path: &part_path,
            final_path: &final_path,
            expected_size: release.size,
            expected_digest: &release.digest,
            cancelled,
        },
        on_verifying,
        on_progress,
    );
    if result.is_err() {
        let _ = fs::remove_file(&part_path);
    }
    result
}

/// Stream an artifact through a bounded reader into a `.part` file.  This is
/// intentionally generic over `Read` so tests can exercise truncation,
/// cancellation, retry cleanup, and digest failures without a live GitHub
/// connection.
#[cfg(test)]
fn write_verified_stream<R: Read>(
    reader: R,
    part_path: &Path,
    final_path: &Path,
    expected_size: u64,
    expected_digest: &str,
    cancelled: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    write_verified_stream_with_progress(
        reader,
        VerifiedStreamTarget {
            part_path,
            final_path,
            expected_size,
            expected_digest,
            cancelled,
        },
        || {},
        |_, _| {},
    )
}

struct VerifiedStreamTarget<'a> {
    part_path: &'a Path,
    final_path: &'a Path,
    expected_size: u64,
    expected_digest: &'a str,
    cancelled: &'a AtomicBool,
}

/// The progress callback is deliberately invoked only after a successful file
/// write and only when the integer percentage advances. This makes progress
/// both truthful and cheap enough for GTK's main-loop event pump.
fn write_verified_stream_with_progress<R: Read>(
    mut reader: R,
    target: VerifiedStreamTarget<'_>,
    on_verifying: impl FnOnce(),
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, DownloadError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target.part_path)?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut last_percent = None;
    let mut buffer = vec![0u8; MAX_READ_CHUNK];
    let result = (|| {
        loop {
            if target.cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > target.expected_size || total >= MAX_ASSET_BYTES {
                return Err(DownloadError::TooLarge);
            }
            file.write_all(&buffer[..read])?;
            hash.update(&buffer[..read]);
            let percent = total.saturating_mul(100) / target.expected_size.max(1);
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                on_progress(total, target.expected_size);
            }
        }
        if total != target.expected_size {
            return Err(DownloadError::TooLarge);
        }
        on_verifying();
        // No cancellation is accepted after this point unless it arrives
        // before the atomic promotion.  That keeps the verified artifact from
        // becoming installable after a user has requested cancellation.
        if target.cancelled.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let actual = format!("sha256:{:x}", hash.finalize());
        if actual != target.expected_digest {
            return Err(DownloadError::DigestMismatch);
        }
        file.sync_all()?;
        drop(file);
        if target.cancelled.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        atomic_promote(target.part_path, target.final_path)?;
        if let Some(parent) = target.final_path.parent() {
            sync_directory(parent)?;
        }
        Ok(target.final_path.to_path_buf())
    })();
    if result.is_err() {
        let _ = fs::remove_file(target.part_path);
    }
    result
}

fn atomic_promote(part: &Path, destination: &Path) -> io::Result<()> {
    // On Unix rename replaces the destination in one operation.  Windows
    // cannot rename over an existing file, so remove only the previous
    // download (never an installed executable) before promoting the part.
    #[cfg(unix)]
    {
        fs::rename(part, destination)
    }
    #[cfg(not(unix))]
    {
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(part, destination)
    }
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

/// Failures from the in-session Debian installer. These are intentionally
/// distinct from deferred-helper failures: the app remains open so the user
/// can retry or choose a manual install after seeing the real authorization
/// outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DebInstallFailure {
    /// The downloaded file changed after the original verified download.
    ArtifactVerificationFailed { error: String },
    /// PolicyKit returned 126, which means its dialog was dismissed or no
    /// desktop agent was available to present it.
    AuthorizationPromptUnavailableOrDismissed { diagnostic: String },
    /// PolicyKit returned 127 before apt could run.
    AuthorizationFailed { diagnostic: String },
    /// The absolute pkexec executable could not be started at all.
    PrivilegeLauncherUnavailable { error: String },
    /// apt-get ran but did not complete the package transaction successfully.
    PackageManagerFailed {
        exit_code: Option<i32>,
        diagnostic: String,
    },
}

impl DebInstallFailure {
    pub(crate) fn actionable_message(&self) -> String {
        match self {
            Self::ArtifactVerificationFailed { .. } => {
                "The downloaded package could not be verified again, so TerminalTiler did not request administrator access. Download the release again or install it manually.".into()
            }
            Self::AuthorizationPromptUnavailableOrDismissed { .. } => {
                "The system authorization prompt was dismissed or could not be shown. Unlock your desktop and try again, or install the downloaded release manually.".into()
            }
            Self::AuthorizationFailed { .. } => {
                "System authorization failed before the package manager could start. Check that PolicyKit and apt are available, then try again or install the release manually.".into()
            }
            Self::PrivilegeLauncherUnavailable { .. } => format!(
                "The system authorization program ({PKEXEC_PATH}) is unavailable. Install the release manually or restore PolicyKit before retrying."
            ),
            Self::PackageManagerFailed { .. } => {
                "The package manager could not install the release. Resolve any package-manager error, then retry or install the release manually.".into()
            }
        }
    }
}

impl std::fmt::Display for DebInstallFailure {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArtifactVerificationFailed { error } => {
                write!(
                    output,
                    "downloaded Debian package verification failed: {error}"
                )
            }
            Self::AuthorizationPromptUnavailableOrDismissed { diagnostic } => write!(
                output,
                "PolicyKit authorization dialog was dismissed or unavailable: {diagnostic}"
            ),
            Self::AuthorizationFailed { diagnostic } => {
                write!(output, "PolicyKit authorization failed: {diagnostic}")
            }
            Self::PrivilegeLauncherUnavailable { error } => {
                write!(output, "could not start {PKEXEC_PATH}: {error}")
            }
            Self::PackageManagerFailed {
                exit_code,
                diagnostic,
            } => write!(
                output,
                "apt-get installation failed with {}: {diagnostic}",
                exit_code
                    .map(|code| format!("exit code {code}"))
                    .unwrap_or_else(|| "an unavailable exit status".into())
            ),
        }
    }
}

/// Construct the privileged Debian installation command without involving a
/// shell. Both executables are absolute so the desktop updater cannot inherit
/// a user-controlled PATH when it crosses the PolicyKit boundary.
fn deb_install_command(release: &ReleaseInfo, artifact: &Path) -> Command {
    let digest = release
        .digest
        .strip_prefix("sha256:")
        .expect("validated Debian releases always have a SHA-256 digest");
    let mut command = Command::new(PKEXEC_PATH);
    command
        .arg(DEBIAN_PRIVILEGED_UPDATER_PATH)
        .arg("--install-deb")
        .arg("--artifact")
        .arg(artifact)
        .arg("--version")
        .arg(release.version.to_string())
        .arg("--size")
        .arg(release.size.to_string())
        .arg("--digest")
        .arg(digest);
    command
}

fn classify_deb_install_failure(exit_code: Option<i32>, diagnostic: String) -> DebInstallFailure {
    match exit_code {
        Some(126) => DebInstallFailure::AuthorizationPromptUnavailableOrDismissed { diagnostic },
        Some(127) => DebInstallFailure::AuthorizationFailed { diagnostic },
        _ => DebInstallFailure::PackageManagerFailed {
            exit_code,
            diagnostic,
        },
    }
}

/// Drain a process stream completely while retaining only a bounded prefix for
/// user-facing diagnostics. Continuing to drain after the bound prevents a
/// verbose apt process from blocking on a full stderr pipe.
fn bounded_diagnostic<R: Read>(mut reader: R, limit: usize) -> io::Result<String> {
    let mut captured = Vec::with_capacity(limit);
    let mut buffer = [0u8; 4096];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    let mut diagnostic = String::from_utf8_lossy(&captured).trim().to_string();
    if diagnostic.is_empty() {
        diagnostic = "no diagnostic output was provided".into();
    }
    if truncated {
        diagnostic.push_str("\n[diagnostic output truncated]");
    }
    Ok(diagnostic)
}

/// Recheck the on-disk Debian payload immediately before handing its path to
/// apt. The download path is user-writable and could otherwise change after
/// the original streaming verification completed.
fn verify_deb_artifact(release: &ReleaseInfo, artifact: &Path) -> Result<(), String> {
    if release.kind != InstallerKind::Deb {
        return Err("a non-Debian release was sent to the Debian verifier".into());
    }
    if !artifact.is_absolute()
        || !artifact.is_file()
        || artifact.file_name().and_then(|name| name.to_str()) != Some(release.asset_name.as_str())
    {
        return Err("the downloaded Debian package path is no longer valid".into());
    }
    if release.size == 0 || release.size >= MAX_ASSET_BYTES {
        return Err("the downloaded Debian package has an invalid expected size".into());
    }
    let expected = release
        .digest
        .strip_prefix("sha256:")
        .ok_or("the downloaded Debian package has an invalid expected digest")?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the downloaded Debian package has an invalid expected digest".into());
    }
    let mut file = File::open(artifact).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; MAX_READ_CHUNK];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > release.size || total >= MAX_ASSET_BYTES {
            return Err("the downloaded Debian package size changed".into());
        }
        hash.update(&buffer[..read]);
    }
    if total != release.size {
        return Err("the downloaded Debian package size changed".into());
    }
    let actual = format!("{:x}", hash.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("the downloaded Debian package digest changed".into());
    }
    Ok(())
}

fn install_deb_while_application_is_running(
    release: &ReleaseInfo,
    artifact: &Path,
) -> Result<(), DebInstallFailure> {
    if !Path::new(DEBIAN_PRIVILEGED_UPDATER_PATH).is_file() {
        return Err(DebInstallFailure::PackageManagerFailed {
            exit_code: None,
            diagnostic: format!(
                "the root-owned Debian update helper is missing at {DEBIAN_PRIVILEGED_UPDATER_PATH}"
            ),
        });
    }
    let mut command = deb_install_command(release, artifact);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child =
        command
            .spawn()
            .map_err(|error| DebInstallFailure::PrivilegeLauncherUnavailable {
                error: error.to_string(),
            })?;
    let stderr = child
        .stderr
        .take()
        .expect("stderr is piped before spawning the Debian installer");
    let diagnostics =
        thread::spawn(move || bounded_diagnostic(stderr, MAX_INSTALL_DIAGNOSTIC_BYTES));
    let status = child
        .wait()
        .map_err(|error| DebInstallFailure::PackageManagerFailed {
            exit_code: None,
            diagnostic: error.to_string(),
        })?;
    let diagnostic = diagnostics
        .join()
        .unwrap_or_else(|_| Ok("could not collect package-manager diagnostics".into()))
        .unwrap_or_else(|error| format!("could not read package-manager diagnostics: {error}"));
    if status.success() {
        Ok(())
    } else {
        Err(classify_deb_install_failure(status.code(), diagnostic))
    }
}

#[derive(Debug)]
pub(crate) enum UpdateEvent {
    Available(ReleaseInfo),
    DownloadStarted {
        release: ReleaseInfo,
    },
    DownloadProgress {
        release: ReleaseInfo,
        downloaded: u64,
        total: u64,
    },
    Verifying {
        release: ReleaseInfo,
    },
    Downloaded {
        release: ReleaseInfo,
        artifact: PathBuf,
    },
    DownloadCancelled {
        release: ReleaseInfo,
    },
    DownloadFailed {
        release: ReleaseInfo,
        error: String,
    },
    DebInstallStarted {
        version: Version,
    },
    DebInstallSucceeded {
        release: ReleaseInfo,
    },
    DebInstallFailed {
        release: ReleaseInfo,
        error: DebInstallFailure,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct UpdateResult {
    pub(crate) version: String,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
}

/// Consume the helper's result breadcrumb once.  Success records are ignored
/// by the UI, while failures are surfaced on the next launch so an installer
/// authorization or runtime error is actionable without prompting repeatedly.
pub(crate) fn take_update_result() -> Option<UpdateResult> {
    let path = app_paths::state_dir()?.join("update-result.json");
    let result = fs::read_to_string(&path)
        .ok()
        .and_then(|contents| serde_json::from_str::<UpdateResult>(&contents).ok());
    let _ = fs::remove_file(path);
    result
}

enum UpdateCommand {
    Download(ReleaseInfo),
    InstallDeb {
        release: ReleaseInfo,
        artifact: PathBuf,
    },
    Stop,
}

#[derive(Clone)]
pub(crate) struct UpdateService {
    inner: Arc<UpdateServiceInner>,
}

struct UpdateServiceInner {
    command_tx: Sender<UpdateCommand>,
    cancelled: Arc<AtomicBool>,
}

impl UpdateService {
    pub(crate) fn start() -> (Self, Receiver<UpdateEvent>) {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        thread::Builder::new()
            .name("terminaltiler-update".into())
            .spawn(move || update_worker(command_rx, event_tx, worker_cancelled))
            .expect("update worker thread should start");
        (
            Self {
                inner: Arc::new(UpdateServiceInner {
                    command_tx,
                    cancelled,
                }),
            },
            event_rx,
        )
    }

    pub(crate) fn download(&self, release: ReleaseInfo) -> Result<(), String> {
        self.inner.cancelled.store(false, Ordering::Relaxed);
        self.inner
            .command_tx
            .send(UpdateCommand::Download(release))
            .map_err(|_| "the update worker is no longer running".into())
    }

    /// Cancellation is intentionally limited to the streaming phase. The
    /// worker acknowledges it with `DownloadCancelled` after deleting its
    /// partial artifact; callers keep the progress window open until then.
    pub(crate) fn cancel_download(&self) -> Result<(), String> {
        self.inner.cancelled.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Request the authenticated Debian transaction on the worker thread.
    /// Calling this never blocks the GTK main loop and, unlike the legacy
    /// deferred helper, starts PolicyKit while this process still owns the
    /// active desktop session.
    pub(crate) fn install_deb(
        &self,
        release: ReleaseInfo,
        artifact: PathBuf,
    ) -> Result<(), String> {
        if release.kind != InstallerKind::Deb {
            return Err("a non-Debian release was sent to the Debian installer".into());
        }
        if !artifact.is_absolute() || !artifact.is_file() {
            return Err("the verified Debian package is no longer available".into());
        }
        self.inner
            .command_tx
            .send(UpdateCommand::InstallDeb { release, artifact })
            .map_err(|_| "the update worker is no longer running".into())
    }
}

// A UI modal owns a temporary `UpdateService` clone.  Stopping the worker from
// `UpdateService::drop` would therefore turn a normal "Later" dismissal into
// a permanent opt-out for the rest of the application session.  The worker is
// stopped only after its shared service state has no remaining owners.
impl Drop for UpdateServiceInner {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let _ = self.command_tx.send(UpdateCommand::Stop);
    }
}

fn update_worker(
    command_rx: Receiver<UpdateCommand>,
    event_tx: Sender<UpdateEvent>,
    cancelled: Arc<AtomicBool>,
) {
    let installation = detect_installation();
    let mut prompted_versions = HashSet::<Version>::new();
    let mut check = true;
    loop {
        let command = if check {
            check = false;
            None
        } else {
            match command_rx.recv_timeout(CHECK_INTERVAL) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => {
                    check = true;
                    None
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match command {
            Some(UpdateCommand::Stop) => return,
            None => {
                let Some(installation) = installation.as_ref() else {
                    continue;
                };
                match query_latest_release(installation) {
                    Ok(Some(release)) if prompted_versions.insert(release.version) => {
                        let _ = event_tx.send(UpdateEvent::Available(release));
                    }
                    Ok(_) => {}
                    Err(error) => logging::info(format!("automatic update check skipped: {error}")),
                }
            }
            Some(UpdateCommand::Download(release)) => {
                let Some(installation) = installation.as_ref() else {
                    continue;
                };
                cancelled.store(false, Ordering::Relaxed);
                let _ = event_tx.send(UpdateEvent::DownloadStarted {
                    release: release.clone(),
                });
                let verifying_release = release.clone();
                let verifying_tx = event_tx.clone();
                let progress_release = release.clone();
                let progress_tx = event_tx.clone();
                match download_release(
                    &release,
                    installation,
                    &cancelled,
                    move || {
                        let _ = verifying_tx.send(UpdateEvent::Verifying {
                            release: verifying_release,
                        });
                    },
                    move |downloaded, total| {
                        let _ = progress_tx.send(UpdateEvent::DownloadProgress {
                            release: progress_release.clone(),
                            downloaded,
                            total,
                        });
                    },
                ) {
                    Ok(artifact) => {
                        let _ = event_tx.send(UpdateEvent::Downloaded { release, artifact });
                    }
                    Err(DownloadError::Cancelled) => {
                        let _ = event_tx.send(UpdateEvent::DownloadCancelled { release });
                    }
                    Err(error) => {
                        let _ = event_tx.send(UpdateEvent::DownloadFailed {
                            release,
                            error: error.to_string(),
                        });
                    }
                }
            }
            Some(UpdateCommand::InstallDeb { release, artifact }) => {
                if release.kind != InstallerKind::Deb {
                    let _ = event_tx.send(UpdateEvent::DebInstallFailed {
                        release,
                        error: DebInstallFailure::PackageManagerFailed {
                            exit_code: None,
                            diagnostic: "a non-Debian release reached the Debian installer".into(),
                        },
                    });
                    continue;
                }
                if let Err(error) = verify_deb_artifact(&release, &artifact) {
                    let _ = event_tx.send(UpdateEvent::DebInstallFailed {
                        release,
                        error: DebInstallFailure::ArtifactVerificationFailed { error },
                    });
                    continue;
                }
                let _ = event_tx.send(UpdateEvent::DebInstallStarted {
                    version: release.version,
                });
                match install_deb_while_application_is_running(&release, &artifact) {
                    Ok(()) => {
                        let _ = event_tx.send(UpdateEvent::DebInstallSucceeded { release });
                    }
                    Err(error) => {
                        let _ = event_tx.send(UpdateEvent::DebInstallFailed { release, error });
                    }
                }
            }
        }
    }
}

fn query_latest_release(installation: &Installation) -> Result<Option<ReleaseInfo>, String> {
    let agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();
    let mut response = agent
        .get(RELEASE_ENDPOINT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "TerminalTiler-Core-Updater")
        .call()
        .map_err(|error| error.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RELEASE_JSON_BYTES)
        .read_to_string()
        .map_err(|error| error.to_string())?;
    select_release(crate::product::PRODUCT_VERSION, installation, &body)
        .map_err(|error| error.to_string())
}

/// Spawn the bundled helper.  Metadata is passed as individual argv values;
/// release data is never interpolated into a shell command.
pub(crate) fn spawn_updater(
    release: &ReleaseInfo,
    artifact: &Path,
    installation: &Installation,
) -> Result<(), String> {
    if installation.kind == InstallerKind::Deb || release.kind == InstallerKind::Deb {
        return Err(
            "Debian packages must be authorized before quit and restarted with the restart-only helper"
                .into(),
        );
    }
    if !artifact.is_file() || !artifact.is_absolute() {
        return Err("downloaded update artifact is not an absolute file".into());
    }
    let digest = release.digest.strip_prefix("sha256:").unwrap_or_default();
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("downloaded update has an invalid digest".into());
    }
    let helper = copy_helper_outside_installation(installation)?;
    let pid = portable_wrapper_pid()
        .unwrap_or_else(std::process::id)
        .to_string();
    Command::new(&helper)
        .arg("--artifact")
        .arg(artifact)
        .arg("--target")
        .arg(installation.target_path())
        .arg("--kind")
        .arg(installation.kind.marker())
        .arg("--version")
        .arg(release.version.to_string())
        .arg("--digest")
        .arg(digest)
        .arg("--pid")
        .arg(pid)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start updater helper: {error}"))
}

/// Copy and start the small post-install helper only after apt has completed.
/// The helper's sole job in this mode is to wait for this process to finish
/// its graceful shutdown and relaunch the Debian launcher.  It receives no
/// artifact or digest and cannot perform a privileged installation itself.
pub(crate) fn spawn_deb_restart_helper(
    installation: &Installation,
    version: &Version,
) -> Result<(), String> {
    if installation.kind != InstallerKind::Deb {
        return Err("only Debian installations may use the restart-only helper".into());
    }
    if !installation.target_path().is_absolute() || !installation.target_path().is_file() {
        return Err("the Debian launcher is no longer available for restart".into());
    }
    let helper = copy_helper_outside_installation(installation)?;
    let pid = portable_wrapper_pid()
        .unwrap_or_else(std::process::id)
        .to_string();
    Command::new(&helper)
        .arg("--restart-only")
        .arg("--target")
        .arg(installation.target_path())
        .arg("--version")
        .arg(version.to_string())
        .arg("--pid")
        .arg(pid)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start restart helper: {error}"))
}

fn copy_helper_outside_installation(installation: &Installation) -> Result<PathBuf, String> {
    let directory = installation
        .update_dir()
        .ok_or_else(|| "no per-user update directory is available".to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let helper_name = if cfg!(windows) {
        format!("terminaltiler-updater-{}.exe", std::process::id())
    } else {
        format!("terminaltiler-updater-{}", std::process::id())
    };
    let destination = directory.join(helper_name);
    fs::copy(&installation.helper, &destination).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(&installation.helper) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(
            &destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        );
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;

    fn installation(kind: InstallerKind) -> Installation {
        Installation {
            kind,
            target: PathBuf::from("/tmp/terminaltiler"),
            helper: PathBuf::from("/tmp/terminaltiler-updater"),
        }
    }

    fn release_json(kind: InstallerKind, digest: &str, version: &str) -> String {
        let version = Version::parse(version).unwrap();
        let name = kind.asset_name(&version);
        format!(
            r#"{{"tag_name":"v{version}","name":"TerminalTiler {version}","body":"notes","draft":false,"prerelease":false,"assets":[{{"name":"{name}","size":42,"browser_download_url":"{RELEASE_DOWNLOAD_ROOT}v{version}/{name}","url":"https://api.github.com/repos/Zethrus/TerminalTiler/releases/assets/1","digest":"{digest}"}}]}}"#
        )
    }

    fn release(kind: InstallerKind) -> ReleaseInfo {
        ReleaseInfo {
            version: Version::parse("1.2.3").unwrap(),
            tag: "v1.2.3".into(),
            title: "TerminalTiler 1.2.3".into(),
            notes: String::new(),
            asset_name: kind.asset_name(&Version::parse("1.2.3").unwrap()),
            download_url: "https://example.invalid/fixture".into(),
            digest: format!("sha256:{}", "0".repeat(64)),
            size: 1,
            kind,
        }
    }

    #[test]
    fn compares_versions_without_accepting_equal_or_downgrade() {
        let install = installation(InstallerKind::AppImage);
        let json = release_json(
            InstallerKind::AppImage,
            &format!("sha256:{}", "a".repeat(64)),
            "0.3.24",
        );
        assert!(select_release("0.3.23", &install, &json).unwrap().is_some());
        assert!(select_release("0.3.24", &install, &json).unwrap().is_none());
        assert!(select_release("0.3.25", &install, &json).unwrap().is_none());
    }

    #[test]
    fn accepts_published_releases_and_rejects_drafts_or_prereleases() {
        let install = installation(InstallerKind::Deb);
        let json = release_json(
            InstallerKind::Deb,
            &format!("sha256:{}", "a".repeat(64)),
            "0.3.24",
        );
        // GitHub's published releases currently report `immutable: false`.
        // That informational flag is not a provenance guarantee, whereas the
        // selected asset is still bound to its expected API URL and SHA-256.
        let published_release = json.replacen(
            "\"prerelease\":false",
            "\"immutable\":false,\"prerelease\":false",
            1,
        );
        assert!(
            select_release("0.3.23", &install, &published_release)
                .expect("a published release with verified asset metadata is accepted")
                .is_some()
        );

        let prerelease = json.replace("\"prerelease\":false", "\"prerelease\":true");
        assert_eq!(
            select_release("0.3.23", &install, &prerelease),
            Err(SelectionError::DraftOrPrerelease)
        );

        let draft = json.replace("\"draft\":false", "\"draft\":true");
        assert_eq!(
            select_release("0.3.23", &install, &draft),
            Err(SelectionError::DraftOrPrerelease)
        );

        let no_v_tag = release_json(
            InstallerKind::Deb,
            &format!("sha256:{}", "a".repeat(64)),
            "0.3.24",
        )
        .replace("\"tag_name\":\"v0.3.24\"", "\"tag_name\":\"0.3.24\"");
        assert_eq!(
            select_release("0.3.23", &install, &no_v_tag),
            Err(SelectionError::InvalidReleaseTag)
        );
    }

    #[test]
    fn requires_exact_asset_url_size_and_digest() {
        let install = installation(InstallerKind::Nsis);
        let valid = release_json(
            InstallerKind::Nsis,
            &format!("sha256:{}", "a".repeat(64)),
            "0.3.24",
        );
        assert!(select_release("0.3.23", &install, &valid).is_ok());
        for (needle, replacement) in [
            (
                "github.com/Zethrus/TerminalTiler/releases/download",
                "example.com/releases/download",
            ),
            ("\"size\":42", "\"size\":0"),
            ("sha256:", "md5:"),
        ] {
            let invalid = valid.replace(needle, replacement);
            assert_eq!(
                select_release("0.3.23", &install, &invalid),
                Err(SelectionError::InvalidAssetMetadata)
            );
        }

        let missing_api_url = valid.replace(
            "\"url\":\"https://api.github.com/repos/Zethrus/TerminalTiler/releases/assets/1\"",
            "\"url\":\"\"",
        );
        assert_eq!(
            select_release("0.3.23", &install, &missing_api_url),
            Err(SelectionError::InvalidAssetMetadata)
        );
    }

    #[test]
    fn asset_names_match_supported_installers() {
        let version = Version::parse("v1.2.3").unwrap();
        assert_eq!(
            InstallerKind::AppImage.asset_name(&version),
            "TerminalTiler-1.2.3-x86_64.AppImage"
        );
        assert_eq!(
            InstallerKind::Deb.asset_name(&version),
            "terminaltiler_1.2.3_amd64.deb"
        );
        assert_eq!(
            InstallerKind::Nsis.asset_name(&version),
            "TerminalTiler-setup-1.2.3-x86_64.exe"
        );
        assert_eq!(
            InstallerKind::Msi.asset_name(&version),
            "TerminalTiler-setup-1.2.3-x86_64.msi"
        );
        assert_eq!(
            InstallerKind::PortableExe.asset_name(&version),
            "TerminalTiler-1.2.3-portable-x86_64.exe"
        );
    }

    #[test]
    fn debian_installer_uses_a_root_owned_helper_with_bound_metadata() {
        let artifact = Path::new("/tmp/terminaltiler_1.2.3_amd64.deb");
        let mut release = release(InstallerKind::Deb);
        release.size = 42;
        release.digest = format!("sha256:{}", "a".repeat(64));
        let command = deb_install_command(&release, artifact);
        assert_eq!(command.get_program(), std::ffi::OsStr::new(PKEXEC_PATH));
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            vec![
                DEBIAN_PRIVILEGED_UPDATER_PATH.to_string(),
                "--install-deb".to_string(),
                "--artifact".to_string(),
                artifact.display().to_string(),
                "--version".to_string(),
                "1.2.3".to_string(),
                "--size".to_string(),
                "42".to_string(),
                "--digest".to_string(),
                "a".repeat(64),
            ]
        );
    }

    #[test]
    fn classifies_polkit_exit_codes_separately_from_apt_failures() {
        assert!(matches!(
            classify_deb_install_failure(Some(126), "dismissed".into()),
            DebInstallFailure::AuthorizationPromptUnavailableOrDismissed { .. }
        ));
        assert!(matches!(
            classify_deb_install_failure(Some(127), "denied".into()),
            DebInstallFailure::AuthorizationFailed { .. }
        ));
        assert!(matches!(
            classify_deb_install_failure(Some(100), "apt failed".into()),
            DebInstallFailure::PackageManagerFailed {
                exit_code: Some(100),
                ..
            }
        ));
    }

    #[test]
    fn bounds_but_drains_package_manager_diagnostics() {
        let diagnostic = bounded_diagnostic(Cursor::new(b"0123456789"), 4).unwrap();
        assert_eq!(diagnostic, "0123\n[diagnostic output truncated]");

        let empty = bounded_diagnostic(Cursor::new(b""), 4).unwrap();
        assert_eq!(empty, "no diagnostic output was provided");
    }

    #[test]
    fn rechecks_the_debian_payload_before_requesting_privilege() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-update-deb-recheck-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut release = release(InstallerKind::Deb);
        let artifact = root.join(&release.asset_name);
        let payload = b"verified Debian payload";
        fs::write(&artifact, payload).unwrap();
        release.size = payload.len() as u64;
        release.digest = format!("sha256:{:x}", Sha256::digest(payload));

        verify_deb_artifact(&release, &artifact).expect("the original payload verifies");
        fs::write(&artifact, b"tampered Debian payload").unwrap();
        assert!(verify_deb_artifact(&release, &artifact).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn debian_install_request_is_queued_without_a_shutdown_command() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-update-install-request-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("terminaltiler_1.2.3_amd64.deb");
        fs::write(&artifact, b"verified fixture").unwrap();

        let (command_tx, command_rx) = mpsc::channel();
        let service = UpdateService {
            inner: Arc::new(UpdateServiceInner {
                command_tx,
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
        };
        service
            .install_deb(release(InstallerKind::Deb), artifact.clone())
            .expect("the verified package should be queued asynchronously");
        match command_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(UpdateCommand::InstallDeb {
                release,
                artifact: queued,
            }) => {
                assert_eq!(release.kind, InstallerKind::Deb);
                assert_eq!(queued, artifact);
            }
            _ => panic!("expected an in-session Debian install command"),
        }
        assert!(matches!(
            command_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        drop(service);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn streamed_download_verifies_size_digest_and_cleans_partial_files() {
        let root =
            std::env::temp_dir().join(format!("terminaltiler-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let part = root.join("artifact.part");
        let destination = root.join("artifact");
        let bytes = b"verified update payload";
        let digest = format!("sha256:{:x}", Sha256::digest(bytes));
        let cancelled = AtomicBool::new(false);
        let path = write_verified_stream(
            Cursor::new(bytes),
            &part,
            &destination,
            bytes.len() as u64,
            &digest,
            &cancelled,
        )
        .unwrap();
        assert_eq!(path, destination);
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);

        let mismatch_part = root.join("mismatch.part");
        let mismatch_destination = root.join("mismatch");
        let error = write_verified_stream(
            Cursor::new(bytes),
            &mismatch_part,
            &mismatch_destination,
            bytes.len() as u64 + 1,
            &digest,
            &cancelled,
        )
        .unwrap_err();
        assert!(matches!(error, DownloadError::TooLarge));
        assert!(!mismatch_part.exists());
        assert!(!mismatch_destination.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn streamed_download_honors_cancellation() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-update-cancel-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let part = root.join("artifact.part");
        let destination = root.join("artifact");
        let cancelled = AtomicBool::new(true);
        let error = write_verified_stream(
            Cursor::new(b"payload"),
            &part,
            &destination,
            7,
            &format!("sha256:{:x}", Sha256::digest(b"payload")),
            &cancelled,
        )
        .unwrap_err();
        assert!(matches!(error, DownloadError::Cancelled));
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn streamed_progress_is_bounded_monotonic_and_percentage_throttled() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-update-progress-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bytes = vec![7u8; MAX_READ_CHUNK + 10];
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        let events = std::cell::RefCell::new(Vec::new());
        let part = root.join("artifact.part");
        let destination = root.join("artifact");
        let cancelled = AtomicBool::new(false);
        write_verified_stream_with_progress(
            Cursor::new(&bytes),
            VerifiedStreamTarget {
                part_path: &part,
                final_path: &destination,
                expected_size: bytes.len() as u64,
                expected_digest: &digest,
                cancelled: &cancelled,
            },
            || events.borrow_mut().push(None),
            |downloaded, total| events.borrow_mut().push(Some((downloaded, total))),
        )
        .unwrap();
        let events = events.into_inner();
        assert_eq!(
            events.last(),
            Some(&None),
            "verification follows streaming writes"
        );
        let events = events.into_iter().flatten().collect::<Vec<_>>();
        assert!(!events.is_empty());
        assert!(events.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            events
                .iter()
                .all(|(downloaded, total)| *downloaded <= *total)
        );
        assert_eq!(
            events.last(),
            Some(&(bytes.len() as u64, bytes.len() as u64))
        );
        assert!(
            events.len() <= 101,
            "percentage throttling bounds event volume"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn streamed_download_rejects_digest_mismatch_and_cleans_up() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-update-digest-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let part = root.join("artifact.part");
        let destination = root.join("artifact");
        let cancelled = AtomicBool::new(false);
        let error = write_verified_stream(
            Cursor::new(b"payload"),
            &part,
            &destination,
            7,
            &format!("sha256:{}", "0".repeat(64)),
            &cancelled,
        )
        .unwrap_err();
        assert!(matches!(error, DownloadError::DigestMismatch));
        assert!(!part.exists());
        assert!(!destination.exists());
        let digest = format!("sha256:{:x}", Sha256::digest(b"payload"));
        write_verified_stream(
            Cursor::new(b"payload"),
            &part,
            &destination,
            7,
            &digest,
            &cancelled,
        )
        .expect("a retry should be able to recreate the part file");
        assert_eq!(std::fs::read(&destination).unwrap(), b"payload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dropping_a_modal_service_handle_does_not_stop_or_disable_the_worker() {
        let (command_tx, command_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let service = UpdateService {
            inner: Arc::new(UpdateServiceInner {
                command_tx,
                cancelled: cancelled.clone(),
            }),
        };

        let modal_service = service.clone();
        drop(modal_service);

        assert!(matches!(
            command_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(!cancelled.load(Ordering::Relaxed));

        service
            .download(release(InstallerKind::AppImage))
            .expect("the update worker should accept a download command");
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)),
            Ok(UpdateCommand::Download(_))
        ));

        drop(service);

        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)),
            Ok(UpdateCommand::Stop)
        ));
        assert!(cancelled.load(Ordering::Relaxed));
    }
}
