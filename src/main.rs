mod api;
mod app;
mod audio;
mod config;
mod event;
mod local;
mod storage;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;

use app::App;
use audio::AudioManager;
use config::Config;
use event::EventHandler;
use storage::Database;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_version() {
    println!("stt-tui {}", VERSION);
}

fn print_help() {
    println!("stt-tui {}", VERSION);
    println!("A terminal speech-to-text interface powered by OpenAI.");
    println!();
    println!("This is an interactive TUI application — it takes over your");
    println!("terminal when launched. There are no subcommands or flags");
    println!("beyond what's listed below.");
    println!();
    println!("USAGE:");
    println!("    stt-tui              Launch the TUI");
    println!("    stt-tui --version    Print version");
    println!("    stt-tui --help       Print this message");
    println!();
    println!("Once inside the TUI, press ? for keybindings and usage.");
    println!();
    println!("CONFIG:");
    println!("    ~/.config/stt-tui/config.toml    API key and settings");
    println!("    ~/.config/stt-tui/stt-tui.db     Transcript history");
    println!();
    println!("On first launch you will be prompted to enter your OpenAI");
    println!("API key. You can also set OPENAI_API_KEY in your environment.");
    println!();
    println!("https://github.com/wbrijesh/stt-tui");
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--version" | "-v" | "-V" => {
                print_version();
                return Ok(());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("stt-tui: unknown option '{}'", other);
                eprintln!("Run 'stt-tui --help' for usage.");
                std::process::exit(1);
            }
        }
    }

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
