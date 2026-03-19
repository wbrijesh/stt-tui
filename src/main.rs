mod api;
mod app;
mod audio;
mod config;
mod event;
mod storage;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;

use app::App;
use audio::AudioManager;
use config::Config;
use event::EventHandler;
use storage::Database;

#[tokio::main]
async fn main() -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::restore();
        default_hook(info);
    }));

    let (tx, mut rx) = mpsc::unbounded_channel();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: config error: {}", e);
            None
        }
    };

    let audio_manager = match AudioManager::new(tx.clone()) {
        Ok(m) => Some(m),
        Err(_) => None,
    };

    let db = match Database::open() {
        Ok(db) => Some(db),
        Err(e) => {
            eprintln!("Warning: database error: {}", e);
            None
        }
    };

    let mut app = App::new(config, audio_manager, db, tx.clone());

    let event_handler = EventHandler::new(tx);
    let _event_handle = event_handler.start();

    let mut terminal = ratatui::init();

    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;

        if let Some(event) = rx.recv().await {
            app.handle_event(event);
            if app.should_quit {
                break;
            }
        } else {
            break;
        }
    }

    ratatui::restore();
    Ok(())
}
