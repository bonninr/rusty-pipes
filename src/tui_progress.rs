use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::{Color, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Gauge, Paragraph},
};
use rust_i18n::t;
use std::{
    io,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use crate::app::{LOGO, PIPES};

pub fn run_progress_ui<B: Backend>(
    terminal: &mut Terminal<B>,
    rx: Receiver<(f32, String)>,
) -> io::Result<()> {
    let mut progress = 0.0;
    let mut status_text = String::from("Initializing...");

    loop {
        // Drain updates from the loader
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok((p, msg)) => {
                    progress = p;
                    status_text = msg;
                }
                Err(TryRecvError::Empty) => break, // Nothing new, go render
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        // Render
        terminal
            .draw(|f| render_ui(f, progress, &status_text))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        // Check if loading thread disconnected (finished)
        if disconnected {
            // Let user see 100% for a split second
            if progress >= 0.99 {
                std::thread::sleep(Duration::from_millis(200));
            }
            return Ok(());
        }
    }
}

fn render_ui(f: &mut Frame, progress: f32, status: &str) {
    let area = f.area();

    // Calculate Header Height
    let pipes_lines = PIPES.lines().count();
    let logo_lines_count = LOGO.lines().count();
    let header_height = (pipes_lines + logo_lines_count) as u16;

    // Create Main Layout (Header vs Rest)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Logo Header
            Constraint::Min(0),                // Remaining space for Progress Bar
        ])
        .split(area);

    // Render Header
    let orange_style = Style::default().fg(Color::Rgb(255, 165, 0));
    let gray_style = Style::default().fg(Color::Gray);
    let white_style = Style::default().fg(Color::White);

    let mut logo_lines_vec: Vec<Line> = PIPES
        .lines()
        .map(|line| Line::from(Span::styled(line, gray_style)))
        .collect();

    for line in LOGO.lines() {
        logo_lines_vec.push(Line::from(Span::styled(line, orange_style)));
    }

    logo_lines_vec.push(Line::from(Span::styled(
        t!("config.subtitle"),
        orange_style,
    )));
    logo_lines_vec.push(Line::from(""));

    logo_lines_vec.push(Line::from(Span::styled(
        t!("tui_config.header_title"),
        white_style.add_modifier(Modifier::BOLD),
    )));

    let header_widget = Paragraph::new(logo_lines_vec)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));

    f.render_widget(header_widget, main_layout[0]);

    // Render Progress Box (Centered in remaining space)
    let center_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),    // Spacer top
            Constraint::Length(10), // Progress box height
            Constraint::Fill(1),    // Spacer bottom
        ])
        .split(main_layout[1]);

    // Center horizontally
    let box_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(center_layout[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(t!("loading.window_title"))
        .title_alignment(Alignment::Center);

    let inner = block.inner(box_area);
    f.render_widget(block, box_area);

    let content = Layout::default()
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(t!("loading.status_init")).alignment(Alignment::Center),
        content[0],
    );

    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(Color::Rgb(255, 165, 0))
                .bg(Color::Black),
        )
        .use_unicode(true)
        .ratio(progress as f64)
        .label(format!("{:.1}%", progress * 100.0));
    f.render_widget(gauge, content[1]);

    f.render_widget(
        Paragraph::new(Span::styled(status, Style::default().fg(Color::DarkGray)))
            .alignment(Alignment::Center),
        content[2],
    );
}
