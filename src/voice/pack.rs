use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{app_paths, logging};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePackManifest {
    pub id: String,
    pub version: String,
    pub engine_executable: String,
    pub model_path: String,
    pub archive_url: String,
    pub archive_sha256: String,
    #[serde(default = "default_parakeet_model_name")]
    pub model_name: String,
    #[serde(default = "default_parakeet_streaming_model_name")]
    pub streaming_model_name: String,
    #[serde(default)]
    pub python_requirements: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePackHealth {
    Missing,
    Ready {
        engine_path: PathBuf,
        model_path: PathBuf,
    },
    Broken(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePackError {
    Io(String),
    CommandFailed {
        command: String,
        status: String,
        log_path: Option<String>,
        output_tail: String,
    },
    ChecksumMismatch {
        expected: String,
        actual: String,
    },
    InvalidManifest(String),
    PythonUnavailable(String),
}

impl From<io::Error> for VoicePackError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl std::fmt::Display for VoicePackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "{message}"),
            Self::CommandFailed {
                status,
                log_path,
                output_tail,
                ..
            } => {
                write!(formatter, "Voice pack command failed ({status})")?;
                if let Some(path) = log_path {
                    write!(formatter, "; see {path}")?;
                }
                if !output_tail.trim().is_empty() {
                    write!(formatter, "; last output: {}", output_tail.trim())?;
                }
                Ok(())
            }
            Self::ChecksumMismatch { expected, actual } => write!(
                formatter,
                "voice pack checksum mismatch: expected {expected}, got {actual}"
            ),
            Self::InvalidManifest(message) => {
                write!(formatter, "invalid voice pack manifest: {message}")
            }
            Self::PythonUnavailable(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for VoicePackError {}

impl VoicePackError {
    pub fn user_message(&self) -> String {
        match self {
            Self::CommandFailed { log_path, .. } => match log_path {
                Some(path) => format!("Voice pack setup failed; see {path}"),
                None => "Voice pack setup failed; check the application logs".into(),
            },
            Self::PythonUnavailable(message) => message.clone(),
            _ => self.to_string(),
        }
    }
}

const BUILTIN_PARAKEET_MANIFEST: &str =
    include_str!("../../resources/voice/parakeet/manifest.toml");
const BUILTIN_PARAKEET_ENGINE: &str =
    include_str!("../../resources/voice/parakeet/parakeet_engine.py");
const VOICE_PACK_INSTALL_LOG_FILE: &str = "voice-pack-install.log";
const COMMAND_OUTPUT_TAIL_LIMIT: usize = 8 * 1024;

pub fn default_parakeet_model_name() -> String {
    "nvidia/parakeet-tdt-0.6b-v2".into()
}

pub fn default_parakeet_streaming_model_name() -> String {
    "nvidia/parakeet-ctc-0.6b".into()
}

pub fn builtin_parakeet_manifest() -> VoicePackManifest {
    toml::from_str(BUILTIN_PARAKEET_MANIFEST)
        .expect("bundled Parakeet voice manifest must be valid TOML")
}

pub fn install_builtin_parakeet_pack(root: &Path) -> Result<VoicePackManifest, VoicePackError> {
    let manifest = builtin_parakeet_manifest();
    let pack_root = root.join(&manifest.id).join(&manifest.version);
    write_builtin_parakeet_pack_assets(&pack_root, &manifest)?;
    Ok(manifest)
}

pub fn refresh_builtin_parakeet_pack_assets(
    root: &Path,
) -> Result<Option<VoicePackManifest>, VoicePackError> {
    let manifest = builtin_parakeet_manifest();
    let pack_root = pack_root(root, &manifest);
    if !pack_root.exists() {
        return Ok(None);
    }
    write_builtin_parakeet_pack_assets(&pack_root, &manifest)?;
    Ok(Some(manifest))
}

fn write_builtin_parakeet_pack_assets(
    pack_root: &Path,
    manifest: &VoicePackManifest,
) -> Result<(), VoicePackError> {
    fs::create_dir_all(pack_root)?;
    let engine_path = pack_root.join(&manifest.engine_executable);
    if let Some(parent) = engine_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(engine_path, BUILTIN_PARAKEET_ENGINE)?;
    fs::create_dir_all(pack_root.join(&manifest.model_path))?;
    fs::write(pack_root.join("manifest.toml"), BUILTIN_PARAKEET_MANIFEST)?;
    if !manifest.python_requirements.is_empty() {
        fs::write(
            pack_root.join("requirements.txt"),
            format!("{}\n", manifest.python_requirements.join("\n")),
        )?;
    }
    Ok(())
}

pub fn pack_root(root: &Path, manifest: &VoicePackManifest) -> PathBuf {
    root.join(&manifest.id).join(&manifest.version)
}

pub fn python_environment_dir(root: &Path, manifest: &VoicePackManifest) -> PathBuf {
    pack_root(root, manifest).join(".venv")
}

pub fn python_environment_executable(root: &Path, manifest: &VoicePackManifest) -> PathBuf {
    let venv = python_environment_dir(root, manifest);
    if cfg!(target_os = "windows") {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

pub fn prepare_python_environment(
    root: &Path,
    manifest: &VoicePackManifest,
) -> Result<bool, VoicePackError> {
    prepare_python_environment_with_progress(root, manifest, |_| {})
}

pub fn prepare_python_environment_with_progress<F>(
    root: &Path,
    manifest: &VoicePackManifest,
    mut progress: F,
) -> Result<bool, VoicePackError>
where
    F: FnMut(u8),
{
    if manifest.python_requirements.is_empty() {
        return Ok(false);
    }
    let pack_root = pack_root(root, manifest);
    let requirements_path = pack_root.join("requirements.txt");
    if !requirements_path.is_file() {
        return Err(VoicePackError::Io(format!(
            "requirements file missing: {}",
            requirements_path.display()
        )));
    }

    let python = python_environment_executable(root, manifest);
    ensure_python_environment_with_working_pip(root, manifest, &mut progress)?;

    run_command_with_progress(
        Command::new(&python)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--upgrade")
            .arg("pip"),
        21,
        34,
        &mut progress,
    )?;
    run_command_with_progress(
        Command::new(&python)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("-r")
            .arg(requirements_path),
        35,
        78,
        &mut progress,
    )?;
    Ok(true)
}

fn ensure_python_environment_with_working_pip<F>(
    root: &Path,
    manifest: &VoicePackManifest,
    progress: &mut F,
) -> Result<(), VoicePackError>
where
    F: FnMut(u8),
{
    let venv_dir = python_environment_dir(root, manifest);
    let python = python_environment_executable(root, manifest);
    if !python.is_file() {
        create_python_environment(root, manifest, 10, 20, progress)?;
    }

    if probe_venv_pip(&python).is_ok() {
        return Ok(());
    }

    logging::info(format!(
        "voice pack Python environment has broken or missing pip; attempting ensurepip repair at {}",
        python.display()
    ));
    let _ = run_command_with_progress(
        Command::new(&python)
            .arg("-m")
            .arg("ensurepip")
            .arg("--upgrade"),
        21,
        24,
        progress,
    );
    if probe_venv_pip(&python).is_ok() {
        return Ok(());
    }

    logging::info(format!(
        "voice pack Python environment pip repair failed; recreating venv at {}",
        venv_dir.display()
    ));
    if venv_dir.exists() {
        fs::remove_dir_all(&venv_dir)?;
    }
    create_python_environment(root, manifest, 10, 24, progress)?;
    match probe_venv_pip(&python) {
        Ok(()) => Ok(()),
        Err(detail) => {
            let ensurepip_error = run_command_with_progress(
                Command::new(&python)
                    .arg("-m")
                    .arg("ensurepip")
                    .arg("--upgrade"),
                25,
                29,
                progress,
            )
            .err();
            if probe_venv_pip(&python).is_ok() {
                return Ok(());
            }
            match ensurepip_error {
                Some(error) => Err(error),
                None => Err(VoicePackError::Io(format!(
                    "Python virtual environment pip is unavailable after repair: {detail}"
                ))),
            }
        }
    }
}

fn create_python_environment<F>(
    root: &Path,
    manifest: &VoicePackManifest,
    start_percent: u8,
    end_percent: u8,
    progress: &mut F,
) -> Result<(), VoicePackError>
where
    F: FnMut(u8),
{
    let mut command = resolve_host_python_command()?;
    command
        .arg("-m")
        .arg("venv")
        .arg(python_environment_dir(root, manifest));
    run_command_with_progress(&mut command, start_percent, end_percent, progress)
}

pub fn delete_pack(root: &Path, manifest: &VoicePackManifest) -> Result<bool, VoicePackError> {
    let pack_root = pack_root(root, manifest);
    if !pack_root.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(pack_root)?;
    Ok(true)
}

pub fn default_voice_pack_dir() -> Option<PathBuf> {
    app_paths::data_local_dir().map(|dir| dir.join("voice-packs"))
}

pub fn verify_archive_checksum(bytes: &[u8], expected_sha256: &str) -> Result<(), VoicePackError> {
    if expected_sha256.trim() == "builtin" {
        return Ok(());
    }
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected_sha256.trim()) {
        Ok(())
    } else {
        Err(VoicePackError::ChecksumMismatch {
            expected: expected_sha256.trim().to_string(),
            actual,
        })
    }
}

pub fn load_manifest(path: &Path) -> Result<VoicePackManifest, VoicePackError> {
    let raw = fs::read_to_string(path)?;
    toml::from_str(&raw).map_err(|error| VoicePackError::InvalidManifest(error.to_string()))
}

pub fn health_check(root: &Path, manifest: &VoicePackManifest) -> VoicePackHealth {
    let pack_root = pack_root(root, manifest);
    if !pack_root.exists() {
        return VoicePackHealth::Missing;
    }
    let engine_path = pack_root.join(&manifest.engine_executable);
    let model_path = pack_root.join(&manifest.model_path);
    if !engine_path.is_file() {
        return VoicePackHealth::Broken(format!(
            "engine executable missing: {}",
            engine_path.display()
        ));
    }
    if !model_path.exists() {
        return VoicePackHealth::Broken(format!("model path missing: {}", model_path.display()));
    }
    VoicePackHealth::Ready {
        engine_path,
        model_path,
    }
}

fn run_command_with_progress<F>(
    command: &mut Command,
    start_percent: u8,
    end_percent: u8,
    progress: &mut F,
) -> Result<(), VoicePackError>
where
    F: FnMut(u8),
{
    let rendered = format!("{command:?}");
    let log_path = crate::voice::process::voice_pack_log_path(VOICE_PACK_INSTALL_LOG_FILE);
    let capture = CommandOutputCapture::start(&rendered, log_path.clone());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::voice::process::apply_background_spawn(command);
    let mut child = command.spawn()?;
    capture.capture_child_output(&mut child);
    let mut percent = start_percent.min(end_percent);
    progress(percent);

    let mut last_progress_tick = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            capture.finish();
            if status.success() {
                progress(end_percent);
                return Ok(());
            }
            return Err(VoicePackError::CommandFailed {
                command: rendered,
                status: status.to_string(),
                log_path: log_path.map(|path| path.display().to_string()),
                output_tail: capture.tail(),
            });
        }

        thread::sleep(Duration::from_millis(250));
        if percent < end_percent && last_progress_tick.elapsed() >= Duration::from_secs(5) {
            percent = percent.saturating_add(1).min(end_percent);
            progress(percent);
            last_progress_tick = Instant::now();
        }
    }
}

#[derive(Clone)]
struct CommandOutputCapture {
    tail: Arc<Mutex<OutputTail>>,
    log: Option<Arc<Mutex<File>>>,
    readers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl CommandOutputCapture {
    fn start(command: &str, log_path: Option<PathBuf>) -> Self {
        let log = log_path
            .and_then(|path| {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                OpenOptions::new().create(true).append(true).open(path).ok()
            })
            .map(|file| Arc::new(Mutex::new(file)));
        let capture = Self {
            tail: Arc::new(Mutex::new(OutputTail::new(COMMAND_OUTPUT_TAIL_LIMIT))),
            log,
            readers: Arc::new(Mutex::new(Vec::new())),
        };
        capture.write_log_line(&format!(
            "\n==> {} voice pack command: {command}\n",
            unix_timestamp()
        ));
        capture
    }

    fn capture_child_output(&self, child: &mut std::process::Child) {
        if let Some(stdout) = child.stdout.take() {
            self.spawn_reader("stdout", stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_reader("stderr", stderr);
        }
    }

    fn spawn_reader<R>(&self, label: &'static str, mut reader: R)
    where
        R: Read + Send + 'static,
    {
        let tail = self.tail.clone();
        let log = self.log.clone();
        let handle = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let chunk = &buffer[..count];
                        if let Some(log) = &log
                            && let Ok(mut log) = log.lock()
                        {
                            let _ = log.write_all(chunk);
                            let _ = log.flush();
                        }
                        if let Ok(mut tail) = tail.lock() {
                            tail.push_labeled(label, chunk);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
        if let Ok(mut readers) = self.readers.lock() {
            readers.push(handle);
        }
    }

    fn finish(&self) {
        let readers = self
            .readers
            .lock()
            .map(|mut readers| readers.drain(..).collect::<Vec<_>>())
            .unwrap_or_default();
        for reader in readers {
            let _ = reader.join();
        }
    }

    fn tail(&self) -> String {
        self.tail
            .lock()
            .map(|tail| tail.as_string())
            .unwrap_or_default()
    }

    fn write_log_line(&self, line: &str) {
        if let Some(log) = &self.log
            && let Ok(mut log) = log.lock()
        {
            let _ = log.write_all(line.as_bytes());
            let _ = log.flush();
        }
    }
}

struct OutputTail {
    bytes: Vec<u8>,
    limit: usize,
}

impl OutputTail {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn push_labeled(&mut self, label: &str, chunk: &[u8]) {
        self.push(format!("[{label}] ").as_bytes());
        self.push(chunk);
        if !chunk.ends_with(b"\n") {
            self.push(b"\n");
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > self.limit {
            let excess = self.bytes.len() - self.limit;
            self.bytes.drain(0..excess);
        }
    }

    fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
fn system_python_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    }
}

#[derive(Clone, Copy, Debug)]
struct PythonCommandSpec {
    program: &'static str,
    args: &'static [&'static str],
}

fn resolve_host_python_command() -> Result<Command, VoicePackError> {
    let mut failures = Vec::new();
    for spec in host_python_candidates() {
        match probe_host_python(spec) {
            Ok(detail) => {
                logging::info(format!(
                    "using host Python for voice pack venv creation: {detail}"
                ));
                return Ok(command_from_python_spec(spec));
            }
            Err(detail) => failures.push(detail),
        }
    }
    Err(VoicePackError::PythonUnavailable(format!(
        "Voice pack setup requires 64-bit Python 3.10+ on PATH. Install Python, then retry Install / Reinstall. Attempts: {}",
        failures.join("; ")
    )))
}

fn host_python_candidates() -> &'static [PythonCommandSpec] {
    #[cfg(target_os = "windows")]
    {
        const CANDIDATES: &[PythonCommandSpec] = &[
            PythonCommandSpec {
                program: "py",
                args: &["-3"],
            },
            PythonCommandSpec {
                program: "python",
                args: &[],
            },
        ];
        CANDIDATES
    }
    #[cfg(not(target_os = "windows"))]
    {
        const CANDIDATES: &[PythonCommandSpec] = &[
            PythonCommandSpec {
                program: "python3",
                args: &[],
            },
            PythonCommandSpec {
                program: "python",
                args: &[],
            },
        ];
        CANDIDATES
    }
}

fn command_from_python_spec(spec: &PythonCommandSpec) -> Command {
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    command
}

fn render_python_spec(spec: &PythonCommandSpec) -> String {
    std::iter::once(spec.program)
        .chain(spec.args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

fn probe_host_python(spec: &PythonCommandSpec) -> Result<String, String> {
    const PYTHON_PROBE: &str = r#"
import platform
import struct
import sys
bits = struct.calcsize("P") * 8
version = f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
print(f"{sys.executable} ({version}, {bits}-bit, {platform.platform()})")
if sys.version_info < (3, 10) or bits != 64:
    raise SystemExit(1)
"#;
    let mut command = command_from_python_spec(spec);
    command
        .arg("-c")
        .arg(PYTHON_PROBE)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::voice::process::apply_background_spawn(&mut command);
    match command.output() {
        Ok(output) if output.status.success() => {
            let detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(if detail.is_empty() {
                render_python_spec(spec)
            } else {
                detail
            })
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                output.status.to_string()
            };
            Err(format!("{}: {detail}", render_python_spec(spec)))
        }
        Err(error) => Err(format!("{}: {error}", render_python_spec(spec))),
    }
}

fn probe_venv_pip(python: &Path) -> Result<(), String> {
    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg("import pip, pip._internal")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::voice::process::apply_background_spawn(&mut command);
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                output.status.to_string()
            };
            Err(detail)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (bytes.len() as u64) * 8;
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = H0;
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let offset = i * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn verifies_sha256_checksum() {
        assert!(
            verify_archive_checksum(
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
            .is_ok()
        );
        assert!(matches!(
            verify_archive_checksum(b"abc", "deadbeef"),
            Err(VoicePackError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn command_progress_reports_start_and_end() {
        let mut seen = Vec::new();
        run_command_with_progress(
            Command::new(system_python_command()).arg("-c").arg("pass"),
            10,
            12,
            &mut |percent| seen.push(percent),
        )
        .unwrap();

        assert_eq!(seen.first(), Some(&10));
        assert_eq!(seen.last(), Some(&12));
    }

    #[test]
    fn command_progress_captures_failure_output() {
        let mut seen = Vec::new();
        let error = run_command_with_progress(
            Command::new(system_python_command()).arg("-c").arg(
                "import sys; print('voice stdout'); print('voice stderr', file=sys.stderr); sys.exit(7)",
            ),
            10,
            12,
            &mut |percent| seen.push(percent),
        )
        .unwrap_err();

        let VoicePackError::CommandFailed {
            status,
            output_tail,
            ..
        } = error
        else {
            panic!("expected command failure");
        };
        assert!(status.contains('7'));
        assert!(output_tail.contains("voice stdout"));
        assert!(output_tail.contains("voice stderr"));
        assert_eq!(seen.first(), Some(&10));
    }

    #[test]
    fn health_check_requires_engine_and_model() {
        let root = std::env::temp_dir().join(format!("terminaltiler-pack-{}", Uuid::new_v4()));
        let manifest = VoicePackManifest {
            id: "fake".into(),
            version: "1".into(),
            engine_executable: "bin/engine".into(),
            model_path: "model".into(),
            archive_url: "https://example.invalid/fake.tar.zst".into(),
            archive_sha256: "00".into(),
            model_name: default_parakeet_model_name(),
            streaming_model_name: default_parakeet_streaming_model_name(),
            python_requirements: Vec::new(),
        };
        assert_eq!(health_check(&root, &manifest), VoicePackHealth::Missing);
        let pack_root = root.join("fake").join("1");
        fs::create_dir_all(pack_root.join("bin")).unwrap();
        fs::write(pack_root.join("bin/engine"), "#!/bin/sh").unwrap();
        assert!(matches!(
            health_check(&root, &manifest),
            VoicePackHealth::Broken(_)
        ));
        fs::create_dir_all(pack_root.join("model")).unwrap();
        assert!(matches!(
            health_check(&root, &manifest),
            VoicePackHealth::Ready { .. }
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundled_parakeet_pack_installs_helper_manifest_and_model_cache_dir() {
        let root =
            std::env::temp_dir().join(format!("terminaltiler-builtin-pack-{}", Uuid::new_v4()));
        let manifest = install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = pack_root(&root, &manifest);

        assert_eq!(manifest.model_name, "nvidia/parakeet-tdt-0.6b-v2");
        assert!(pack_root.join(&manifest.engine_executable).is_file());
        assert!(pack_root.join("requirements.txt").is_file());
        assert!(pack_root.join(&manifest.model_path).is_dir());
        assert_eq!(
            python_environment_executable(&root, &manifest),
            if cfg!(target_os = "windows") {
                pack_root.join(".venv").join("Scripts").join("python.exe")
            } else {
                pack_root.join(".venv").join("bin").join("python")
            }
        );
        assert!(matches!(
            health_check(&root, &manifest),
            VoicePackHealth::Ready { .. }
        ));
        assert!(delete_pack(&root, &manifest).unwrap());
        assert_eq!(health_check(&root, &manifest), VoicePackHealth::Missing);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refresh_builtin_parakeet_pack_rewrites_assets_without_deleting_cache_or_venv() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-builtin-pack-refresh-{}",
            Uuid::new_v4()
        ));
        let manifest = builtin_parakeet_manifest();
        let pack_root = pack_root(&root, &manifest);
        let venv_sentinel = pack_root.join(".venv").join("sentinel.txt");
        let cache_sentinel = pack_root
            .join(&manifest.model_path)
            .join("model-sentinel.txt");
        fs::create_dir_all(venv_sentinel.parent().unwrap()).unwrap();
        fs::create_dir_all(cache_sentinel.parent().unwrap()).unwrap();
        fs::write(
            pack_root.join(&manifest.engine_executable),
            "# stale helper",
        )
        .unwrap();
        fs::write(pack_root.join("manifest.toml"), "stale = true").unwrap();
        fs::write(pack_root.join("requirements.txt"), "stale-requirement").unwrap();
        fs::write(&venv_sentinel, "keep venv").unwrap();
        fs::write(&cache_sentinel, "keep cache").unwrap();

        assert_eq!(
            refresh_builtin_parakeet_pack_assets(&root).unwrap(),
            Some(manifest.clone())
        );

        let refreshed_engine =
            fs::read_to_string(pack_root.join(&manifest.engine_executable)).unwrap();
        assert!(refreshed_engine.contains("audio-final-pcm16-hex"));
        assert!(
            fs::read_to_string(pack_root.join("manifest.toml"))
                .unwrap()
                .contains("nvidia-parakeet-tdt-0.6b-v2-nemo")
        );
        assert!(
            fs::read_to_string(pack_root.join("requirements.txt"))
                .unwrap()
                .contains("nemo_toolkit")
        );
        assert_eq!(fs::read_to_string(venv_sentinel).unwrap(), "keep venv");
        assert_eq!(fs::read_to_string(cache_sentinel).unwrap(), "keep cache");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn python_environment_recreates_broken_venv_without_deleting_model_cache() {
        if resolve_host_python_command().is_err() {
            eprintln!("skipping voice pack venv repair test because Python 3.10+ is unavailable");
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-broken-venv-repair-{}",
            Uuid::new_v4()
        ));
        let manifest = VoicePackManifest {
            id: "fake".into(),
            version: "1".into(),
            engine_executable: "engine.py".into(),
            model_path: "hf-cache/model".into(),
            archive_url: "builtin://fake".into(),
            archive_sha256: "builtin".into(),
            model_name: default_parakeet_model_name(),
            streaming_model_name: default_parakeet_streaming_model_name(),
            python_requirements: Vec::new(),
        };
        let pack_root = pack_root(&root, &manifest);
        let cache_sentinel = pack_root.join(&manifest.model_path).join("sentinel.txt");
        fs::create_dir_all(cache_sentinel.parent().unwrap()).unwrap();
        fs::write(&cache_sentinel, "keep cache").unwrap();

        let broken_python = python_environment_executable(&root, &manifest);
        fs::create_dir_all(broken_python.parent().unwrap()).unwrap();
        write_broken_python_executable(&broken_python);

        let mut progress = Vec::new();
        ensure_python_environment_with_working_pip(&root, &manifest, &mut |percent| {
            progress.push(percent);
        })
        .unwrap();

        let repaired_python = python_environment_executable(&root, &manifest);
        assert!(repaired_python.is_file());
        assert!(probe_venv_pip(&repaired_python).is_ok());
        assert_eq!(fs::read_to_string(cache_sentinel).unwrap(), "keep cache");
        assert!(progress.contains(&24));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_failure_user_message_points_to_log_without_traceback_tail() {
        let error = VoicePackError::CommandFailed {
            command: "python -m pip install".into(),
            status: "exit code: 1".into(),
            log_path: Some("C:\\Users\\z\\voice-pack-install.log".into()),
            output_tail: "Traceback (most recent call last):\nModuleNotFoundError: No module named 'pip._internal'".into(),
        };

        let message = error.user_message();

        assert_eq!(
            message,
            "Voice pack setup failed; see C:\\Users\\z\\voice-pack-install.log"
        );
        assert!(!message.contains("Traceback"));
        assert!(error.to_string().contains("pip._internal"));
    }

    fn write_broken_python_executable(path: &Path) {
        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::write(path, "#!/bin/sh\nexit 1\n").unwrap();
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
        #[cfg(not(target_family = "unix"))]
        {
            fs::write(path, "not a real python executable").unwrap();
        }
    }
}
