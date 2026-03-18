use anyhow::Result;
use futures::StreamExt;
use reqwest::multipart;
use tokio::sync::mpsc;

use crate::event::AppEvent;

pub struct ApiClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ApiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
        }
    }

    pub async fn transcribe(
        &self,
        wav_bytes: Vec<u8>,
        tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<()> {
        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "text");

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let msg = format!("API error {}: {}", status, body);
            let _ = tx.send(AppEvent::ApiError(msg));
            return Ok(());
        }

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        full_text.push_str(&text);
                        let _ = tx.send(AppEvent::TranscriptDelta(full_text.clone()));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::ApiError(format!("Stream error: {}", e)));
                    return Ok(());
                }
            }
        }

        let _ = tx.send(AppEvent::TranscriptComplete(full_text.trim().to_string()));
        Ok(())
    }
}
