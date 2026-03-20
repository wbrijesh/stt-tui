use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::event::AppEvent;

const MODEL_NAME: &str = "parakeet-tdt-0.6b-v3-int8";
const MODEL_URL: &str = "https://blob.handy.computer/parakeet-v3-int8.tar.gz";

pub struct LocalEngine {
    model: Option<transcribe_rs::onnx::parakeet::ParakeetModel>,
    model_dir: PathBuf,
}

impl LocalEngine {
    pub fn new() -> Result<Self> {
        let model_dir = Config::models_dir()?.join(MODEL_NAME);
        Ok(Self {
            model: None,
            model_dir,
        })
    }

    pub fn is_model_downloaded(&self) -> bool {
        self.model_dir.exists() && self.model_dir.is_dir()
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn load(&mut self) -> Result<()> {
        if self.model.is_some() {
            return Ok(());
        }
        let model = transcribe_rs::onnx::parakeet::ParakeetModel::load(
            &self.model_dir,
            &transcribe_rs::onnx::Quantization::Int8,
        )
        .context("Failed to load parakeet model")?;
        self.model = Some(model);
        Ok(())
    }

    pub fn unload(&mut self) {
        self.model = None;
    }

    pub fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        let engine = self
            .model
            .as_mut()
            .context("Model not loaded")?;

        let params = transcribe_rs::onnx::parakeet::ParakeetParams {
            timestamp_granularity: Some(
                transcribe_rs::onnx::parakeet::TimestampGranularity::Segment,
            ),
            ..Default::default()
        };

        let result = engine
            .transcribe_with(audio, &params)
            .context("Transcription failed")?;

        Ok(result.text)
    }
}

pub async fn download_model(tx: mpsc::UnboundedSender<AppEvent>) -> Result<()> {
    let models_dir = Config::models_dir()?;
    let final_dir = models_dir.join(MODEL_NAME);

    if final_dir.exists() {
        return Ok(());
    }

    let archive_path = models_dir.join(format!("{}.tar.gz", MODEL_NAME));
    let extract_dir = models_dir.join(format!(".{}.extracting", MODEL_NAME));

    // Download with progress
    let client = reqwest::Client::new();
    let response = client.get(MODEL_URL).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", response.status());
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    let mut file = tokio::fs::File::create(&archive_path).await?;
    let mut stream = response.bytes_stream();

    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut last_progress = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        // Throttle progress events to ~10/sec
        if last_progress.elapsed().as_millis() >= 100 {
            let progress = if total_size > 0 {
                (downloaded as f64 / total_size as f64 * 100.0) as u8
            } else {
                0
            };
            let _ = tx.send(AppEvent::ModelDownloadProgress {
                percent: progress,
                downloaded_mb: downloaded as f64 / 1_048_576.0,
                total_mb: total_size as f64 / 1_048_576.0,
            });
            last_progress = std::time::Instant::now();
        }
    }

    file.flush().await?;
    drop(file);

    let _ = tx.send(AppEvent::ModelDownloadProgress {
        percent: 100,
        downloaded_mb: downloaded as f64 / 1_048_576.0,
        total_mb: total_size as f64 / 1_048_576.0,
    });

    // Extract
    let _ = tx.send(AppEvent::ModelExtracting);

    let archive_path_clone = archive_path.clone();
    let extract_dir_clone = extract_dir.clone();
    let final_dir_clone = final_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        // Clean up any previous failed extraction
        if extract_dir_clone.exists() {
            fs::remove_dir_all(&extract_dir_clone)?;
        }
        fs::create_dir_all(&extract_dir_clone)?;

        let file = fs::File::open(&archive_path_clone)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&extract_dir_clone)?;

        // The archive may contain a single root directory — find it
        let entries: Vec<_> = fs::read_dir(&extract_dir_clone)?
            .filter_map(|e| e.ok())
            .collect();

        if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
            // Single root dir inside archive — move that
            fs::rename(entries[0].path(), &final_dir_clone)?;
            fs::remove_dir_all(&extract_dir_clone)?;
        } else {
            // Files directly in extract dir
            fs::rename(&extract_dir_clone, &final_dir_clone)?;
        }

        // Clean up archive
        fs::remove_file(&archive_path_clone)?;

        Ok(())
    })
    .await??;

    let _ = tx.send(AppEvent::ModelReady);

    Ok(())
}
