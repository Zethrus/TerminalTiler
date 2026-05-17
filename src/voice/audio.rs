#[cfg(feature = "voice-cpal")]
mod imp {
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::{AudioCaptureError, MicrophoneDevice};

    pub struct AudioCapture {
        _stream: cpal::Stream,
        captured_samples: Arc<Mutex<Vec<i16>>>,
    }

    impl AudioCapture {
        pub fn enumerate_microphones() -> Result<Vec<MicrophoneDevice>, AudioCaptureError> {
            let host = cpal::default_host();
            let default_id = host
                .default_input_device()
                .and_then(|device| device.id().ok())
                .map(|id| id.to_string());
            let devices = host
                .input_devices()
                .map_err(|error| AudioCaptureError::BackendUnavailable(error.to_string()))?;

            let mut microphones = Vec::new();
            for (index, device) in devices.enumerate() {
                let description = device.description().ok();
                let name = description
                    .as_ref()
                    .map(|description| description.name().to_string())
                    .unwrap_or_else(|| format!("Input device {}", index + 1));
                let id = device
                    .id()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|_| name.clone());
                microphones.push(MicrophoneDevice {
                    is_default: default_id.as_deref() == Some(id.as_str()),
                    id,
                    name,
                });
            }
            Ok(microphones)
        }

        pub fn start(microphone_id: Option<&str>) -> Result<Self, AudioCaptureError> {
            let host = cpal::default_host();
            let device = select_input_device(&host, microphone_id)?;
            let supported_config = device
                .default_input_config()
                .map_err(|error| AudioCaptureError::UnsupportedFormat(error.to_string()))?;
            let input_sample_rate = supported_config.sample_rate();
            let channels = usize::from(supported_config.channels()).max(1);
            let stream_config: cpal::StreamConfig = supported_config.clone().into();
            let captured_samples = Arc::new(Mutex::new(Vec::new()));
            let sink = captured_samples.clone();
            let error_callback =
                |error| eprintln!("TerminalTiler voice input stream error: {error}");

            let stream = match supported_config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        append_resampled_frames(&sink, data, channels, input_sample_rate)
                    },
                    error_callback,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        append_resampled_frames(&sink, data, channels, input_sample_rate)
                    },
                    error_callback,
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        append_resampled_frames(&sink, data, channels, input_sample_rate)
                    },
                    error_callback,
                    None,
                ),
                other => {
                    return Err(AudioCaptureError::UnsupportedFormat(format!(
                        "unsupported input sample format {other:?}"
                    )));
                }
            }
            .map_err(|error| AudioCaptureError::StreamBuild(error.to_string()))?;

            stream
                .play()
                .map_err(|error| AudioCaptureError::StreamPlay(error.to_string()))?;

            Ok(Self {
                _stream: stream,
                captured_samples,
            })
        }

        pub fn take_pcm16_mono_16khz(&self) -> Vec<i16> {
            self.captured_samples
                .lock()
                .map(|mut captured| std::mem::take(&mut *captured))
                .unwrap_or_default()
        }
    }

    fn select_input_device(
        host: &cpal::Host,
        microphone_id: Option<&str>,
    ) -> Result<cpal::Device, AudioCaptureError> {
        if let Some(requested) = microphone_id.filter(|value| !value.trim().is_empty()) {
            let devices = host
                .input_devices()
                .map_err(|error| AudioCaptureError::BackendUnavailable(error.to_string()))?;
            for device in devices {
                let id_matches = device
                    .id()
                    .map(|id| id.to_string() == requested)
                    .unwrap_or(false);
                let name_matches = device
                    .description()
                    .map(|description| description.name() == requested)
                    .unwrap_or(false);
                if id_matches || name_matches {
                    return Ok(device);
                }
            }
            return Err(AudioCaptureError::DeviceUnavailable(requested.into()));
        }

        host.default_input_device()
            .ok_or_else(|| AudioCaptureError::DeviceUnavailable("default input device".into()))
    }

    fn append_resampled_frames<T>(
        sink: &Arc<Mutex<Vec<i16>>>,
        data: &[T],
        channels: usize,
        input_sample_rate: u32,
    ) where
        T: PcmSample,
    {
        let pcm = data.iter().map(PcmSample::to_i16).collect::<Vec<_>>();
        let mono = super::normalize_to_mono_16khz(&pcm, channels);
        let resampled = super::resample_mono_to_16khz(&mono, input_sample_rate);
        if let Ok(mut captured) = sink.try_lock() {
            captured.extend(resampled);
        }
    }

    trait PcmSample {
        fn to_i16(&self) -> i16;
    }

    impl PcmSample for i16 {
        fn to_i16(&self) -> i16 {
            *self
        }
    }

    impl PcmSample for u16 {
        fn to_i16(&self) -> i16 {
            (*self as i32 - 32_768) as i16
        }
    }

    impl PcmSample for f32 {
        fn to_i16(&self) -> i16 {
            (self.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
        }
    }
}

#[cfg(not(feature = "voice-cpal"))]
mod imp {
    use super::{AudioCaptureError, MicrophoneDevice};

    pub struct AudioCapture;

    impl AudioCapture {
        pub fn enumerate_microphones() -> Result<Vec<MicrophoneDevice>, AudioCaptureError> {
            Ok(Vec::new())
        }

        pub fn start(_microphone_id: Option<&str>) -> Result<Self, AudioCaptureError> {
            Err(AudioCaptureError::BackendUnavailable(
                "built without voice-cpal feature".into(),
            ))
        }

        pub fn take_pcm16_mono_16khz(&self) -> Vec<i16> {
            Vec::new()
        }
    }
}

pub use imp::AudioCapture;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicrophoneDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioCaptureError {
    BackendUnavailable(String),
    DeviceUnavailable(String),
    UnsupportedFormat(String),
    StreamBuild(String),
    StreamPlay(String),
}

pub fn normalize_to_mono_16khz(input: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return input.to_vec();
    }
    input
        .chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|sample| i32::from(*sample)).sum();
            (sum / frame.len() as i32) as i16
        })
        .collect()
}

pub fn resample_mono_to_16khz(input: &[i16], input_sample_rate: u32) -> Vec<i16> {
    const OUTPUT_RATE: u32 = 16_000;
    if input_sample_rate == OUTPUT_RATE || input.is_empty() {
        return input.to_vec();
    }
    let output_len =
        ((input.len() as u64 * OUTPUT_RATE as u64) / input_sample_rate as u64).max(1) as usize;
    let ratio = input_sample_rate as f64 / OUTPUT_RATE as f64;
    (0..output_len)
        .map(|index| {
            let source = index as f64 * ratio;
            let lower = source.floor() as usize;
            let upper = (lower + 1).min(input.len() - 1);
            let fraction = source - lower as f64;
            let lower_sample = input[lower] as f64;
            let upper_sample = input[upper] as f64;
            (lower_sample + (upper_sample - lower_sample) * fraction).round() as i16
        })
        .collect()
}

impl AudioCapture {
    pub fn normalize_to_mono_16khz(input: &[i16], channels: usize) -> Vec<i16> {
        normalize_to_mono_16khz(input, channels)
    }

    pub fn resample_mono_to_16khz(input: &[i16], input_sample_rate: u32) -> Vec<i16> {
        resample_mono_to_16khz(input, input_sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::AudioCapture;

    #[test]
    fn downmixes_interleaved_stereo_to_mono() {
        assert_eq!(
            AudioCapture::normalize_to_mono_16khz(&[100, 300, -100, 100], 2),
            vec![200, 0]
        );
    }

    #[test]
    fn resamples_mono_audio_to_16khz() {
        let input = (0..48_000).map(|value| value as i16).collect::<Vec<_>>();
        let output = AudioCapture::resample_mono_to_16khz(&input, 48_000);
        assert_eq!(output.len(), 16_000);
        assert_eq!(output[0], 0);
        assert_eq!(output[1], 3);
    }
}
