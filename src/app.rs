use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::api::ApiClient;
use crate::audio::AudioManager;
use crate::config::Config;
use crate::event::AppEvent;

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
    pub transcriptions: Vec<String>,
    pub current_index: usize,
    pub current_partial: String,
    pub audio_level: f32,
    pub recording_start: Option<std::time::Instant>,
    pub recording_duration_ms: u64,
    pub tick_count: u64,
    pub yank_ticks_remaining: u64,
    pub show_help: bool,
    pub should_quit: bool,
    pub api_key_input: String,
    pub setup_error: Option<String>,

    audio_manager: Option<Arc<Mutex<AudioManager>>>,
    api_client: Option<Arc<ApiClient>>,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    model: String,
}

impl App {
    pub fn new(
        config: Option<Config>,
        audio_manager: Option<AudioManager>,
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

        Self {
            state,
            transcriptions: Vec::new(),
            current_index: 0,
            current_partial: String::new(),
            audio_level: 0.0,
            recording_start: None,
            recording_duration_ms: 0,
            tick_count: 0,
            yank_ticks_remaining: 0,
            show_help: false,
            should_quit: false,
            api_key_input: String::new(),
            setup_error: None,
            audio_manager: audio_manager.map(|m| Arc::new(Mutex::new(m))),
            api_client,
            event_tx,
            model,
        }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::AudioLevel(level) => {
                self.audio_level = level;
            }
            AppEvent::RecordingComplete(wav_bytes) => {
                self.state = AppState::Transcribing;
                self.current_partial.clear();
                self.audio_level = 0.0;

                if let Some(client) = &self.api_client {
                    let client = client.clone();
                    let tx = self.event_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = client.transcribe(wav_bytes, tx.clone()).await {
                            let _ = tx.send(AppEvent::ApiError(e.to_string()));
                        }
                    });
                }
            }
            AppEvent::TranscriptDelta(text) => {
                self.current_partial = text;
            }
            AppEvent::TranscriptComplete(text) => {
                if !text.is_empty() {
                    self.transcriptions.push(text);
                    self.current_index = self.transcriptions.len() - 1;
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

        if let AppState::Error(_) = &self.state {
            self.state = AppState::Idle;
            return;
        }

        match key.code {
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.state == AppState::Idle {
                    self.should_quit = true;
                }
            }
            KeyCode::Char(' ') => {
                self.toggle_recording();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if self.state == AppState::Idle && !self.transcriptions.is_empty() {
                    self.current_index = self.current_index.saturating_sub(1);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.state == AppState::Idle && !self.transcriptions.is_empty() {
                    self.current_index =
                        (self.current_index + 1).min(self.transcriptions.len() - 1);
                }
            }
            KeyCode::Char('y') => {
                if self.state == AppState::Idle && !self.transcriptions.is_empty() {
                    self.yank_current();
                }
            }
            KeyCode::Char('c') => {
                if self.state == AppState::Idle {
                    self.transcriptions.clear();
                    self.current_index = 0;
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
        if let Some(text) = self.transcriptions.get(self.current_index) {
            let content = text.clone();
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
