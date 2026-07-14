//! Small, dependency-light updater helper.
//!
//! The running application copies no code into its installation and never
//! replaces itself while loaded. It verifies and installs deferred non-Debian
//! artifacts, or (after an in-session Debian transaction) only restarts the
//! packaged Debian launcher.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const APT_GET_PATH: &str = "/usr/bin/apt-get";
const DEBIAN_STAGING_DIR: &str = "/var/lib/terminaltiler/updates";

#[derive(Clone, Copy, Debug)]
enum Kind {
    AppImage,
    Deb,
    Nsis,
    Msi,
    PortableExe,
}

impl Kind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "appimage" => Self::AppImage,
            "deb" => Self::Deb,
            "nsis" => Self::Nsis,
            "msi" => Self::Msi,
            "portable-exe" => Self::PortableExe,
            _ => return None,
        })
    }
}

struct InstallArgs {
    artifact: PathBuf,
    target: PathBuf,
    kind: Kind,
    version: String,
    digest: String,
    pid: u32,
}

struct RestartArgs {
    target: PathBuf,
    version: String,
    pid: u32,
}

struct DebInstallArgs {
    artifact: PathBuf,
    version: String,
    size: u64,
    digest: String,
}

enum Args {
    Install(InstallArgs),
    RestartOnly(RestartArgs),
    InstallDeb(DebInstallArgs),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("TerminalTiler updater failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(env::args_os().skip(1))?;
    match args {
        Args::Install(args) => run_install(args),
        Args::RestartOnly(args) => run_restart_only(args),
        Args::InstallDeb(args) => run_install_deb(args),
    }
}

fn run_install(args: InstallArgs) -> Result<(), String> {
    if !args.artifact.is_absolute()
        || !args.target.is_absolute()
        || matches!(args.kind, Kind::Deb)
        || args.digest.len() != 64
        || !args.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !valid_version(&args.version)
        || !valid_artifact_extension(args.kind, &args.artifact)
        || !valid_artifact_name(args.kind, &args.version, &args.artifact)
        || !valid_target_kind(args.kind, &args.target)
    {
        return Err("invalid updater arguments".into());
    }
    let result_path = result_path();
    if !args.artifact.is_file() {
        let error = "update artifact does not exist".to_string();
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        relaunch_if_application_stopped(&args.target, args.pid);
        return Err(error);
    }
    if !args.target.is_file() {
        let error = "installed application target does not exist".to_string();
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        return Err(error);
    }
    if let Err(error) = verify_digest(&args.artifact, &args.digest) {
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        relaunch_if_application_stopped(&args.target, args.pid);
        return Err(error);
    }
    // A user can decline the existing active-session quit confirmation after
    // the helper has started waiting.  Do not relaunch a second copy when the
    // original process is still alive; record the cancellation and leave it
    // untouched instead.
    if let Err(error) = wait_for_process(args.pid) {
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        return Err(error);
    }
    let outcome = (|| {
        install(&args)?;
        write_result(&result_path, &args.version, true, None).map_err(|error| error.to_string())?;
        relaunch(&args.target)?;
        Ok::<(), String>(())
    })();

    if let Err(error) = outcome {
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        // A failed installer must not strand the user.  The existing target
        // is left untouched for atomic replacement kinds and is relaunched
        // when possible for installer kinds.
        let _ = relaunch(&args.target);
        return Err(error);
    }
    Ok(())
}

/// Debian packages are installed before the app exits, while its Polkit
/// session is still associated with the active desktop. The helper only waits
/// for graceful shutdown and starts the system launcher after that succeeds.
fn run_restart_only(args: RestartArgs) -> Result<(), String> {
    if !args.target.is_absolute() || !valid_debian_launcher(&args.target) || !args.target.is_file()
    {
        return Err("invalid restart-only updater arguments".into());
    }
    if !valid_version(&args.version) {
        return Err("invalid restart-only updater arguments".into());
    }

    // A user can dismiss the normal quit confirmation because workspace
    // processes are still active.  Keep this restart intent alive until the
    // original application actually exits instead of timing out after 30
    // seconds and silently losing a successful update.
    let result_path = result_path();
    let outcome = (|| {
        wait_for_process_exit(args.pid);
        relaunch(&args.target)?;
        write_result(&result_path, &args.version, true, None).map_err(|error| error.to_string())
    })();
    if let Err(error) = outcome {
        let _ = write_result(&result_path, &args.version, false, Some(&error));
        return Err(error);
    }
    Ok(())
}

/// Run the Debian package transaction after crossing the PolicyKit boundary.
/// The source artifact is intentionally untrusted: it is copied and verified
/// in a root-owned staging directory before apt ever receives a pathname.
fn run_install_deb(args: DebInstallArgs) -> Result<(), String> {
    if !is_root_process() {
        return Err("Debian installation must be run through PolicyKit".into());
    }

    if !args.artifact.is_absolute()
        || !args.artifact.is_file()
        || !valid_version(&args.version)
        || !valid_artifact_extension(Kind::Deb, &args.artifact)
        || !valid_artifact_name(Kind::Deb, &args.version, &args.artifact)
        || args.size == 0
        || args.size >= MAX_ASSET_BYTES
        || args.digest.len() != 64
        || !args.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid Debian installer arguments".into());
    }

    let staging_directory = secure_debian_staging_dir()?;
    let staged =
        copy_verified_deb_artifact(&args.artifact, args.size, &args.digest, &staging_directory)?;
    let outcome = Command::new(APT_GET_PATH)
        .arg("install")
        .arg("--yes")
        .arg(&staged)
        .status()
        .map_err(|error| format!("could not start apt-get: {error}"))
        .and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or_else(|| format!("apt-get exited with {}", status.code().unwrap_or(1)))
        });
    let _ = fs::remove_file(&staged);
    outcome
}

#[cfg(target_os = "linux")]
fn is_root_process() -> bool {
    // The process is launched by pkexec; use the effective UID because that is
    // the identity that will open the private staging directory and invoke apt.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(target_os = "linux"))]
fn is_root_process() -> bool {
    false
}

fn copy_verified_deb_artifact(
    source: &Path,
    expected_size: u64,
    expected_digest: &str,
    staging_directory: &Path,
) -> Result<PathBuf, String> {
    let destination = staging_directory.join(format!(
        "terminaltiler-update-{}-{}.deb",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    let mut input = File::open(source).map_err(|error| error.to_string())?;
    let mut output = create_private_staging_file(&destination)?;
    let copied = (|| {
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 1024 * 1024];
        loop {
            let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > expected_size || total >= MAX_ASSET_BYTES {
                return Err("Debian package size changed while it was being secured".into());
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            hash.update(&buffer[..read]);
        }
        if total != expected_size {
            return Err("Debian package size changed while it was being secured".into());
        }
        let actual = format!("{:x}", hash.finalize());
        if !actual.eq_ignore_ascii_case(expected_digest) {
            return Err("Debian package digest changed while it was being secured".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = copied {
        drop(output);
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    Ok(destination)
}

#[cfg(unix)]
fn create_private_staging_file(path: &Path) -> Result<File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn create_private_staging_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())
}

fn secure_debian_staging_dir() -> Result<PathBuf, String> {
    let directory = PathBuf::from(DEBIAN_STAGING_DIR);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let metadata = fs::metadata(&directory).map_err(|error| error.to_string())?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err("Debian update staging directory is not root-owned and private".into());
        }
    }
    Ok(directory)
}

fn valid_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let valid = (0..3).all(|_| {
        parts.next().is_some_and(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
    });
    valid && parts.next().is_none()
}

fn valid_artifact_extension(kind: Kind, artifact: &Path) -> bool {
    let extension = artifact.extension().and_then(|value| value.to_str());
    match kind {
        Kind::AppImage => extension == Some("AppImage"),
        Kind::Deb => extension == Some("deb"),
        Kind::Nsis | Kind::PortableExe => extension == Some("exe"),
        Kind::Msi => extension == Some("msi"),
    }
}

fn valid_artifact_name(kind: Kind, version: &str, artifact: &Path) -> bool {
    let expected = match kind {
        Kind::AppImage => format!("TerminalTiler-{version}-x86_64.AppImage"),
        Kind::Deb => format!("terminaltiler_{version}_amd64.deb"),
        Kind::Nsis => format!("TerminalTiler-setup-{version}-x86_64.exe"),
        Kind::Msi => format!("TerminalTiler-setup-{version}-x86_64.msi"),
        Kind::PortableExe => format!("TerminalTiler-{version}-portable-x86_64.exe"),
    };
    artifact.file_name().and_then(|name| name.to_str()) == Some(expected.as_str())
}

fn valid_target_kind(kind: Kind, target: &Path) -> bool {
    let extension = target.extension().and_then(|value| value.to_str());
    match kind {
        Kind::AppImage => extension.is_some_and(|value| value.eq_ignore_ascii_case("AppImage")),
        Kind::Deb => target
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == "terminaltiler" || value == "terminaltiler-bin"),
        Kind::Nsis | Kind::Msi | Kind::PortableExe => {
            extension.is_some_and(|value| value.eq_ignore_ascii_case("exe"))
        }
    }
}

fn valid_debian_launcher(target: &Path) -> bool {
    target
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == "terminaltiler")
}

fn parse_args<I>(mut args: I) -> Result<Args, String>
where
    I: Iterator<Item = std::ffi::OsString>,
{
    let mut artifact = None;
    let mut target = None;
    let mut kind = None;
    let mut version = None;
    let mut digest = None;
    let mut size = None;
    let mut pid = None;
    let mut restart_only = false;
    let mut install_deb = false;
    while let Some(flag) = args.next() {
        let flag = flag.to_string_lossy();
        match flag.as_ref() {
            "--restart-only" => {
                if restart_only {
                    return Err("duplicate --restart-only".into());
                }
                restart_only = true;
            }
            "--install-deb" => {
                if install_deb {
                    return Err("duplicate --install-deb".into());
                }
                install_deb = true;
            }
            "--artifact" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if artifact.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --artifact".into());
                }
            }
            "--target" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if target.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --target".into());
                }
            }
            "--kind" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                let parsed =
                    Kind::parse(&value.to_string_lossy()).ok_or("missing or invalid --kind")?;
                if kind.replace(parsed).is_some() {
                    return Err("duplicate --kind".into());
                }
            }
            "--version" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if version
                    .replace(value.to_string_lossy().into_owned())
                    .is_some()
                {
                    return Err("duplicate --version".into());
                }
            }
            "--digest" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if digest
                    .replace(value.to_string_lossy().to_ascii_lowercase())
                    .is_some()
                {
                    return Err("duplicate --digest".into());
                }
            }
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if size
                    .replace(
                        value
                            .to_string_lossy()
                            .parse()
                            .map_err(|_| "invalid size")?,
                    )
                    .is_some()
                {
                    return Err("duplicate --size".into());
                }
            }
            "--pid" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?;
                if pid
                    .replace(value.to_string_lossy().parse().map_err(|_| "invalid pid")?)
                    .is_some()
                {
                    return Err("duplicate --pid".into());
                }
            }
            _ => return Err(format!("unknown updater argument {flag}")),
        }
    }
    if restart_only && install_deb {
        return Err("restart-only and Debian install modes cannot be combined".into());
    }
    if restart_only {
        if artifact.is_some() || kind.is_some() || digest.is_some() || size.is_some() {
            return Err("restart-only helper does not accept installer arguments".into());
        }
        return Ok(Args::RestartOnly(RestartArgs {
            target: target.ok_or("missing --target")?,
            version: version.ok_or("missing --version")?,
            pid: pid.ok_or("missing --pid")?,
        }));
    }
    if install_deb {
        if target.is_some() || kind.is_some() || pid.is_some() {
            return Err("Debian installer does not accept application restart arguments".into());
        }
        return Ok(Args::InstallDeb(DebInstallArgs {
            artifact: artifact.ok_or("missing --artifact")?,
            version: version.ok_or("missing --version")?,
            size: size.ok_or("missing --size")?,
            digest: digest.ok_or("missing --digest")?,
        }));
    }
    Ok(Args::Install(InstallArgs {
        artifact: artifact.ok_or("missing --artifact")?,
        target: target.ok_or("missing --target")?,
        kind: kind.ok_or("missing or invalid --kind")?,
        version: version.ok_or("missing --version")?,
        digest: digest.ok_or("missing --digest")?,
        pid: pid.ok_or("missing --pid")?,
    }))
}

fn verify_digest(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total >= MAX_ASSET_BYTES {
            return Err("update artifact exceeds the safety limit".into());
        }
        hash.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual != expected {
        return Err("update artifact digest mismatch".into());
    }
    Ok(())
}

fn wait_for_process(pid: u32) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Ok(());
    }
    for _ in 0..120 {
        if !process_running(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err("TerminalTiler did not exit after the update was approved".into())
}

fn wait_for_process_exit(pid: u32) {
    if pid == 0 || pid == std::process::id() {
        return;
    }
    while process_running(pid) {
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(unix)]
fn process_running(pid: u32) -> bool {
    let proc_root = Path::new("/proc");
    // Supported Linux installers expose procfs.  If it is unavailable, fail
    // closed and keep waiting rather than replacing a loaded executable.
    !proc_root.is_dir() || proc_root.join(pid.to_string()).is_dir()
}

#[cfg(windows)]
fn process_running(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map(|output| {
            !output.status.success()
                || !String::from_utf8_lossy(&output.stdout).contains("No tasks")
        })
        .unwrap_or(true)
}

fn install(args: &InstallArgs) -> Result<(), String> {
    match args.kind {
        Kind::AppImage | Kind::PortableExe => {
            atomic_replace(&args.artifact, &args.target, &args.digest)
        }
        Kind::Deb => Err("Debian packages must be installed before TerminalTiler exits".into()),
        Kind::Nsis => run_command(Command::new(&args.artifact).arg("/S"), &[0]),
        Kind::Msi => run_command(
            Command::new("msiexec")
                .arg("/i")
                .arg(&args.artifact)
                .arg("/passive")
                .arg("/norestart"),
            &[0, 1641, 3010],
        ),
    }
}

fn run_command(command: &mut Command, accepted: &[i32]) -> Result<(), String> {
    let status = command.status().map_err(|error| error.to_string())?;
    let code = status.code().unwrap_or(1);
    if accepted.contains(&code) {
        Ok(())
    } else {
        Err(format!("installer exited with code {code}"))
    }
}

fn atomic_replace(source: &Path, target: &Path, expected_digest: &str) -> Result<(), String> {
    let parent = target.parent().ok_or("target has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = target.with_extension("terminaltiler-update.part");
    let _ = fs::remove_file(&temp);
    fs::copy(source, &temp).map_err(|error| error.to_string())?;
    // Hash the copied bytes as well as the downloaded source. This closes the
    // small local TOCTOU window between verification and installation.
    if let Err(error) = verify_digest(&temp, expected_digest) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let temp_file = OpenOptions::new()
        .write(true)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    temp_file.sync_all().map_err(|error| error.to_string())?;
    preserve_permissions(target, &temp);

    #[cfg(unix)]
    {
        // Unix rename replaces the destination in one filesystem operation,
        // so readers never observe a missing application between moves.
        if let Err(error) = fs::rename(&temp, target) {
            let _ = fs::remove_file(&temp);
            return Err(error.to_string());
        }
        sync_directory(parent);
    }

    #[cfg(not(unix))]
    if target.exists() {
        let backup = target.with_extension("terminaltiler-update.old");
        let _ = fs::remove_file(&backup);
        fs::rename(target, &backup).map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    if let Err(error) = fs::rename(&temp, target) {
        let backup = target.with_extension("terminaltiler-update.old");
        if backup.exists() && !target.exists() {
            let _ = fs::rename(backup, target);
        }
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    #[cfg(not(unix))]
    let _ = fs::remove_file(target.with_extension("terminaltiler-update.old"));
    #[cfg(not(unix))]
    sync_directory(parent);
    Ok(())
}

#[cfg(unix)]
fn preserve_permissions(source: &Path, destination: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(source) {
        let _ = fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        );
    }
}

#[cfg(not(unix))]
fn preserve_permissions(_source: &Path, _destination: &Path) {}

#[cfg(unix)]
fn sync_directory(path: &Path) {
    let _ = File::open(path).and_then(|directory| directory.sync_all());
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}

fn result_path() -> PathBuf {
    let root = env::var_os("TERMINALTILER_PROFILE_ROOT")
        .map(PathBuf::from)
        .map(|path| path.join("state"))
        .or_else(|| {
            directories::ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
                .and_then(|dirs| dirs.state_dir().map(PathBuf::from))
        })
        .unwrap_or_else(|| env::temp_dir().join("terminaltiler"));
    root.join("update-result.json")
}

fn write_result(path: &Path, version: &str, success: bool, error: Option<&str>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::json!({
        "version": version,
        "success": success,
        "error": error,
    });
    let temp = path.with_extension("json.part");
    let _ = fs::remove_file(&temp);
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let bytes = serde_json::to_vec_pretty(&content).expect("result is serializable");
    std::io::Write::write_all(&mut temp_file, &bytes)?;
    temp_file.sync_all()?;
    #[cfg(not(unix))]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        sync_directory(parent);
    }
    Ok(())
}

fn relaunch(target: &Path) -> Result<(), String> {
    if target.is_file() {
        Command::new(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        Err("installed application target is missing".into())
    }
}

fn relaunch_if_application_stopped(target: &Path, pid: u32) {
    if pid == 0 || pid == std::process::id() || !process_running(pid) {
        let _ = relaunch(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn parses_only_the_explicit_helper_arguments() {
        let args = vec![
            OsString::from("--artifact"),
            OsString::from("/tmp/update.AppImage"),
            OsString::from("--target"),
            OsString::from("/opt/terminaltiler"),
            OsString::from("--kind"),
            OsString::from("appimage"),
            OsString::from("--version"),
            OsString::from("1.2.3"),
            OsString::from("--digest"),
            OsString::from("a".repeat(64)),
            OsString::from("--pid"),
            OsString::from("42"),
        ];
        let parsed = parse_args(args.into_iter()).expect("valid helper arguments");
        let Args::Install(parsed) = parsed else {
            panic!("expected install arguments");
        };
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.pid, 42);
        assert!(matches!(parsed.kind, Kind::AppImage));
    }

    #[test]
    fn parses_the_restart_only_contract_without_an_artifact() {
        let args = vec![
            OsString::from("--restart-only"),
            OsString::from("--target"),
            OsString::from("/usr/bin/terminaltiler"),
            OsString::from("--version"),
            OsString::from("1.2.3"),
            OsString::from("--pid"),
            OsString::from("42"),
        ];
        let parsed = parse_args(args.into_iter()).expect("valid restart-only arguments");
        let Args::RestartOnly(parsed) = parsed else {
            panic!("expected restart-only arguments");
        };
        assert_eq!(parsed.target, PathBuf::from("/usr/bin/terminaltiler"));
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.pid, 42);
    }

    #[test]
    fn parses_the_privileged_debian_installer_contract() {
        let args = vec![
            OsString::from("--install-deb"),
            OsString::from("--artifact"),
            OsString::from("/tmp/terminaltiler_1.2.3_amd64.deb"),
            OsString::from("--version"),
            OsString::from("1.2.3"),
            OsString::from("--size"),
            OsString::from("42"),
            OsString::from("--digest"),
            OsString::from("a".repeat(64)),
        ];
        let parsed = parse_args(args.into_iter()).expect("valid Debian installer arguments");
        let Args::InstallDeb(parsed) = parsed else {
            panic!("expected Debian installer arguments");
        };
        assert_eq!(parsed.size, 42);
        assert_eq!(parsed.version, "1.2.3");
    }

    #[test]
    fn securing_a_debian_artifact_copies_and_verifies_the_root_input() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-updater-secure-deb-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("terminaltiler_1.2.3_amd64.deb");
        let staging = root.join("staging");
        fs::create_dir(&staging).unwrap();
        let payload = b"verified Debian package bytes";
        fs::write(&source, payload).unwrap();
        let digest = format!("{:x}", Sha256::digest(payload));

        let staged = copy_verified_deb_artifact(&source, payload.len() as u64, &digest, &staging)
            .expect("verified bytes should be copied into the private staging directory");
        assert_ne!(staged, source);
        assert_eq!(fs::read(staged).unwrap(), payload);

        assert!(
            copy_verified_deb_artifact(&source, payload.len() as u64, &"0".repeat(64), &staging)
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_only_mode_rejects_installation_arguments() {
        let args = vec![
            OsString::from("--restart-only"),
            OsString::from("--target"),
            OsString::from("/usr/bin/terminaltiler"),
            OsString::from("--version"),
            OsString::from("1.2.3"),
            OsString::from("--pid"),
            OsString::from("42"),
            OsString::from("--artifact"),
            OsString::from("/tmp/terminaltiler_1.2.3_amd64.deb"),
        ];
        assert!(parse_args(args.into_iter()).is_err());
    }

    #[test]
    fn restart_only_mode_allows_only_the_debian_launcher_name() {
        assert!(valid_debian_launcher(Path::new("/usr/bin/terminaltiler")));
        assert!(!valid_debian_launcher(Path::new(
            "/usr/bin/terminaltiler-bin"
        )));
        assert!(!valid_debian_launcher(Path::new("/usr/bin/sh")));
    }

    #[test]
    fn rejects_non_canonical_versions() {
        assert!(valid_version("1.2.3"));
        assert!(!valid_version("01.2.3"));
        assert!(!valid_version("1.2.3.4"));
        assert!(!valid_version("1.2"));
    }

    #[test]
    fn rejects_duplicate_arguments() {
        let args = vec![
            OsString::from("--artifact"),
            OsString::from("/tmp/one"),
            OsString::from("--artifact"),
            OsString::from("/tmp/two"),
        ];
        assert!(parse_args(args.into_iter()).is_err());
    }

    #[test]
    fn atomically_replaces_a_target_fixture() {
        let root = env::temp_dir().join(format!(
            "terminaltiler-updater-replace-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("downloaded");
        let target = root.join("installed");
        fs::write(&source, b"new payload").unwrap();
        fs::write(&target, b"old payload").unwrap();

        let digest = format!("{:x}", Sha256::digest(b"new payload"));
        atomic_replace(&source, &target, &digest).expect("fixture replacement should succeed");
        assert_eq!(fs::read(&target).unwrap(), b"new payload");
        assert!(!target.with_extension("terminaltiler-update.old").exists());

        fs::write(&source, b"tampered payload").unwrap();
        let error = atomic_replace(&source, &target, &digest).unwrap_err();
        assert!(error.contains("digest mismatch"));
        assert_eq!(fs::read(&target).unwrap(), b"new payload");
        assert!(!target.with_extension("terminaltiler-update.part").exists());
        let _ = fs::remove_dir_all(root);
    }
}
