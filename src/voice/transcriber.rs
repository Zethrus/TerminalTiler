use std::io;

use crate::voice::audio::{AudioCapture, AudioCaptureError};
use crate::voice::engine::{
    VoiceEngineCapabilities, VoiceEngineEvent, VoiceEngineProcess, VoiceEngineRequest,
};
use crate::voice::pack::{VoicePackHealth, VoicePackManifest};
use crate::voice::preferences::VoiceEngineMode;

pub struct ParakeetTranscriber {
    engine: VoiceEngineProcess,
    capture: Option<AudioCapture>,
}

#[derive(Debug)]
pub enum ParakeetTranscriberError {
    Audio(AudioCaptureError),
    EngineIo(String),
    Engine(String),
    EngineExited,
}

impl From<AudioCaptureError> for ParakeetTranscriberError {
    fn from(error: AudioCaptureError) -> Self {
        Self::Audio(error)
    }
}

impl From<io::Error> for ParakeetTranscriberError {
    fn from(error: io::Error) -> Self {
        Self::EngineIo(error.to_string())
    }
}

impl ParakeetTranscriber {
    pub fn launch(
        manifest: &VoicePackManifest,
        health: VoicePackHealth,
        engine_mode: VoiceEngineMode,
    ) -> Result<Self, ParakeetTranscriberError> {
        let mut engine = VoiceEngineProcess::launch(manifest, health, engine_mode)?;
        // Consume the helper's startup ready frame when present. If the process
        // exits before readiness, the first later request will surface the error.
        let _ = engine.read_event()?;
        Ok(Self {
            engine,
            capture: None,
        })
    }

    pub fn warm_up(&mut self) -> Result<(), ParakeetTranscriberError> {
        self.engine
            .send(&VoiceEngineRequest::Warm)
            .map_err(ParakeetTranscriberError::from)?;
        loop {
            match self.engine.read_event()? {
                Some(VoiceEngineEvent::Health { ok: true, .. }) => return Ok(()),
                Some(VoiceEngineEvent::Health { detail, .. }) => {
                    return Err(ParakeetTranscriberError::Engine(detail));
                }
                Some(VoiceEngineEvent::Error(message)) => {
                    if message.trim() == "unknown command: warm" {
                        return self.warm_up_legacy_health();
                    }
                    return Err(ParakeetTranscriberError::Engine(message));
                }
                Some(_) => continue,
                None => return Err(ParakeetTranscriberError::EngineExited),
            }
        }
    }

    fn warm_up_legacy_health(&mut self) -> Result<(), ParakeetTranscriberError> {
        self.engine
            .send(&VoiceEngineRequest::Health)
            .map_err(ParakeetTranscriberError::from)?;
        loop {
            match self.engine.read_event()? {
                Some(VoiceEngineEvent::Health { ok: true, .. }) => return Ok(()),
                Some(VoiceEngineEvent::Health { detail, .. }) => {
                    return Err(ParakeetTranscriberError::Engine(detail));
                }
                Some(VoiceEngineEvent::Error(message)) => {
                    return Err(ParakeetTranscriberError::Engine(message));
                }
                Some(_) => continue,
                None => return Err(ParakeetTranscriberError::EngineExited),
            }
        }
    }

    pub fn capabilities(&mut self) -> Result<VoiceEngineCapabilities, ParakeetTranscriberError> {
        self.engine
            .send(&VoiceEngineRequest::Capabilities)
            .map_err(ParakeetTranscriberError::from)?;
        loop {
            match self.engine.read_event()? {
                Some(VoiceEngineEvent::Capabilities(capabilities)) => return Ok(capabilities),
                Some(VoiceEngineEvent::Error(message)) => {
                    if message.trim() == "unknown command: capabilities" {
                        return Ok(VoiceEngineCapabilities::legacy_warm());
                    }
                    return Err(ParakeetTranscriberError::Engine(message));
                }
                Some(_) => continue,
                None => return Err(ParakeetTranscriberError::EngineExited),
            }
        }
    }

    pub fn start_capture(
        &mut self,
        microphone_id: Option<&str>,
    ) -> Result<(), ParakeetTranscriberError> {
        self.capture = Some(AudioCapture::start(microphone_id)?);
        self.engine
            .send(&VoiceEngineRequest::Start {
                sample_rate_hz: 16_000,
            })
            .map_err(ParakeetTranscriberError::from)
    }

    pub fn stop_capture_and_transcribe(&mut self) -> Result<String, ParakeetTranscriberError> {
        self.stop_capture_and_transcribe_with_partials(|_| {})
    }

    pub fn stop_capture_and_transcribe_with_partials(
        &mut self,
        on_partial: impl FnMut(String),
    ) -> Result<String, ParakeetTranscriberError> {
        let Some(capture) = self.capture.take() else {
            return self.transcribe_pcm16_with_partials(Vec::new(), on_partial);
        };
        self.finish_started_capture(capture.take_pcm16_mono_16khz(), on_partial)
    }

    pub fn flush_captured_audio(&mut self) -> Result<Option<String>, ParakeetTranscriberError> {
        let Some(capture) = self.capture.as_ref() else {
            return Ok(None);
        };
        let samples = capture.take_pcm16_mono_16khz();
        if samples.is_empty() {
            return Ok(None);
        }
        self.engine
            .send(&VoiceEngineRequest::AudioPcm16(samples))
            .map_err(ParakeetTranscriberError::from)?;
        loop {
            match self.engine.read_event()? {
                Some(VoiceEngineEvent::Partial(text)) => return Ok(Some(text)),
                Some(VoiceEngineEvent::Error(message)) => {
                    return Err(ParakeetTranscriberError::Engine(message));
                }
                Some(_) => continue,
                None => return Err(ParakeetTranscriberError::EngineExited),
            }
        }
    }

    pub fn transcribe_pcm16(
        &mut self,
        samples: Vec<i16>,
    ) -> Result<String, ParakeetTranscriberError> {
        self.transcribe_pcm16_with_partials(samples, |_| {})
    }

    pub fn transcribe_pcm16_with_partials(
        &mut self,
        samples: Vec<i16>,
        on_partial: impl FnMut(String),
    ) -> Result<String, ParakeetTranscriberError> {
        self.engine
            .send(&VoiceEngineRequest::Start {
                sample_rate_hz: 16_000,
            })
            .map_err(ParakeetTranscriberError::from)?;
        if !samples.is_empty() {
            self.engine
                .send(&VoiceEngineRequest::FinalAudioPcm16(samples))
                .map_err(ParakeetTranscriberError::from)?;
        }
        self.read_final(on_partial)
    }

    fn finish_started_capture(
        &mut self,
        samples: Vec<i16>,
        on_partial: impl FnMut(String),
    ) -> Result<String, ParakeetTranscriberError> {
        if !samples.is_empty() {
            self.engine
                .send(&VoiceEngineRequest::FinalAudioPcm16(samples))
                .map_err(ParakeetTranscriberError::from)?;
        }
        self.read_final(on_partial)
    }

    fn read_final(
        &mut self,
        mut on_partial: impl FnMut(String),
    ) -> Result<String, ParakeetTranscriberError> {
        self.engine
            .send(&VoiceEngineRequest::Stop)
            .map_err(ParakeetTranscriberError::from)?;

        loop {
            match self.engine.read_event()? {
                Some(VoiceEngineEvent::Partial(text)) => {
                    on_partial(text);
                }
                Some(VoiceEngineEvent::Final(text)) => return Ok(text),
                Some(VoiceEngineEvent::Error(message)) => {
                    return Err(ParakeetTranscriberError::Engine(message));
                }
                Some(_) => continue,
                None => return Err(ParakeetTranscriberError::EngineExited),
            }
        }
    }

    pub fn shutdown(self) -> Result<(), ParakeetTranscriberError> {
        self.engine.shutdown()?;
        Ok(())
    }

    pub fn engine_process_id(&self) -> u32 {
        self.engine.process_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_helper_returns_empty_final_for_empty_capture_without_loading_nemo() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-transcriber-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut transcriber =
            ParakeetTranscriber::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(transcriber.transcribe_pcm16(Vec::new()).unwrap(), "");
        transcriber.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_transcription_path_accepts_partial_callback() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-transcriber-partial-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut transcriber =
            ParakeetTranscriber::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();
        let mut partials = Vec::new();

        assert_eq!(
            transcriber
                .transcribe_pcm16_with_partials(Vec::new(), |partial| partials.push(partial))
                .unwrap(),
            ""
        );
        assert!(partials.is_empty());
        transcriber.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn flush_without_active_capture_is_noop() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-transcriber-flush-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut transcriber =
            ParakeetTranscriber::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        assert_eq!(transcriber.flush_captured_audio().unwrap(), None);
        transcriber.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reuses_resident_helper_between_consecutive_dictations() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-transcriber-resident-{}",
            uuid::Uuid::new_v4()
        ));
        let manifest = crate::voice::pack::install_builtin_parakeet_pack(&root).unwrap();
        let health = crate::voice::pack::health_check(&root, &manifest);
        let mut transcriber =
            ParakeetTranscriber::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();
        let process_id = transcriber.engine_process_id();

        assert_eq!(transcriber.transcribe_pcm16(Vec::new()).unwrap(), "");
        assert_eq!(transcriber.engine_process_id(), process_id);
        assert_eq!(transcriber.transcribe_pcm16(Vec::new()).unwrap(), "");
        assert_eq!(transcriber.engine_process_id(), process_id);

        transcriber.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_helpers_without_capabilities_are_treated_as_warm_offline_helpers() {
        let root = std::env::temp_dir().join(format!(
            "terminaltiler-transcriber-legacy-capabilities-{}",
            uuid::Uuid::new_v4()
        ));
        let pack_root = root.join("legacy").join("1");
        std::fs::create_dir_all(pack_root.join("model")).unwrap();
        let engine_path = pack_root.join("legacy_engine.py");
        std::fs::write(
            &engine_path,
            r#"#!/usr/bin/env python3
import sys
def emit(kind, payload=""):
    print(f"{kind} {payload}" if payload else kind, flush=True)
emit("ready", "legacy-helper")
for raw in sys.stdin:
    kind = raw.strip().split(" ", 1)[0]
    if kind == "health":
        emit("health", "ok: legacy model loaded")
    elif kind == "warm":
        emit("error", "unknown command: warm")
    elif kind == "capabilities":
        emit("error", "unknown command: capabilities")
    elif kind == "shutdown":
        emit("ready", "shutdown")
        raise SystemExit(0)
    else:
        emit("error", f"unexpected command: {kind}")
"#,
        )
        .unwrap();
        let manifest = VoicePackManifest {
            id: "legacy".into(),
            version: "1".into(),
            engine_executable: "legacy_engine.py".into(),
            model_path: "model".into(),
            archive_url: "builtin://legacy".into(),
            archive_sha256: "builtin".into(),
            model_name: "legacy/offline".into(),
            streaming_model_name: "legacy/streaming".into(),
            python_requirements: Vec::new(),
        };
        let health = VoicePackHealth::Ready {
            engine_path,
            model_path: pack_root.join("model"),
        };
        let mut transcriber =
            ParakeetTranscriber::launch(&manifest, health, VoiceEngineMode::Cpu).unwrap();

        transcriber.warm_up().unwrap();
        assert_eq!(
            transcriber.capabilities().unwrap(),
            VoiceEngineCapabilities::legacy_warm()
        );

        transcriber.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
