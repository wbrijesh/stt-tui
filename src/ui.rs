use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, AppState};

const GREEN: Color = Color::Rgb(0, 180, 80);
const DIM: Color = Color::Rgb(80, 80, 80);

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

    if app.show_stats {
        render_stats_modal(frame, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let bg = GREEN;
    let fg = Color::Black;

    let right_text = match &app.state {
        AppState::Setup => "SETUP ".to_string(),
        AppState::Recording => {
            let secs = app.recording_duration_ms / 1000;
            format!(
                "REC {:02}:{:02} ",
                secs / 60,
                secs % 60
            )
        }
        AppState::Transcribing => "TRANSCRIBING ".to_string(),
        AppState::Error(_) => "ERROR ".to_string(),
        AppState::Idle => {
            if app.transcripts.is_empty() {
                "READY ".to_string()
            } else {
                let has_prev = app.current_index > 0;
                let has_next = app.current_index < app.transcripts.len() - 1;
                format!(
                    "{} {}/{} {} ",
                    if has_prev { "\u{25C0}" } else { " " },
                    app.current_index + 1,
                    app.transcripts.len(),
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

    let secs = app.recording_duration_ms / 1000;
    let millis = (app.recording_duration_ms % 1000) / 100;
    let timer = format!("{:02}:{:02}.{}", secs / 60, secs % 60, millis);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("\u{25CF} Recording  {}", timer),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled("SPACE to stop  /  ESC to cancel", Style::default().fg(DIM))),
    ];

    let offset = mid_y.saturating_sub(2);
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

    let dot_count = ((app.tick_count / 3) as usize % 3) + 1;
    let dots = format!("{}{}", ".".repeat(dot_count), " ".repeat(3 - dot_count));

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Transcribing{}", dots),
            Style::default().fg(Color::Yellow),
        )),
    ];

    if !app.current_partial.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("\"{}\"", &app.current_partial),
            Style::default().fg(Color::White).add_modifier(Modifier::DIM),
        )));
    }

    let offset = mid_y.saturating_sub(2);
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

    if app.transcripts.is_empty() {
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
        let transcript = &app.transcripts[app.current_index];

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  > ", Style::default().fg(GREEN)),
            Span::styled(transcript.text.clone(), Style::default().fg(Color::White)),
        ]));

        // Metadata line: duration, relative time, cost
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("    ", Style::default().fg(DIM)),
            Span::styled(transcript.duration_display(), Style::default().fg(DIM)),
            Span::styled("  \u{00B7}  ", Style::default().fg(DIM)),
            Span::styled(transcript.relative_time(), Style::default().fg(DIM)),
            Span::styled("  \u{00B7}  ", Style::default().fg(DIM)),
            Span::styled(format!("${:.6}", transcript.cost_usd), Style::default().fg(DIM)),
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

    if app.state == AppState::Recording {
        spans.extend(vec![
            Span::styled("ESC", key_style),
            Span::styled(" Cancel  ", label_style),
        ]);
    }

    if app.state == AppState::Idle && !app.transcripts.is_empty() {
        spans.extend(vec![
            Span::styled("h", key_style),
            Span::styled("/", label_style),
            Span::styled("l", key_style),
            Span::styled(" Nav  ", label_style),
            Span::styled("y", key_style),
            Span::styled(" Yank  ", label_style),
        ]);
    }

    if app.state == AppState::Idle && !app.transcripts.is_empty() {
        spans.extend(vec![
            Span::styled("d", key_style),
            Span::styled(" Del  ", label_style),
            Span::styled("D", key_style),
            Span::styled(" Del All  ", label_style),
        ]);
    }

    if app.state != AppState::Recording {
        spans.extend(vec![
            Span::styled("S", key_style),
            Span::styled(" Stats  ", label_style),
            Span::styled("q", key_style),
            Span::styled(" Quit  ", label_style),
            Span::styled("?", key_style),
            Span::styled(" Help", label_style),
        ]);
    }

    let controls = Line::from(spans);
    let paragraph = Paragraph::new(controls).style(Style::default().bg(bg));
    frame.render_widget(paragraph, area);
}

fn render_help_modal(frame: &mut Frame) {
    let area = frame.area();

    // Center a box ~60 wide, ~18 tall
    let modal_width = 60u16.min(area.width.saturating_sub(4));
    let modal_height = 24u16.min(area.height.saturating_sub(4));

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
            Span::styled("    ESC    ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel recording / quit", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    h / l  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Navigate between transcripts", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    y      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Yank (copy) current to clipboard", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    d      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Delete current transcript", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    D      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Delete all transcripts", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    S      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Usage stats", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("    q      ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Quit", Style::default().fg(DIM)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Transcripts saved to ~/.config/stt-tui/stt-tui.db",
            Style::default().fg(DIM),
        )),
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

fn render_stats_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let modal_width = 50u16.min(area.width.saturating_sub(4));
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
    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title(" Usage Stats ")
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN));

    let stats = match &app.usage_stats {
        Some(s) => s,
        None => {
            let p = Paragraph::new("  No data available")
                .block(block)
                .style(Style::default().bg(Color::Rgb(15, 15, 15)));
            frame.render_widget(p, modal_area);
            return;
        }
    };

    let label_style = Style::default().fg(DIM);
    let val_style = Style::default().fg(Color::White);
    let head_style = Style::default().fg(GREEN).add_modifier(Modifier::BOLD);

    let mut content: Vec<Line> = Vec::new();

    let periods = [
        ("TODAY", &stats.today),
        ("THIS WEEK", &stats.this_week),
        ("THIS MONTH", &stats.this_month),
        ("ALL TIME", &stats.all_time),
    ];

    for (name, period) in &periods {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(format!("  {}", name), head_style)));
        content.push(Line::from(vec![
            Span::styled("    Transcripts  ", label_style),
            Span::styled(format!("{}", period.count), val_style),
        ]));
        content.push(Line::from(vec![
            Span::styled("    Duration     ", label_style),
            Span::styled(period.duration_display(), val_style),
        ]));
        content.push(Line::from(vec![
            Span::styled("    Cost         ", label_style),
            Span::styled(format!("${:.4}", period.cost_usd), val_style),
        ]));
    }

    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Press any key to close",
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Color::Rgb(15, 15, 15)));

    frame.render_widget(paragraph, modal_area);
}
