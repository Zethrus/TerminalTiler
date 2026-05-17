use std::io;

use crate::voice::audio::{AudioCapture, AudioCaptureError};
use crate::voice::engine::{VoiceEngineEvent, VoiceEngineProcess, VoiceEngineRequest};
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
                .send(&VoiceEngineRequest::AudioPcm16(samples))
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
                .send(&VoiceEngineRequest::AudioPcm16(samples))
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
}
