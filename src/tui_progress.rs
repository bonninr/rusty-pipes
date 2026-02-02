use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(10),
            Constraint::Min(0),
        ])
        .split(f.area());

    let area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(chunks[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(t!("loading.window_title"))
        .title_alignment(Alignment::Center);

    let inner = block.inner(area);
    f.render_widget(block, area);

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
