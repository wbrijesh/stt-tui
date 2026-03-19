use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::api::ApiClient;
use crate::audio::AudioManager;
use crate::config::Config;
use crate::event::AppEvent;
use crate::storage::{fetch_usage_stats, Database, Transcript, UsageStats};

const YANK_FEEDBACK_TICKS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Setup,
    Idle,
    Recording,
    Transcribing,
    Error(String),
}

pub struct App {
    pub state: AppState,
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
    pub mic_devices: Vec<String>,
    pub mic_selected: usize,
    pub mic_current_name: String,
    pub usage_stats: Option<UsageStats>,
    pub should_quit: bool,
    pub api_key_input: String,
    pub setup_error: Option<String>,
    pub total_cost_usd: f64,

    audio_manager: Option<Arc<Mutex<AudioManager>>>,
    api_client: Option<Arc<ApiClient>>,
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
        let (state, api_client, model) = match config {
            Some(cfg) => {
                let model = cfg.model.clone();
                let client = ApiClient::new(cfg.api_key, cfg.model);
                (AppState::Idle, Some(Arc::new(client)), model)
            }
            None => (
                AppState::Setup,
                None,
                "gpt-4o-mini-transcribe".to_string(),
            ),
        };

        let (transcripts, total_cost_usd) = match &db {
            Some(db) => {
                let t = db.active_transcripts().unwrap_or_default();
                let c = db.total_cost().unwrap_or(0.0);
                (t, c)
            }
            None => (Vec::new(), 0.0),
        };

        let mic_current_name = match &audio_manager {
            Some(m) => m.device_name(),
            None => "None".to_string(),
        };

        let current_index = if transcripts.is_empty() {
            0
        } else {
            transcripts.len() - 1
        };

        Self {
            state,
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
            mic_devices: Vec::new(),
            mic_selected: 0,
            mic_current_name,
            usage_stats: None,
            should_quit: false,
            api_key_input: String::new(),
            setup_error: None,
            total_cost_usd,
            audio_manager: audio_manager.map(|m| Arc::new(Mutex::new(m))),
            api_client,
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
                // Keep max 200 samples (~20 seconds at ~10Hz)
                if self.level_history.len() > 200 {
                    self.level_history.remove(0);
                }
            }
            AppEvent::RecordingComplete(wav_bytes) => {
                self.pending_duration_ms = self.recording_duration_ms;
                self.state = AppState::Transcribing;
                self.current_partial.clear();
                self.audio_level = 0.0;

                if let Some(client) = &self.api_client {
                    let client = client.clone();
                    let tx = self.event_tx.clone();
                    let dur = self.pending_duration_ms;
                    tokio::spawn(async move {
                        if let Err(e) = client.transcribe(wav_bytes, tx.clone(), dur).await {
                            let _ = tx.send(AppEvent::ApiError(e.to_string()));
                        }
                    });
                }
            }
            AppEvent::TranscriptDelta(text) => {
                self.current_partial = text;
            }
            AppEvent::TranscriptComplete { text, duration_ms } => {
                if !text.is_empty() {
                    let transcript = Transcript::new(text, duration_ms);
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
                let key = self.api_key_input.trim().to_string();
                if key.is_empty() {
                    self.setup_error = Some("API key cannot be empty".to_string());
                    return;
                }

                let config = Config {
                    api_key: key.clone(),
                    model: self.model.clone(),
                };

                if let Err(e) = config.save() {
                    self.setup_error = Some(format!("Failed to save config: {}", e));
                    return;
                }

                self.api_client = Some(Arc::new(ApiClient::new(key, self.model.clone())));
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

    fn open_mic_picker(&mut self) {
        let devices = AudioManager::list_devices();
        if devices.is_empty() {
            self.state = AppState::Error("No input devices found".to_string());
            return;
        }
        // Find the currently active device in the list
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
