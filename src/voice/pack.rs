use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

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
    CommandFailed { command: String, status: String },
    ChecksumMismatch { expected: String, actual: String },
    InvalidManifest(String),
}

impl From<io::Error> for VoicePackError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

const BUILTIN_PARAKEET_MANIFEST: &str =
    include_str!("../../resources/voice/parakeet/manifest.toml");
const BUILTIN_PARAKEET_ENGINE: &str =
    include_str!("../../resources/voice/parakeet/parakeet_engine.py");

pub fn default_parakeet_model_name() -> String {
    "nvidia/parakeet-tdt-0.6b-v2".into()
}

pub fn builtin_parakeet_manifest() -> VoicePackManifest {
    toml::from_str(BUILTIN_PARAKEET_MANIFEST)
        .expect("bundled Parakeet voice manifest must be valid TOML")
}

pub fn install_builtin_parakeet_pack(root: &Path) -> Result<VoicePackManifest, VoicePackError> {
    let manifest = builtin_parakeet_manifest();
    let pack_root = root.join(&manifest.id).join(&manifest.version);
    fs::create_dir_all(&pack_root)?;
    fs::write(
        pack_root.join(&manifest.engine_executable),
        BUILTIN_PARAKEET_ENGINE,
    )?;
    fs::create_dir_all(pack_root.join(&manifest.model_path))?;
    fs::write(pack_root.join("manifest.toml"), BUILTIN_PARAKEET_MANIFEST)?;
    if !manifest.python_requirements.is_empty() {
        fs::write(
            pack_root.join("requirements.txt"),
            format!("{}\n", manifest.python_requirements.join("\n")),
        )?;
    }
    Ok(manifest)
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
    if !python.is_file() {
        run_command_with_progress(
            Command::new(system_python_command())
                .arg("-m")
                .arg("venv")
                .arg(python_environment_dir(root, manifest)),
            10,
            20,
            &mut progress,
        )?;
    }

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

pub fn delete_pack(root: &Path, manifest: &VoicePackManifest) -> Result<bool, VoicePackError> {
    let pack_root = pack_root(root, manifest);
    if !pack_root.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(pack_root)?;
    Ok(true)
}

pub fn default_voice_pack_dir() -> Option<PathBuf> {
    ProjectDirs::from("dev", "Zethrus", "TerminalTiler")
        .map(|dirs| dirs.data_local_dir().join("voice-packs"))
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
    let mut child = command.spawn()?;
    let mut percent = start_percent.min(end_percent);
    progress(percent);

    let mut last_progress_tick = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                progress(end_percent);
                return Ok(());
            }
            return Err(VoicePackError::CommandFailed {
                command: rendered,
                status: status.to_string(),
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

fn system_python_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
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
}
