use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::api::ApiClient;
use crate::audio::AudioManager;
use crate::config::{Backend, Config};
use crate::event::AppEvent;
use crate::local::LocalEngine;
use crate::storage::{fetch_usage_stats, Database, Transcript, UsageStats};

const YANK_FEEDBACK_TICKS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Setup,
    Idle,
    Recording,
    Transcribing,
    Downloading,
    Error(String),
}

pub struct App {
    pub state: AppState,
    pub backend: Backend,
    pub transcripts: Vec<Transcript>,
    pub current_index: usize,
    pub current_partial: String,
    pub audio_level: f32,
    pub level_history: Vec<f32>,
    pub recording_start: Option<std::time::Instant>,
    pub recording_duration_ms: u64,
    pub tick_count: u64,
    pub yank_ticks_remaining: u64,
    pub show_help: bool,
    pub show_stats: bool,
    pub show_mic_picker: bool,
    pub show_backend_picker: bool,
    pub mic_devices: Vec<String>,
    pub mic_selected: usize,
    pub mic_current_name: String,
    pub backend_selected: usize,
    pub usage_stats: Option<UsageStats>,
    pub should_quit: bool,
    pub api_key_input: String,
    pub setup_error: Option<String>,
    pub total_cost_usd: f64,
    pub model_loaded: bool,
    pub download_progress: Option<(u8, f64, f64)>, // percent, downloaded_mb, total_mb
    pub model_extracting: bool,

    audio_manager: Option<Arc<Mutex<AudioManager>>>,
    api_client: Option<Arc<ApiClient>>,
    local_engine: Option<Arc<Mutex<LocalEngine>>>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    model: String,
    db: Option<Arc<Database>>,
    pending_duration_ms: u64,
}

impl App {
    pub fn new(
        config: Option<Config>,
        audio_manager: Option<AudioManager>,
        db: Option<Database>,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        let (state, api_client, model, backend) = match &config {
            Some(cfg) => {
                let model = cfg.model.clone();
                let backend = cfg.backend.clone();
                let client = if !cfg.api_key.is_empty() {
                    Some(Arc::new(ApiClient::new(
                        cfg.api_key.clone(),
                        cfg.model.clone(),
                    )))
                } else {
                    None
                };
                let state = if backend == Backend::Openai && cfg.api_key.is_empty() {
                    AppState::Setup
                } else {
                    AppState::Idle
                };
                (state, client, model, backend)
            }
            None => (
                AppState::Idle,
                None,
                "gpt-4o-mini-transcribe".to_string(),
                Backend::Local,
            ),
        };

        let local_engine = LocalEngine::new().ok().map(|e| Arc::new(Mutex::new(e)));

        let (transcripts, total_cost_usd) = match &db {
            Some(db) => {
                let t = db.active_transcripts().unwrap_or_default();
                let c = db.total_cost().unwrap_or(0.0);
                (t, c)
            }
            None => (Vec::new(), 0.0),
        };

        let current_index = if transcripts.is_empty() {
            0
        } else {
            transcripts.len() - 1
        };

        let mic_current_name = match &audio_manager {
            Some(m) => m.device_name(),
            None => "None".to_string(),
        };

        let model_loaded = local_engine
            .as_ref()
            .map(|e| e.lock().unwrap().is_loaded())
            .unwrap_or(false);

        let backend_selected = match backend {
            Backend::Local => 0,
            Backend::Openai => 1,
        };

        Self {
            state,
            backend,
            transcripts,
            current_index,
            current_partial: String::new(),
            audio_level: 0.0,
            level_history: Vec::new(),
            recording_start: None,
            recording_duration_ms: 0,
            tick_count: 0,
            yank_ticks_remaining: 0,
            show_help: false,
            show_stats: false,
            show_mic_picker: false,
            show_backend_picker: false,
            mic_devices: Vec::new(),
            mic_selected: 0,
            mic_current_name,
            backend_selected,
            usage_stats: None,
            should_quit: false,
            api_key_input: String::new(),
            setup_error: None,
            total_cost_usd,
            model_loaded,
            download_progress: None,
            model_extracting: false,
            audio_manager: audio_manager.map(|m| Arc::new(Mutex::new(m))),
            api_client,
            local_engine,
            event_tx,
            model,
            db: db.map(Arc::new),
            pending_duration_ms: 0,
        }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::AudioLevel(level) => {
                self.audio_level = level;
                self.level_history.push(level);
                if self.level_history.len() > 200 {
                    self.level_history.remove(0);
                }
            }
            AppEvent::RecordingComplete(wav_bytes) => {
                self.pending_duration_ms = self.recording_duration_ms;
                self.audio_level = 0.0;

                match self.backend {
                    Backend::Openai => {
                        self.state = AppState::Transcribing;
                        self.current_partial.clear();
                        if let Some(client) = &self.api_client {
                            let client = client.clone();
                            let tx = self.event_tx.clone();
                            let dur = self.pending_duration_ms;
                            tokio::spawn(async move {
                                if let Err(e) =
                                    client.transcribe(wav_bytes, tx.clone(), dur).await
                                {
                                    let _ = tx.send(AppEvent::ApiError(e.to_string()));
                                }
                            });
                        }
                    }
                    Backend::Local => {
                        self.state = AppState::Transcribing;
                        self.current_partial.clear();
                        self.transcribe_local(wav_bytes);
                    }
                }
            }
            AppEvent::TranscriptDelta(text) => {
                self.current_partial = text;
            }
            AppEvent::TranscriptComplete { text, duration_ms } => {
                if !text.is_empty() {
                    let mut transcript = Transcript::new(text, duration_ms);
                    // Local transcriptions are free
                    if self.backend == Backend::Local {
                        transcript.cost_usd = 0.0;
                    }
                    if let Some(db) = &self.db {
                        match db.insert(&transcript) {
                            Ok(_) => {
                                self.total_cost_usd += transcript.cost_usd;
                            }
                            Err(e) => {
                                let _ = self
                                    .event_tx
                                    .send(AppEvent::ApiError(format!("DB save error: {}", e)));
                            }
                        }
                    }
                    self.transcripts.push(transcript);
                    self.current_index = self.transcripts.len() - 1;
                }
                self.current_partial.clear();
                self.state = AppState::Idle;
                // Unload local model immediately after transcription
                if self.backend == Backend::Local && self.model_loaded {
                    self.unload_local_model();
                }
            }
            AppEvent::ModelDownloadProgress {
                percent,
                downloaded_mb,
                total_mb,
            } => {
                self.download_progress = Some((percent, downloaded_mb, total_mb));
            }
            AppEvent::ModelExtracting => {
                self.model_extracting = true;
            }
            AppEvent::ModelReady => {
                self.download_progress = None;
                self.model_extracting = false;
                self.state = AppState::Idle;
            }
            AppEvent::ModelLoaded => {
                self.model_loaded = true;
            }
            AppEvent::ModelError(msg) => {
                self.download_progress = None;
                self.model_extracting = false;
                self.state = AppState::Error(msg);
            }
            AppEvent::ApiError(msg) => {
                self.state = AppState::Error(msg);
                self.current_partial.clear();
            }
            AppEvent::Tick => {
                self.tick_count = self.tick_count.wrapping_add(1);
                if self.yank_ticks_remaining > 0 {
                    self.yank_ticks_remaining -= 1;
                }
                if let AppState::Recording = self.state {
                    if let Some(start) = self.recording_start {
                        self.recording_duration_ms = start.elapsed().as_millis() as u64;
                    }
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        if self.state == AppState::Setup {
            self.handle_setup_key(key);
            return;
        }

        if self.show_help {
            self.show_help = false;
            return;
        }

        if self.show_stats {
            self.show_stats = false;
            return;
        }

        if self.show_mic_picker {
            self.handle_mic_picker_key(key);
            return;
        }

        if self.show_backend_picker {
            self.handle_backend_picker_key(key);
            return;
        }

        if let AppState::Error(_) = &self.state {
            self.state = AppState::Idle;
            return;
        }

        match key.code {
            KeyCode::Esc => {
                if self.state == AppState::Recording {
                    self.cancel_recording();
                } else if self.state == AppState::Idle {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('b') => {
                if self.state == AppState::Idle {
                    self.open_backend_picker();
                }
            }
            KeyCode::Char('m') => {
                if self.state == AppState::Idle {
                    self.open_mic_picker();
                }
            }
            KeyCode::Char('S') => {
                if self.state == AppState::Idle {
                    if let Some(db) = &self.db {
                        match fetch_usage_stats(db) {
                            Ok(stats) => {
                                self.usage_stats = Some(stats);
                                self.show_stats = true;
                            }
                            Err(e) => {
                                self.state = AppState::Error(format!("Stats error: {}", e));
                            }
                        }
                    }
                }
            }
            KeyCode::Char('q') => {
                if self.state == AppState::Idle {
                    self.should_quit = true;
                }
            }
            KeyCode::Char(' ') => {
                self.toggle_recording();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if self.state == AppState::Idle && !self.transcripts.is_empty() {
                    self.current_index = self.current_index.saturating_sub(1);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.state == AppState::Idle && !self.transcripts.is_empty() {
                    self.current_index =
                        (self.current_index + 1).min(self.transcripts.len() - 1);
                }
            }
            KeyCode::Char('y') => {
                if self.state == AppState::Idle && !self.transcripts.is_empty() {
                    self.yank_current();
                }
            }
            KeyCode::Char('d') => {
                if self.state == AppState::Idle && !self.transcripts.is_empty() {
                    self.delete_current();
                }
            }
            KeyCode::Char('D') => {
                if self.state == AppState::Idle && !self.transcripts.is_empty() {
                    self.delete_all();
                }
            }
            _ => {}
        }
    }

    fn handle_setup_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let api_key = self.api_key_input.trim().to_string();
                if api_key.is_empty() {
                    self.setup_error = Some("API key cannot be empty".to_string());
                    return;
                }

                let config = Config {
                    backend: self.backend.clone(),
                    api_key: api_key.clone(),
                    model: self.model.clone(),
                };

                if let Err(e) = config.save() {
                    self.setup_error = Some(format!("Failed to save config: {}", e));
                    return;
                }

                self.api_client =
                    Some(Arc::new(ApiClient::new(api_key, self.model.clone())));
                self.setup_error = None;
                self.api_key_input.clear();
                self.state = AppState::Idle;
            }
            KeyCode::Char(c) => {
                self.api_key_input.push(c);
                self.setup_error = None;
            }
            KeyCode::Backspace => {
                self.api_key_input.pop();
                self.setup_error = None;
            }
            KeyCode::Esc => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    // -- Backend picker --

    fn open_backend_picker(&mut self) {
        self.backend_selected = match self.backend {
            Backend::Local => 0,
            Backend::Openai => 1,
        };
        self.show_backend_picker = true;
    }

    fn handle_backend_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.backend_selected = (self.backend_selected + 1).min(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.backend_selected = self.backend_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.show_backend_picker = false;
                let new_backend = if self.backend_selected == 0 {
                    Backend::Local
                } else {
                    Backend::Openai
                };
                self.set_backend(new_backend);
            }
            KeyCode::Esc | KeyCode::Char('b') => {
                self.show_backend_picker = false;
            }
            _ => {}
        }
    }

    fn set_backend(&mut self, backend: Backend) {
        if self.backend == backend {
            return;
        }
        self.backend = backend.clone();

        // Persist to config
        if let Ok(Some(mut config)) = Config::load() {
            config.backend = backend.clone();
            let _ = config.save();
        } else {
            let config = Config {
                backend: backend.clone(),
                api_key: String::new(),
                model: self.model.clone(),
            };
            let _ = config.save();
        }

        // If switching to OpenAI and no API key, show setup
        if backend == Backend::Openai && self.api_client.is_none() {
            self.state = AppState::Setup;
        }
    }

    // -- Mic picker --

    fn open_mic_picker(&mut self) {
        let devices = AudioManager::list_devices();
        if devices.is_empty() {
            self.state = AppState::Error("No input devices found".to_string());
            return;
        }
        let current_idx = devices
            .iter()
            .position(|name| name == &self.mic_current_name)
            .unwrap_or(0);
        self.mic_devices = devices;
        self.mic_selected = current_idx;
        self.show_mic_picker = true;
    }

    fn handle_mic_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.mic_selected + 1 < self.mic_devices.len() {
                    self.mic_selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mic_selected = self.mic_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.select_mic(self.mic_selected);
                self.show_mic_picker = false;
            }
            KeyCode::Esc | KeyCode::Char('m') => {
                self.show_mic_picker = false;
            }
            _ => {}
        }
    }

    fn select_mic(&mut self, index: usize) {
        if let Some(mgr) = &self.audio_manager {
            let mut mgr = mgr.lock().unwrap();
            match mgr.set_device_by_index(index) {
                Ok(()) => {
                    self.mic_current_name = mgr.device_name();
                }
                Err(e) => {
                    self.state = AppState::Error(format!("Failed to set mic: {}", e));
                }
            }
        }
    }

    // -- Local transcription --

    fn transcribe_local(&mut self, wav_bytes: Vec<u8>) {
        let engine = match &self.local_engine {
            Some(e) => e.clone(),
            None => {
                self.state = AppState::Error("Local engine not available".to_string());
                return;
            }
        };

        // Check if model is downloaded
        {
            let e = engine.lock().unwrap();
            if !e.is_model_downloaded() {
                // Need to download first
                self.state = AppState::Downloading;
                let tx = self.event_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::local::download_model(tx.clone()).await {
                        let _ = tx.send(AppEvent::ModelError(format!("Download failed: {}", e)));
                    }
                });
                // Re-queue the recording for after download
                // For now, user will need to record again
                return;
            }
        }

        let tx = self.event_tx.clone();
        let dur = self.pending_duration_ms;

        // Load model + transcribe in a blocking task
        tokio::task::spawn_blocking(move || {
            let mut e = engine.lock().unwrap();

            // Load if not loaded
            if !e.is_loaded() {
                if let Err(err) = e.load() {
                    let _ = tx.send(AppEvent::ApiError(format!("Model load error: {}", err)));
                    return;
                }
                let _ = tx.send(AppEvent::ModelLoaded);
            }

            // Decode WAV bytes back to f32 samples
            let samples = match decode_wav(&wav_bytes) {
                Ok(s) => s,
                Err(err) => {
                    let _ = tx.send(AppEvent::ApiError(format!("Audio decode error: {}", err)));
                    return;
                }
            };

            match e.transcribe(&samples) {
                Ok(text) => {
                    let _ = tx.send(AppEvent::TranscriptComplete {
                        text,
                        duration_ms: dur,
                    });
                }
                Err(err) => {
                    let _ =
                        tx.send(AppEvent::ApiError(format!("Transcription error: {}", err)));
                }
            }
        });
    }

    fn unload_local_model(&mut self) {
        if let Some(engine) = &self.local_engine {
            let mut e = engine.lock().unwrap();
            e.unload();
        }
        self.model_loaded = false;
    }

    // -- Actions --

    fn yank_current(&mut self) {
        if let Some(transcript) = self.transcripts.get(self.current_index) {
            let content = transcript.text.clone();
            let result = std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(ref mut stdin) = child.stdin {
                        stdin.write_all(content.as_bytes())?;
                    }
                    child.wait()
                });
            match result {
                Ok(_) => self.yank_ticks_remaining = YANK_FEEDBACK_TICKS,
                Err(e) => {
                    self.state = AppState::Error(format!("Yank failed: {}", e));
                }
            }
        }
    }

    fn delete_current(&mut self) {
        let transcript = &self.transcripts[self.current_index];
        if let Some(db) = &self.db {
            let _ = db.soft_delete(transcript.id);
        }
        self.transcripts.remove(self.current_index);
        if self.transcripts.is_empty() {
            self.current_index = 0;
        } else if self.current_index >= self.transcripts.len() {
            self.current_index = self.transcripts.len() - 1;
        }
    }

    fn delete_all(&mut self) {
        if let Some(db) = &self.db {
            let _ = db.soft_delete_all();
        }
        self.transcripts.clear();
        self.current_index = 0;
    }

    fn cancel_recording(&mut self) {
        if let Some(mgr) = &self.audio_manager {
            let mut mgr = mgr.lock().unwrap();
            mgr.cancel_recording();
        }
        self.state = AppState::Idle;
        self.audio_level = 0.0;
        self.recording_duration_ms = 0;
    }

    fn toggle_recording(&mut self) {
        let audio_manager = match &self.audio_manager {
            Some(m) => m.clone(),
            None => {
                self.state =
                    AppState::Error("No audio device available. Check microphone.".to_string());
                return;
            }
        };

        match self.state {
            AppState::Idle => {
                // For local backend, check model is downloaded before recording
                if self.backend == Backend::Local {
                    if let Some(engine) = &self.local_engine {
                        let e = engine.lock().unwrap();
                        if !e.is_model_downloaded() {
                            self.state = AppState::Downloading;
                            let tx = self.event_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    crate::local::download_model(tx.clone()).await
                                {
                                    let _ = tx.send(AppEvent::ModelError(format!(
                                        "Download failed: {}",
                                        e
                                    )));
                                }
                            });
                            return;
                        }
                    }
                }

                let mut mgr = audio_manager.lock().unwrap();
                match mgr.start_recording() {
                    Ok(()) => {
                        self.state = AppState::Recording;
                        self.recording_start = Some(std::time::Instant::now());
                        self.recording_duration_ms = 0;
                        self.audio_level = 0.0;
                        self.level_history.clear();
                    }
                    Err(e) => {
                        self.state = AppState::Error(format!("Mic error: {}", e));
                    }
                }
            }
            AppState::Recording => {
                let mut mgr = audio_manager.lock().unwrap();
                if let Err(e) = mgr.stop_recording() {
                    self.state = AppState::Error(format!("Recording error: {}", e));
                }
            }
            _ => {}
        }
    }
}

/// Decode WAV bytes (16-bit PCM mono 16kHz) back to f32 samples
fn decode_wav(wav_bytes: &[u8]) -> anyhow::Result<Vec<f32>> {
    use std::io::Cursor;
    let cursor = Cursor::new(wav_bytes);
    let mut reader =
        hound::WavReader::new(cursor).context("Failed to read WAV")?;

    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to decode WAV samples")?;

    Ok(samples)
}

use anyhow::Context;
