use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    AudioLevel(f32),
    RecordingComplete(Vec<u8>),
    TranscriptDelta(String),
    TranscriptComplete(String),
    ApiError(String),
    Tick,
}

pub struct EventHandler {
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let tx = self.tx;
        tokio::task::spawn_blocking(move || loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if tx.send(AppEvent::Key(key)).is_err() {
                        break;
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }
                }
            } else if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        })
    }
}
