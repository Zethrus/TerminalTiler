use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::voice::pack::{VoicePackHealth, VoicePackManifest};
use crate::voice::preferences::VoiceEngineMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceEngineRequest {
    Start { sample_rate_hz: u32 },
    AudioPcm16(Vec<i16>),
    BufferedAudioPcm16(Vec<i16>),
    FinalAudioPcm16(Vec<i16>),
    Stop,
    Capabilities,
    Warm,
    Health,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceEngineEvent {
    Ready,
    Capabilities(VoiceEngineCapabilities),
    Partial(String),
    Final(String),
    Error(String),
    Health { ok: bool, detail: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoiceEngineCapabilities {
    pub streaming: bool,
    pub model_id: String,
    pub device: String,
    pub warm: bool,
}

impl VoiceEngineCapabilities {
    pub fn legacy_warm() -> Self {
        Self {
            streaming: false,
            model_id: "legacy-helper".into(),
            device: "unknown".into(),
            warm: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramedMessage {
    pub kind: String,
    pub payload: String,
}

impl FramedMessage {
    pub fn new(kind: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            payload: payload.into(),
        }
    }

    pub fn encode(&self, mut writer: impl Write) -> io::Result<()> {
        let kind = self.kind.replace('\n', " ");
        let payload = self.payload.replace('\n', "\\n");
        writeln!(writer, "{} {}", kind, payload)
    }

    pub fn decode(line: &str) -> Option<Self> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let (kind, payload) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
        if kind.is_empty() {
            return None;
        }
        Some(Self::new(kind, payload.replace("\\n", "\n")))
    }
}

pub fn read_framed_message(reader: &mut impl BufRead) -> io::Result<Option<FramedMessage>> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Ok(None);
    }
    Ok(FramedMessage::decode(&line))
}

fn read_engine_event(reader: &mut impl BufRead) -> io::Result<Option<VoiceEngineEvent>> {
    loop {
        let Some(frame) = read_framed_message(reader)? else {
            return Ok(None);
        };
        if is_engine_event_frame(&frame) {
            return Ok(Some(event_from_frame(&frame)));
        }
        eprintln!(
            "ignored non-protocol voice engine stdout line starting with {:?}",
            frame.kind
        );
    }
}

fn is_engine_event_frame(frame: &FramedMessage) -> bool {
    matches!(
        frame.kind.as_str(),
        "ready" | "capabilities" | "partial" | "final" | "health" | "error"
    )
}

pub fn frame_from_request(request: &VoiceEngineRequest) -> FramedMessage {
    match request {
        VoiceEngineRequest::Start { sample_rate_hz } => {
            FramedMessage::new("start", sample_rate_hz.to_string())
        }
        VoiceEngineRequest::AudioPcm16(samples) => {
            FramedMessage::new("audio-pcm16-hex", pcm16_samples_to_hex(samples))
        }
        VoiceEngineRequest::BufferedAudioPcm16(samples) => {
            FramedMessage::new("audio-buffer-pcm16-hex", pcm16_samples_to_hex(samples))
        }
        VoiceEngineRequest::FinalAudioPcm16(samples) => {
            FramedMessage::new("audio-final-pcm16-hex", pcm16_samples_to_hex(samples))
        }
        VoiceEngineRequest::Stop => FramedMessage::new("stop", ""),
        VoiceEngineRequest::Capabilities => FramedMessage::new("capabilities", ""),
        VoiceEngineRequest::Warm => FramedMessage::new("warm", ""),
        VoiceEngineRequest::Health => FramedMessage::new("health", ""),
        VoiceEngineRequest::Shutdown => FramedMessage::new("shutdown", ""),
    }
}

pub fn event_from_frame(frame: &FramedMessage) -> VoiceEngineEvent {
    match frame.kind.as_str() {
        "ready" => VoiceEngineEvent::Ready,
        "capabilities" => VoiceEngineEvent::Capabilities(capabilities_from_payload(&frame.payload)),
        "partial" => VoiceEngineEvent::Partial(frame.payload.clone()),
        "final" => VoiceEngineEvent::Final(frame.payload.clone()),
        "health" => VoiceEngineEvent::Health {
            ok: frame.payload.starts_with("ok"),
            detail: frame.payload.clone(),
        },
        "error" => VoiceEngineEvent::Error(frame.payload.clone()),
        _ => VoiceEngineEvent::Error(format!("unknown engine frame kind '{}'", frame.kind)),
    }
}

fn capabilities_from_payload(payload: &str) -> VoiceEngineCapabilities {
    let mut capabilities = VoiceEngineCapabilities::default();
    for part in payload.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        match key.trim() {
            "streaming" => capabilities.streaming = matches!(value.trim(), "true" | "1" | "yes"),
            "model" | "model_id" => capabilities.model_id = value.trim().to_string(),
            "device" => capabilities.device = value.trim().to_string(),
            "warm" => capabilities.warm = matches!(value.trim(), "true" | "1" | "yes"),
            _ => {}
        }
    }
    capabilities
}

pub struct VoiceEngineProcess {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<io::Result<Option<VoiceEngineEvent>>>,
}

#[derive(Clone, Copy, Debug)]
struct VoiceEngineHealthCheckTimeouts {
    startup: Duration,
    response: Duration,
    shutdown: Duration,
}

const DEFAULT_HEALTH_CHECK_TIMEOUTS: VoiceEngineHealthCheckTimeouts =
    VoiceEngineHealthCheckTimeouts {
        startup: Duration::from_secs(3),
        response: Duration::from_secs(60),
        shutdown: Duration::from_secs(2),
    };

pub fn run_voice_engine_health_check(
    manifest: &VoicePackManifest,
    health: VoicePackHealth,
    engine_mode: VoiceEngineMode,
) -> io::Result<VoiceEngineEvent> {
    run_voice_engine_health_check_with_timeouts(
        manifest,
        health,
        engine_mode,
        DEFAULT_HEALTH_CHECK_TIMEOUTS,
    )
}

fn run_voice_engine_health_check_with_timeouts(
    manifest: &VoicePackManifest,
    health: VoicePackHealth,
    engine_mode: VoiceEngineMode,
    timeouts: VoiceEngineHealthCheckTimeouts,
) -> io::Result<VoiceEngineEvent> {
    let mut process = VoiceEngineProcess::launch(manifest, health, engine_mode)?;
    let event = run_voice_engine_health_check_request(&mut process, timeouts);
    let shutdown = process.shutdown_with_timeout(timeouts.shutdown);
    match event {
        Ok(event) => Ok(event),
        Err(error) => {
            let _ = shutdown;
            Err(error)
        }
    }
}

fn run_voice_engine_health_check_request(
    process: &mut VoiceEngineProcess,
    timeouts: VoiceEngineHealthCheckTimeouts,
) -> io::Result<VoiceEngineEvent> {
    let _ = process.read_event_timeout(timeouts.startup)?;
    process.send(&VoiceEngineRequest::Health)?;
    let deadline = Instant::now() + timeouts.response;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(VoiceEngineEvent::Error(
                "voice engine health check timed out".into(),
            ));
        };
        match process.read_event_timeout(remaining) {
            Ok(Some(event @ VoiceEngineEvent::Health { .. })) => return Ok(event),
            Ok(Some(event @ VoiceEngineEvent::Error(_))) => return Ok(event),
            Ok(Some(_)) => continue,
            Ok(None) => {
                return Ok(VoiceEngineEvent::Error(
                    "voice engine exited during health check".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                return Ok(VoiceEngineEvent::Error(
                    "voice engine health check timed out".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

impl VoiceEngineProcess {
    pub fn launch(
        manifest: &VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
    ) -> io::Result<Self> {
        let VoicePackHealth::Ready {
            engine_path,
            model_path,
        } = health
        else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "voice pack is not ready",
            ));
        };

        let mut command = command_for_engine(&engine_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(crate::voice::process::voice_engine_stderr())
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_PARAKEET_STREAMING_MODEL",
                &manifest.streaming_model_name,
            )
            .env("TERMINALTILER_VOICE_MODEL_PATH", model_path)
            .env("TERMINALTILER_VOICE_ENGINE_MODE", engine_mode.env_value());
        crate::voice::process::apply_background_spawn(&mut command);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("voice engine stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("voice engine stdout unavailable"))?;

        let stdout_rx = spawn_stdout_reader(stdout);

        Ok(Self {
            child,
            stdin,
            stdout_rx,
        })
    }

    pub fn send(&mut self, request: &VoiceEngineRequest) -> io::Result<()> {
        frame_from_request(request).encode(&mut self.stdin)?;
        self.stdin.flush()
    }

    pub fn read_event(&mut self) -> io::Result<Option<VoiceEngineEvent>> {
        self.stdout_rx.recv().unwrap_or(Ok(None))
    }

    pub fn read_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> io::Result<Option<VoiceEngineEvent>> {
        match self.stdout_rx.recv_timeout(timeout) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "voice engine did not respond within {}ms",
                    timeout.as_millis()
                ),
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    pub fn shutdown(self) -> io::Result<()> {
        self.shutdown_with_timeout(Duration::from_secs(2))
    }

    fn shutdown_with_timeout(mut self, timeout: Duration) -> io::Result<()> {
        let _ = self.send(&VoiceEngineRequest::Shutdown);
        wait_for_child_or_kill(&mut self.child, timeout)
    }
}

fn spawn_stdout_reader(
    stdout: std::process::ChildStdout,
) -> mpsc::Receiver<io::Result<Option<VoiceEngineEvent>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_engine_event(&mut reader) {
                Ok(Some(event)) => {
                    if tx.send(Ok(Some(event))).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = tx.send(Ok(None));
                    break;
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                    break;
                }
            }
        }
    });
    rx
}

fn wait_for_child_or_kill(child: &mut Child, timeout: Duration) -> io::Result<()> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

impl VoiceEngineMode {
    fn env_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cuda => "cuda",
            Self::Cpu => "cpu",
        }
    }
}

fn command_for_engine(engine_path: &Path) -> Command {
    if engine_path.extension() == Some(OsStr::new("py")) {
        let mut command = Command::new(python_command_for_engine(engine_path));
        command.arg(engine_path);
        command
    } else {
        Command::new(engine_path)
    }
}

fn python_command_for_engine(engine_path: &Path) -> std::ffi::OsString {
    if let Some(pack_root) = engine_path.parent() {
        let venv_python = if cfg!(target_os = "windows") {
            pack_root.join(".venv").join("Scripts").join("python.exe")
        } else {
            pack_root.join(".venv").join("bin").join("python")
        };
        if venv_python.is_file() {
            return venv_python.into_os_string();
        }
    }
    python_command().into()
}

fn python_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    }
}

fn pcm16_samples_to_hex(samples: &[i16]) -> String {
    let mut output = String::with_capacity(samples.len() * 4);
    for sample in samples {
        for byte in sample.to_le_bytes() {
            output.push_str(&format!("{byte:02x}"));
        }
    }
    output
}

#[derive(Default)]
pub struct FakeVoiceEngineClient {
    scripted_events: Vec<VoiceEngineEvent>,
    requests: Vec<VoiceEngineRequest>,
}

impl FakeVoiceEngineClient {
    pub fn with_events(scripted_events: Vec<VoiceEngineEvent>) -> Self {
        Self {
            scripted_events,
            requests: Vec::new(),
        }
    }

    pub fn send(&mut self, request: VoiceEngineRequest) {
        self.requests.push(request);
    }

    pub fn drain_events(&mut self) -> Vec<VoiceEngineEvent> {
        std::mem::take(&mut self.scripted_events)
    }

    pub fn requests(&self) -> &[VoiceEngineRequest] {
        &self.requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framed_messages_round_trip_payloads() {
        let frame = FramedMessage::new("partial", "hello\nworld");
        let mut bytes = Vec::new();
        frame.encode(&mut bytes).unwrap();
        let decoded = read_framed_message(&mut Cursor::new(bytes))
            .unwrap()
            .unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn encodes_pcm16_audio_as_little_endian_hex_frame() {
        assert_eq!(
            frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![1, -2, 0x1234])),
            FramedMessage::new("audio-pcm16-hex", "0100feff3412")
        );
        assert_eq!(
            frame_from_request(&VoiceEngineRequest::BufferedAudioPcm16(vec![1, -2, 0x1234])),
            FramedMessage::new("audio-buffer-pcm16-hex", "0100feff3412")
        );
        assert_eq!(
            frame_from_request(&VoiceEngineRequest::FinalAudioPcm16(vec![1, -2, 0x1234])),
            FramedMessage::new("audio-final-pcm16-hex", "0100feff3412")
        );
        assert_eq!(
            frame_from_request(&VoiceEngineRequest::Warm),
            FramedMessage::new("warm", "")
        );
    }

    #[test]
    fn engine_event_reader_ignores_third_party_stdout_noise() {
        let mut reader = Cursor::new(
            b"[NeMo I 2026-05-17 fake restore log]
Downloading model shard 1/2
health ok: model loaded
"
            .as_slice(),
        );

        assert_eq!(
            read_engine_event(&mut reader).unwrap(),
            Some(VoiceEngineEvent::Health {
                ok: true,
                detail: "ok: model loaded".into(),
            })
        );
    }

    #[test]
    fn parses_health_frames_with_details() {
        assert_eq!(
            event_from_frame(&FramedMessage::new("health", "ok: NeMo available")),
            VoiceEngineEvent::Health {
                ok: true,
                detail: "ok: NeMo available".into()
            }
        );
        assert_eq!(
            event_from_frame(&FramedMessage::new("health", "error: missing nemo")),
            VoiceEngineEvent::Health {
                ok: false,
                detail: "error: missing nemo".into()
            }
        );
    }

    #[test]
    fn parses_capabilities_frames() {
        assert_eq!(
            event_from_frame(&FramedMessage::new(
                "capabilities",
                "streaming=true, model=nvidia/parakeet-ctc-0.6b, device=cuda, warm=true"
            )),
            VoiceEngineEvent::Capabilities(VoiceEngineCapabilities {
                streaming: true,
                model_id: "nvidia/parakeet-ctc-0.6b".into(),
                device: "cuda".into(),
                warm: true,
            })
        );
    }

    #[test]
    fn engine_read_timeout_returns_timed_out_error() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-engine-timeout-{}",
            uuid::Uuid::new_v4()
        ));
        let pack_root = root.join("timeout").join("1");
        std::fs::create_dir_all(pack_root.join("model")).unwrap();
        let engine_path = pack_root.join("timeout_engine.py");
        std::fs::write(
            &engine_path,
            r#"#!/usr/bin/env python3
import sys
import time
print("ready timeout-helper", flush=True)
for raw in sys.stdin:
    kind = raw.strip().split(" ", 1)[0]
    if kind == "start":
        time.sleep(0.2)
        print("ready started", flush=True)
    elif kind == "shutdown":
        print("ready shutdown", flush=True)
        raise SystemExit(0)
"#,
        )
        .unwrap();
        let manifest = VoicePackManifest {
            id: "timeout".into(),
            version: "1".into(),
            engine_executable: "timeout_engine.py".into(),
            model_path: "model".into(),
            archive_url: "builtin://timeout".into(),
            archive_sha256: "builtin".into(),
            model_name: "legacy/offline".into(),
            streaming_model_name: "legacy/streaming".into(),
            python_requirements: Vec::new(),
        };
        let health = VoicePackHealth::Ready {
            engine_path,
            model_path: pack_root.join("model"),
        };
        let mut process =
            VoiceEngineProcess::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process
            .send(&VoiceEngineRequest::Start {
                sample_rate_hz: 16_000,
            })
            .unwrap();
        let error = process
            .read_event_timeout(Duration::from_millis(20))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);

        process.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn health_check_timeout_shuts_down_helper() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-health-timeout-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        let pack_root = root.join("health-timeout").join("1");
        std::fs::create_dir_all(pack_root.join("model")).unwrap();
        let pid_path = root.join("helper.pid");
        let engine_path = pack_root.join("health_timeout_engine.py");
        std::fs::write(
            &engine_path,
            format!(
                r#"#!/usr/bin/env python3
import os
import sys
import time
with open({pid_path:?}, "w", encoding="utf-8") as handle:
    handle.write(str(os.getpid()))
print("ready health-timeout-helper", flush=True)
for raw in sys.stdin:
    kind = raw.strip().split(" ", 1)[0]
    if kind == "health":
        time.sleep(60)
    elif kind == "shutdown":
        print("ready shutdown", flush=True)
        raise SystemExit(0)
"#,
                pid_path = pid_path.display().to_string(),
            ),
        )
        .unwrap();
        let manifest = VoicePackManifest {
            id: "health-timeout".into(),
            version: "1".into(),
            engine_executable: "health_timeout_engine.py".into(),
            model_path: "model".into(),
            archive_url: "builtin://health-timeout".into(),
            archive_sha256: "builtin".into(),
            model_name: "legacy/offline".into(),
            streaming_model_name: "legacy/streaming".into(),
            python_requirements: Vec::new(),
        };
        let health = VoicePackHealth::Ready {
            engine_path,
            model_path: pack_root.join("model"),
        };

        let event = run_voice_engine_health_check_with_timeouts(
            &manifest,
            health,
            VoiceEngineMode::Cpu,
            VoiceEngineHealthCheckTimeouts {
                startup: Duration::from_secs(1),
                response: Duration::from_millis(20),
                shutdown: Duration::from_millis(50),
            },
        )
        .unwrap();

        assert!(matches!(
            event,
            VoiceEngineEvent::Error(message) if message.contains("timed out")
        ));
        let pid = std::fs::read_to_string(&pid_path).unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists(),
            "health-check helper should be shut down after timeout"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fake_engine_records_requests_and_returns_scripted_events() {
        let mut engine = FakeVoiceEngineClient::with_events(vec![
            VoiceEngineEvent::Partial("hel".into()),
            VoiceEngineEvent::Final("hello".into()),
        ]);
        engine.send(VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        });
        assert_eq!(
            engine.requests(),
            &[VoiceEngineRequest::Start {
                sample_rate_hz: 16_000
            }]
        );
        assert_eq!(engine.drain_events().len(), 2);
        assert!(engine.drain_events().is_empty());
    }
    #[test]
    fn launches_bundled_python_helper_and_reads_ready_event() {
        let root =
            std::env::temp_dir().join(format!("terminaltiler-engine-{}", uuid::Uuid::new_v4()));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut process =
            VoiceEngineProcess::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process.send(&VoiceEngineRequest::Shutdown).unwrap();
        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn python_helpers_prefer_pack_virtual_environment() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-engine-python-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let venv_python = if cfg!(target_os = "windows") {
            pack_root.join(".venv").join("Scripts").join("python.exe")
        } else {
            pack_root.join(".venv").join("bin").join("python")
        };
        std::fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
        std::fs::write(&venv_python, "").unwrap();

        assert_eq!(
            python_command_for_engine(&pack_root.join(&manifest.engine_executable)),
            venv_python.into_os_string()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_reports_audio_capture_partials_before_transcription() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-streaming-audio-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut process =
            VoiceEngineProcess::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process
            .send(&VoiceEngineRequest::Start {
                sample_rate_hz: 16_000,
            })
            .unwrap();
        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process
            .send(&VoiceEngineRequest::AudioPcm16(vec![0, 1, -1, 2]))
            .unwrap();

        assert!(matches!(
            process.read_event().unwrap(),
            Some(VoiceEngineEvent::Partial(text))
                if text.contains("Captured") || text.contains("Streaming ASR unavailable")
        ));
        process.send(&VoiceEngineRequest::Shutdown).unwrap();
        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_buffers_audio_without_partial_inference() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-buffered-audio-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut process =
            VoiceEngineProcess::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process
            .send(&VoiceEngineRequest::Start {
                sample_rate_hz: 16_000,
            })
            .unwrap();
        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process
            .send(&VoiceEngineRequest::BufferedAudioPcm16(vec![0, 1, -1, 2]))
            .unwrap();

        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        process.send(&VoiceEngineRequest::Shutdown).unwrap();
        assert_eq!(process.read_event().unwrap(), Some(VoiceEngineEvent::Ready));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_transcribes_with_fake_nemo_runtime() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-parakeet-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        assert inplace is True
        model.quantized = True
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("numpy.py"),
            r#"
class FakeArray:
    def astype(self, dtype):
        return self
    def __truediv__(self, other):
        return self
def frombuffer(data, dtype=None):
    return FakeArray()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class Transcript:
    text = "fake parakeet transcript"
class FakeModel:
    def to(self, device):
        self.device = device
        return self
    def eval(self):
        self.evaluated = True
    def transcribe(self, paths, timestamps=False):
        assert paths
        assert timestamps is False
        return [Transcript()]
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        print("[NeMo I fake restore] loading checkpoint", flush=True)
        model = FakeModel()
        model.model_name = model_name
        return model
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        })
        .encode(&mut stdin)
        .unwrap();
        frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        frame_from_request(&VoiceEngineRequest::Stop)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let mut saw_finalized_partial = false;
        let final_text = loop {
            let event = read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
                .expect("helper should emit final transcript");
            match event {
                VoiceEngineEvent::Partial(text) => {
                    saw_finalized_partial |= text.contains("final after release");
                }
                VoiceEngineEvent::Final(text) => break text,
                VoiceEngineEvent::Ready => continue,
                other => panic!("unexpected helper event: {other:?}"),
            }
        };

        assert!(saw_finalized_partial);
        assert_eq!(final_text, "fake parakeet transcript");
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_deduplicates_streaming_partials() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-streaming-dedup-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("numpy.py"),
            r#"
class FakeArray:
    def astype(self, dtype):
        return self
    def __truediv__(self, other):
        return self
def frombuffer(data, dtype=None):
    return FakeArray()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class Transcript:
    def __init__(self, text):
        self.text = text
class FakeModel:
    calls = 0
    def to(self, device):
        return self
    def eval(self):
        pass
    def transcribe(self, audio, **kwargs):
        FakeModel.calls += 1
        if FakeModel.calls == 1:
            return [Transcript("cargo")]
        if FakeModel.calls == 2:
            return [Transcript("cargo cargo test")]
        return [Transcript("cargo test extra")]
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        return FakeModel()
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_PARAKEET_STREAMING_MODEL",
                &manifest.streaming_model_name,
            )
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu")
            .env("TERMINALTILER_VOICE_PARTIAL_MIN_MS", "0");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        })
        .encode(&mut stdin)
        .unwrap();
        frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let mut partials = Vec::new();
        while partials.len() < 2 {
            match read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
            {
                Some(VoiceEngineEvent::Partial(text)) => partials.push(text),
                Some(VoiceEngineEvent::Ready) => continue,
                other => panic!("unexpected helper event: {other:?}"),
            }
        }
        assert_eq!(partials, vec!["cargo", "cargo test"]);
        frame_from_request(&VoiceEngineRequest::FinalAudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        frame_from_request(&VoiceEngineRequest::Stop)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();
        let final_text = loop {
            match read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
            {
                Some(VoiceEngineEvent::Partial(_)) | Some(VoiceEngineEvent::Ready) => continue,
                Some(VoiceEngineEvent::Final(text)) => break text,
                other => panic!("unexpected helper event: {other:?}"),
            }
        };
        assert_eq!(final_text, "cargo test extra");
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_acks_audio_without_partial_and_transcribes_on_stop() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-streaming-empty-partial-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("numpy.py"),
            r#"
class FakeArray:
    def astype(self, dtype):
        return self
    def __truediv__(self, other):
        return self
def frombuffer(data, dtype=None):
    return FakeArray()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class Transcript:
    def __init__(self, text):
        self.text = text
class FakeModel:
    calls = 0
    def to(self, device):
        return self
    def eval(self):
        pass
    def transcribe(self, audio, **kwargs):
        FakeModel.calls += 1
        if FakeModel.calls == 1:
            return [Transcript("")]
        return [Transcript("final transcript")]
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        return FakeModel()
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_PARAKEET_STREAMING_MODEL",
                &manifest.streaming_model_name,
            )
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu")
            .env("TERMINALTILER_VOICE_PARTIAL_MIN_MS", "0");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        })
        .encode(&mut stdin)
        .unwrap();
        frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let mut ready_count = 0;
        while ready_count < 2 {
            match read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
            {
                Some(VoiceEngineEvent::Ready) => ready_count += 1,
                other => panic!("unexpected helper event before audio ack: {other:?}"),
            }
        }

        frame_from_request(&VoiceEngineRequest::FinalAudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        frame_from_request(&VoiceEngineRequest::Stop)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();
        let final_text = loop {
            match read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
            {
                Some(VoiceEngineEvent::Partial(_)) | Some(VoiceEngineEvent::Ready) => continue,
                Some(VoiceEngineEvent::Final(text)) => break text,
                other => panic!("unexpected helper event after stop: {other:?}"),
            }
        };
        assert_eq!(final_text, "final transcript");
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_falls_back_to_offline_tdt_when_streaming_init_fails() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-streaming-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class Transcript:
    text = "offline fallback transcript"
class FakeModel:
    def to(self, device):
        return self
    def eval(self):
        pass
    def transcribe(self, paths, timestamps=False):
        assert paths
        return [Transcript()]
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        if "ctc" in model_name:
            raise RuntimeError("stream init failed")
        return FakeModel()
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_PARAKEET_STREAMING_MODEL",
                &manifest.streaming_model_name,
            )
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("PYTHONIOENCODING", "cp1252")
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        })
        .encode(&mut stdin)
        .unwrap();
        frame_from_request(&VoiceEngineRequest::AudioPcm16(vec![0; 160]))
            .encode(&mut stdin)
            .unwrap();
        frame_from_request(&VoiceEngineRequest::Stop)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let mut saw_fallback = false;
        let final_text = loop {
            match read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
            {
                Some(VoiceEngineEvent::Partial(text)) => {
                    saw_fallback |= text.contains("offline TDT");
                }
                Some(VoiceEngineEvent::Final(text)) => break text,
                Some(VoiceEngineEvent::Ready) => continue,
                other => panic!("unexpected helper event: {other:?}"),
            }
        };
        assert!(saw_fallback);
        assert_eq!(final_text, "offline fallback transcript");
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_reports_fake_runtime_health_without_loading_model() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-health-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        assert inplace is True
        model.quantized = True
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class FakeModel:
    def to(self, device):
        self.device = device
        return self
    def eval(self):
        self.evaluated = True
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        raise AssertionError("health must not load the model")
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Health)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let event = read_framed_message(&mut stdout)
            .unwrap()
            .map(|frame| event_from_frame(&frame))
            .expect("helper should emit health");
        let VoiceEngineEvent::Health { ok, detail } = event else {
            panic!("unexpected helper event: {event:?}");
        };
        assert!(ok);
        assert!(detail.contains("dependencies ready"));
        assert!(detail.contains("streaming=False") || detail.contains("streaming=false"));
        assert!(detail.contains("warm=False") || detail.contains("warm=false"));
        assert!(detail.contains("device=cpu"));
        assert!(detail.contains("torch=2.12.fake"));
        assert!(detail.contains("nemo=2.7.fake"));
        assert!(detail.contains("nvidia/parakeet-ctc"));
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_start_is_ready_without_model_warm() {
        use std::process::Stdio;

        let root = std::env::temp_dir().join(format!(
            "terminaltiler-fake-start-before-warm-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        raise AssertionError("start must not load the model")
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Start {
            sample_rate_hz: 16_000,
        })
        .encode(&mut stdin)
        .unwrap();
        stdin.flush().unwrap();
        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_warm_loads_fake_runtime_model() {
        use std::process::Stdio;

        let root =
            std::env::temp_dir().join(format!("terminaltiler-fake-warm-{}", uuid::Uuid::new_v4()));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let pack_root = crate::voice::pack::pack_root(&root, &manifest);
        let fake_modules = root.join("fake-python-modules");
        std::fs::create_dir_all(fake_modules.join("nemo/collections")).unwrap();
        std::fs::write(
            fake_modules.join("nemo/__init__.py"),
            "__version__ = \"2.7.fake\"\n",
        )
        .unwrap();
        std::fs::write(fake_modules.join("nemo/collections/__init__.py"), "").unwrap();
        std::fs::write(
            fake_modules.join("torch.py"),
            r#"
__version__ = "2.12.fake"
class cuda:
    @staticmethod
    def is_available():
        return False
class nn:
    class Linear:
        pass
class quantization:
    @staticmethod
    def quantize_dynamic(model, layers, dtype=None, inplace=False):
        assert inplace is True
        model.quantized = True
        return model
qint8 = object()
"#,
        )
        .unwrap();
        std::fs::write(
            fake_modules.join("nemo/collections/asr.py"),
            r#"
class FakeModel:
    def to(self, device):
        self.device = device
        return self
    def eval(self):
        self.evaluated = True
class ASRModel:
    @staticmethod
    def from_pretrained(model_name):
        print("[NeMo I fake restore] loading checkpoint", flush=True)
        model = FakeModel()
        model.model_name = model_name
        return model
class models:
    ASRModel = ASRModel
"#,
        )
        .unwrap();

        let mut command = Command::new(python_command());
        command
            .arg(pack_root.join(&manifest.engine_executable))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("PYTHONPATH", &fake_modules)
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env(
                "TERMINALTILER_VOICE_MODEL_PATH",
                pack_root.join(&manifest.model_path),
            )
            .env("TERMINALTILER_VOICE_ENGINE_MODE", "cpu");
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);

        assert_eq!(
            read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame)),
            Some(VoiceEngineEvent::Ready)
        );
        frame_from_request(&VoiceEngineRequest::Warm)
            .encode(&mut stdin)
            .unwrap();
        stdin.flush().unwrap();

        let mut saw_model_load_partial = false;
        let event = loop {
            let event = read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
                .expect("helper should emit warm health");
            match event {
                VoiceEngineEvent::Partial(text) => {
                    saw_model_load_partial |= text.contains("CPU quantization unavailable");
                }
                VoiceEngineEvent::Health { .. } => break event,
                other => panic!("unexpected helper event: {other:?}"),
            }
        };
        let VoiceEngineEvent::Health { ok, detail } = event else {
            panic!("unexpected helper event: {event:?}");
        };
        assert!(ok);
        assert!(detail.contains("model loaded"));
        assert!(detail.contains("streaming=True") || detail.contains("streaming=true"));
        assert!(detail.contains("quantized=True"));
        assert!(detail.contains("nvidia/parakeet-ctc"));
        assert!(!saw_model_load_partial);
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_reports_health_event() {
        let root =
            std::env::temp_dir().join(format!("terminaltiler-health-{}", uuid::Uuid::new_v4()));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let event = run_voice_engine_health_check(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert!(matches!(
            event,
            VoiceEngineEvent::Health { .. } | VoiceEngineEvent::Error(_)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
