use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, AppState};

const GREEN: Color = Color::Rgb(0, 180, 80);
const DIM: Color = Color::Rgb(80, 80, 80);

const BRAILLE_SPINNER: &[&str] = &[
    "\u{2801}", "\u{2803}", "\u{2807}", "\u{280F}", "\u{281F}", "\u{283F}", "\u{287F}", "\u{28FF}",
    "\u{28FE}", "\u{28FC}", "\u{28F8}", "\u{28F0}", "\u{28E0}", "\u{28C0}", "\u{2880}", "\u{2800}",
];

const BRAILLE_WAVE: &[char] = &[
    '\u{2800}', '\u{2880}', '\u{28A0}', '\u{28A4}', '\u{28B4}',
    '\u{28F4}', '\u{28FC}', '\u{28FE}', '\u{28FF}',
];

const BRAILLE_DOTS: &[&str] = &[
    "\u{2804}\u{2800}\u{2800}",
    "\u{2844}\u{2800}\u{2800}",
    "\u{28C4}\u{2800}\u{2800}",
    "\u{28E4}\u{2800}\u{2800}",
    "\u{28A4}\u{2804}\u{2800}",
    "\u{2824}\u{2844}\u{2800}",
    "\u{2800}\u{28C4}\u{2800}",
    "\u{2800}\u{28E4}\u{2800}",
    "\u{2800}\u{28A4}\u{2804}",
    "\u{2800}\u{2824}\u{2844}",
    "\u{2800}\u{2800}\u{28C4}",
    "\u{2800}\u{2800}\u{28E4}",
    "\u{2800}\u{2800}\u{28A4}",
    "\u{2800}\u{2800}\u{2824}",
    "\u{2800}\u{2800}\u{2800}",
];

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_header(frame, chunks[0], app);
    render_main(frame, chunks[1], app);
    render_audio_bar(frame, chunks[2], app);
    render_controls(frame, chunks[3], app);

    if app.show_help {
        render_help_modal(frame);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let bg = GREEN;
    let fg = Color::Black;

    let right_text = match &app.state {
        AppState::Setup => "SETUP ".to_string(),
        AppState::Recording => {
            let secs = app.recording_duration_ms / 1000;
            let idx = (app.tick_count as usize) % BRAILLE_SPINNER.len();
            format!(
                "{} REC {:02}:{:02} ",
                BRAILLE_SPINNER[idx],
                secs / 60,
                secs % 60
            )
        }
        AppState::Transcribing => {
            let idx = (app.tick_count as usize) % BRAILLE_DOTS.len();
            format!("{} TRANSCRIBING ", BRAILLE_DOTS[idx])
        }
        AppState::Error(_) => "ERROR ".to_string(),
        AppState::Idle => {
            if app.transcriptions.is_empty() {
                "READY ".to_string()
            } else {
                let has_prev = app.current_index > 0;
                let has_next = app.current_index < app.transcriptions.len() - 1;
                format!(
                    "{} {}/{} {} ",
                    if has_prev { "\u{25C0}" } else { " " },
                    app.current_index + 1,
                    app.transcriptions.len(),
                    if has_next { "\u{25B6}" } else { " " },
                )
            }
        }
    };

    let left = " SPEECH TO TEXT TUI";
    let pad = area
        .width
        .saturating_sub(left.len() as u16 + right_text.len() as u16);

    let header = Line::from(vec![
        Span::styled(
            left,
            Style::default()
                .fg(fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad as usize), Style::default().bg(bg)),
        Span::styled(
            right_text,
            Style::default()
                .fg(fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let paragraph = Paragraph::new(header).style(Style::default().bg(bg));
    frame.render_widget(paragraph, area);
}

fn render_main(frame: &mut Frame, area: Rect, app: &App) {
    match &app.state {
        AppState::Setup => render_setup_view(frame, area, app),
        AppState::Recording => render_recording_view(frame, area, app),
        AppState::Transcribing => render_transcribing_view(frame, area, app),
        AppState::Error(msg) => render_error_view(frame, area, msg),
        AppState::Idle => render_idle_view(frame, area, app),
    }
}

fn render_setup_view(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mid_y = inner.height / 2;

    // Mask the key: show first 8 chars, rest as dots
    let masked: String = if app.api_key_input.is_empty() {
        String::new()
    } else if app.api_key_input.len() <= 8 {
        app.api_key_input.clone()
    } else {
        let visible: String = app.api_key_input.chars().take(8).collect();
        let hidden = "\u{2022}".repeat(app.api_key_input.len() - 8);
        format!("{}{}", visible, hidden)
    };

    let cursor = "\u{2588}";

    // Heading lines rendered as Paragraph above the input
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome to stt-tui",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Enter your OpenAI API key to get started.",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "It will be saved to ~/.config/stt-tui/config.toml",
            Style::default().fg(DIM),
        )),
        Line::from(""),
    ];

    let offset = mid_y.saturating_sub(7);
    let heading_area = Rect {
        x: inner.x,
        y: inner.y + offset,
        width: inner.width,
        height: 6,
    };
    let heading = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(heading, heading_area);

    // Input box — centered, bordered
    let input_width = 52u16.min(inner.width.saturating_sub(4));
    let input_x = inner.x + (inner.width.saturating_sub(input_width)) / 2;
    let input_y = inner.y + offset + 6;
    let input_area = Rect {
        x: input_x,
        y: input_y,
        width: input_width,
        height: 3,
    };

    let input_block = Block::default()
        .title(" API Key ")
        .title_style(Style::default().fg(GREEN))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN));

    let input_line = Line::from(vec![
        Span::styled(&masked, Style::default().fg(Color::White)),
        Span::styled(cursor, Style::default().fg(GREEN)),
    ]);

    let input_paragraph = Paragraph::new(input_line).block(input_block);
    frame.render_widget(input_paragraph, input_area);

    // Error / hint below input
    let hint_y = input_y + 3;
    let hint_area = Rect {
        x: inner.x,
        y: hint_y,
        width: inner.width,
        height: 3,
    };

    let mut hint_lines = Vec::new();
    if let Some(err) = &app.setup_error {
        hint_lines.push(Line::from(Span::styled(
            format!("! {}", err),
            Style::default().fg(Color::Red),
        )));
    }
    hint_lines.push(Line::from(""));
    hint_lines.push(Line::from(Span::styled(
        "ENTER to confirm  /  ESC to quit",
        Style::default().fg(DIM),
    )));

    let hint = Paragraph::new(hint_lines).alignment(Alignment::Center);
    frame.render_widget(hint, hint_area);
}

fn render_recording_view(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mid_y = inner.height / 2;
    let wave_width = (inner.width as usize).min(60);
    let tick = app.tick_count as f64;
    let level = app.audio_level.clamp(0.0, 1.0) as f64;

    let mut wave_chars = String::with_capacity(wave_width);
    for i in 0..wave_width {
        let x = i as f64;
        let val = (0.3 + level * 0.7)
            * ((x * 0.15 + tick * 0.3).sin() * 0.5
                + (x * 0.08 - tick * 0.2).sin() * 0.3
                + (x * 0.22 + tick * 0.5).sin() * 0.2);
        let normalized = ((val + 1.0) / 2.0).clamp(0.0, 1.0);
        let idx = (normalized * (BRAILLE_WAVE.len() - 1) as f64) as usize;
        wave_chars.push(BRAILLE_WAVE[idx]);
    }

    let secs = app.recording_duration_ms / 1000;
    let millis = (app.recording_duration_ms % 1000) / 100;
    let timer = format!("{:02}:{:02}.{}", secs / 60, secs % 60, millis);
    let spinner_idx = (app.tick_count as usize) % BRAILLE_SPINNER.len();

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(&wave_chars, Style::default().fg(Color::Red))),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {} ", BRAILLE_SPINNER[spinner_idx]),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("Recording  {}", timer), Style::default().fg(Color::Red)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Press SPACE to stop", Style::default().fg(DIM))),
    ];

    let offset = mid_y.saturating_sub(3);
    let content_area = Rect {
        x: inner.x,
        y: inner.y + offset,
        width: inner.width,
        height: inner.height.saturating_sub(offset),
    };
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, content_area);
}

fn render_transcribing_view(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mid_y = inner.height / 2;
    let tick = app.tick_count as usize;

    let bar_width = 20usize;
    let mut bar = String::with_capacity(bar_width);
    for i in 0..bar_width {
        let phase = ((i as f64 + tick as f64 * 0.5).sin() + 1.0) / 2.0;
        let idx = (phase * (BRAILLE_WAVE.len() - 1) as f64) as usize;
        bar.push(BRAILLE_WAVE[idx]);
    }
    let dots_idx = tick % BRAILLE_DOTS.len();

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(&bar, Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {} ", BRAILLE_DOTS[dots_idx]),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Transcribing...", Style::default().fg(Color::Yellow)),
        ]),
    ];

    if !app.current_partial.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  \"{}\"", &app.current_partial),
            Style::default().fg(Color::White).add_modifier(Modifier::DIM),
        )));
    }

    let offset = mid_y.saturating_sub(3);
    let content_area = Rect {
        x: inner.x,
        y: inner.y + offset,
        width: inner.width,
        height: inner.height.saturating_sub(offset),
    };
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, content_area);
}

fn render_error_view(frame: &mut Frame, area: Rect, msg: &str) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mid_y = inner.height / 2;
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(format!("  ! {}", msg), Style::default().fg(Color::Red))),
        Line::from(""),
        Line::from(Span::styled("  Press any key to continue", Style::default().fg(DIM))),
    ];

    let offset = mid_y.saturating_sub(2);
    let content_area = Rect {
        x: inner.x,
        y: inner.y + offset,
        width: inner.width,
        height: inner.height.saturating_sub(offset),
    };
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, content_area);
}

fn render_idle_view(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.transcriptions.is_empty() {
        let mid_y = inner.height / 2;
        let lines = vec![
            Line::from(Span::styled("Waiting for input...", Style::default().fg(DIM))),
            Line::from(""),
            Line::from(Span::styled("Press SPACE to start recording", Style::default().fg(DIM))),
        ];
        let offset = mid_y.saturating_sub(1);
        let content_area = Rect {
            x: inner.x,
            y: inner.y + offset,
            width: inner.width,
            height: inner.height.saturating_sub(offset),
        };
        let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
        frame.render_widget(paragraph, content_area);
    } else {
        let text = &app.transcriptions[app.current_index];

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  > ", Style::default().fg(GREEN)),
            Span::styled(text.clone(), Style::default().fg(Color::White)),
        ]));

        if app.yank_ticks_remaining > 0 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  yanked!",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            )));
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }
}

fn render_audio_bar(frame: &mut Frame, area: Rect, app: &App) {
    let level = (app.audio_level * 5.0).clamp(0.0, 1.0);
    let meter_width = 30usize;
    let filled = (level * meter_width as f32) as usize;
    let empty = meter_width - filled;

    let meter = format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(empty),
    );

    let rec_indicator = if app.state == AppState::Recording {
        vec![
            Span::styled("  REC ", Style::default().fg(Color::White)),
            Span::styled("\u{25CF}", Style::default().fg(Color::Red)),
        ]
    } else {
        vec![Span::styled("  REC \u{25CB}", Style::default().fg(DIM))]
    };

    let mut spans = vec![
        Span::styled(
            " AUDIO  ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(meter, Style::default().fg(GREEN)),
    ];
    spans.extend(rec_indicator);

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(paragraph, area);
}

fn render_controls(frame: &mut Frame, area: Rect, app: &App) {
    let bg = GREEN;
    let fg = Color::Black;
    let key_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(fg).bg(bg);

    if app.state == AppState::Setup {
        let spans = vec![
            Span::styled(" ENTER", key_style),
            Span::styled(" Confirm  ", label_style),
            Span::styled("ESC", key_style),
            Span::styled(" Quit", label_style),
        ];
        let controls = Line::from(spans);
        let paragraph = Paragraph::new(controls).style(Style::default().bg(bg));
        frame.render_widget(paragraph, area);
        return;
    }

    let mut spans = vec![
        Span::styled(" \u{2423}", key_style),
        Span::styled(" Start/Stop  ", label_style),
    ];

    if app.state == AppState::Idle && !app.transcriptions.is_empty() {
        spans.extend(vec![
            Span::styled("h", key_style),
            Span::styled("/", label_style),
            Span::styled("l", key_style),
            Span::styled(" Nav  ", label_style),
            Span::styled("y", key_style),
            Span::styled(" Yank  ", label_style),
        ]);
    }

    spans.extend(vec![
        Span::styled("c", key_style),
        Span::styled(" Clear  ", label_style),
        Span::styled("q", key_style),
        Span::styled(" Quit  ", label_style),
        Span::styled("?", key_style),
        Span::styled(" Help", label_style),
    ]);

    let controls = Line::from(spans);
    let paragraph = Paragraph::new(controls).style(Style::default().bg(bg));
    frame.render_widget(paragraph, area);
}

fn render_help_modal(frame: &mut Frame) {
    let area = frame.area();

    // Center a box ~60 wide, ~18 tall
    let modal_width = 60u16.min(area.width.saturating_sub(4));
    let modal_height = 20u16.min(area.height.saturating_sub(4));

    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(modal_height),
        Constraint::Fill(1),
    ])
    .split(area);

    let horizontal = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(modal_width),
        Constraint::Fill(1),
    ])
    .split(vertical[1]);

    let modal_area = horizontal[1];

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(" stt-tui ")
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN));

    let content = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  A terminal speech-to-text interface.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Record your voice directly from the terminal",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "  and get transcriptions powered by OpenAI.",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "  No browser, no GUI, no distractions.",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  USAGE",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    SPACE  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Start / stop recording", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    h / l  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Navigate between transcriptions", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    y      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Yank (copy) current to clipboard", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    c      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Clear all transcriptions", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    q/ESC  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Quit", Style::default().fg(DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Config: ~/.config/stt-tui/config.toml",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )),
    ];

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Color::Rgb(15, 15, 15)));

    frame.render_widget(paragraph, modal_area);
}
