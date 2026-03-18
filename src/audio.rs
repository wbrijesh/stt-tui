use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::event::AppEvent;

pub struct AudioManager {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    stream: Option<Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl AudioManager {
    pub fn new(event_tx: mpsc::UnboundedSender<AppEvent>) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No input device available. Check microphone permissions.")?;

        let config = device
            .default_input_config()
            .context("Failed to get default input config")?;

        Ok(Self {
            device,
            config,
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            event_tx,
        })
    }

    pub fn start_recording(&mut self) -> Result<()> {
        // Clear buffer
        {
            let mut buf = self.buffer.lock().unwrap();
            buf.clear();
        }

        let buffer = self.buffer.clone();
        let tx = self.event_tx.clone();
        let sample_format = self.config.sample_format();
        let config = self.config.clone().into();

        let err_tx = self.event_tx.clone();
        let err_fn = move |err: cpal::StreamError| {
            let _ = err_tx.send(AppEvent::ApiError(format!("Audio stream error: {}", err)));
        };

        let level_counter = Arc::new(Mutex::new(0u32));

        let stream = match sample_format {
            SampleFormat::F32 => {
                let level_counter = level_counter.clone();
                self.device.build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut buf = buffer.lock().unwrap();
                        buf.extend_from_slice(data);

                        // Send audio level ~10 times per second
                        let mut counter = level_counter.lock().unwrap();
                        *counter += data.len() as u32;
                        if *counter >= 1600 {
                            *counter = 0;
                            let rms = (data.iter().map(|s| s * s).sum::<f32>()
                                / data.len() as f32)
                                .sqrt();
                            let _ = tx.send(AppEvent::AudioLevel(rms));
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let level_counter = level_counter.clone();
                self.device.build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mut buf = buffer.lock().unwrap();
                        for &sample in data {
                            buf.push(sample as f32 / i16::MAX as f32);
                        }

                        let mut counter = level_counter.lock().unwrap();
                        *counter += data.len() as u32;
                        if *counter >= 1600 {
                            *counter = 0;
                            let rms = (data
                                .iter()
                                .map(|s| {
                                    let f = *s as f32 / i16::MAX as f32;
                                    f * f
                                })
                                .sum::<f32>()
                                / data.len() as f32)
                                .sqrt();
                            let _ = tx.send(AppEvent::AudioLevel(rms));
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            _ => anyhow::bail!("Unsupported sample format: {:?}", sample_format),
        };

        stream.play()?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<()> {
        // Drop the stream to stop recording
        self.stream = None;

        let samples = {
            let buf = self.buffer.lock().unwrap();
            buf.clone()
        };

        let source_rate = self.config.sample_rate().0;
        let source_channels = self.config.channels() as usize;

        // Mix down to mono if stereo
        let mono: Vec<f32> = if source_channels > 1 {
            samples
                .chunks(source_channels)
                .map(|frame| frame.iter().sum::<f32>() / source_channels as f32)
                .collect()
        } else {
            samples
        };

        // Resample to 16kHz if needed
        let target_rate = 16000u32;
        let resampled = if source_rate != target_rate {
            resample(&mono, source_rate, target_rate)
        } else {
            mono
        };

        // Encode as WAV
        let wav_bytes = encode_wav(&resampled, target_rate)?;

        self.event_tx
            .send(AppEvent::RecordingComplete(wav_bytes))
            .ok();

        Ok(())
    }
}

fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < samples.len() {
            samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac
        } else if idx < samples.len() {
            samples[idx] as f64
        } else {
            0.0
        };
        output.push(sample as f32);
    }

    output
}

fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    let num_samples = samples.len() as u32;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 1;
    let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample) / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = num_samples * u32::from(bits_per_sample) / 8;
    let file_size = 36 + data_size;

    let mut buf: Vec<u8> = Vec::with_capacity(file_size as usize + 8);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt subchunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // subchunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data subchunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample = (clamped * i16::MAX as f32) as i16;
        buf.extend_from_slice(&int_sample.to_le_bytes());
    }

    Ok(buf)
}
