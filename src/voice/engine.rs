use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::voice::pack::{VoicePackHealth, VoicePackManifest};
use crate::voice::preferences::VoiceEngineMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceEngineRequest {
    Start { sample_rate_hz: u32 },
    AudioPcm16(Vec<i16>),
    Stop,
    Health,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceEngineEvent {
    Ready,
    Partial(String),
    Final(String),
    Error(String),
    Health { ok: bool, detail: String },
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

pub fn frame_from_request(request: &VoiceEngineRequest) -> FramedMessage {
    match request {
        VoiceEngineRequest::Start { sample_rate_hz } => {
            FramedMessage::new("start", sample_rate_hz.to_string())
        }
        VoiceEngineRequest::AudioPcm16(samples) => {
            FramedMessage::new("audio-pcm16-hex", pcm16_samples_to_hex(samples))
        }
        VoiceEngineRequest::Stop => FramedMessage::new("stop", ""),
        VoiceEngineRequest::Health => FramedMessage::new("health", ""),
        VoiceEngineRequest::Shutdown => FramedMessage::new("shutdown", ""),
    }
}

pub fn event_from_frame(frame: &FramedMessage) -> VoiceEngineEvent {
    match frame.kind.as_str() {
        "ready" => VoiceEngineEvent::Ready,
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

pub struct VoiceEngineProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

pub fn run_voice_engine_health_check(
    manifest: &VoicePackManifest,
    health: VoicePackHealth,
    engine_mode: VoiceEngineMode,
) -> io::Result<VoiceEngineEvent> {
    let mut process = VoiceEngineProcess::launch(manifest, health, engine_mode)?;
    let _ = process.read_event()?;
    process.send(&VoiceEngineRequest::Health)?;
    let event = loop {
        match process.read_event()? {
            Some(event @ VoiceEngineEvent::Health { .. }) => break event,
            Some(event @ VoiceEngineEvent::Error(_)) => break event,
            Some(_) => continue,
            None => {
                break VoiceEngineEvent::Error("voice engine exited during health check".into());
            }
        }
    };
    let _ = process.shutdown();
    Ok(event)
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
            .stderr(Stdio::inherit())
            .env("TERMINALTILER_PARAKEET_MODEL", &manifest.model_name)
            .env("TERMINALTILER_VOICE_MODEL_PATH", model_path)
            .env("TERMINALTILER_VOICE_ENGINE_MODE", engine_mode.env_value());

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("voice engine stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("voice engine stdout unavailable"))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    pub fn send(&mut self, request: &VoiceEngineRequest) -> io::Result<()> {
        frame_from_request(request).encode(&mut self.stdin)?;
        self.stdin.flush()
    }

    pub fn read_event(&mut self) -> io::Result<Option<VoiceEngineEvent>> {
        Ok(read_framed_message(&mut self.stdout)?.map(|frame| event_from_frame(&frame)))
    }

    pub fn shutdown(mut self) -> io::Result<()> {
        let _ = self.send(&VoiceEngineRequest::Shutdown);
        let _ = self.child.wait();
        Ok(())
    }
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
            Some(VoiceEngineEvent::Partial(text)) if text.contains("Captured")
        ));
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

        let mut saw_transcribing_partial = false;
        let mut saw_finalized_partial = false;
        let final_text = loop {
            let event = read_framed_message(&mut stdout)
                .unwrap()
                .map(|frame| event_from_frame(&frame))
                .expect("helper should emit final transcript");
            match event {
                VoiceEngineEvent::Partial(text) => {
                    saw_transcribing_partial |= text.contains("Transcribing");
                    saw_finalized_partial |= text.contains("finalized in");
                }
                VoiceEngineEvent::Final(text) => break text,
                VoiceEngineEvent::Ready => continue,
                other => panic!("unexpected helper event: {other:?}"),
            }
        };

        assert!(saw_transcribing_partial);
        assert!(saw_finalized_partial);
        assert_eq!(final_text, "fake parakeet transcript");
        frame_from_request(&VoiceEngineRequest::Shutdown)
            .encode(&mut stdin)
            .unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_python_helper_reports_fake_runtime_health_diagnostics() {
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
        assert!(detail.contains("device=cpu"));
        assert!(detail.contains("quantized=True"));
        assert!(detail.contains("torch=2.12.fake"));
        assert!(detail.contains("nemo=2.7.fake"));
        assert!(detail.contains(&manifest.model_name));
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
