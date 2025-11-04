use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{
    io::{stdout, Stdout},
    sync::{mpsc::{Sender, Receiver}, Arc},
    time::Duration,
    collections::{BTreeSet, VecDeque},
};

use crate::{app::{AppMessage, TuiMessage}, organ::Organ};

const MIDI_LOG_CAPACITY: usize = 10; // Max log lines
const NUM_COLUMNS: usize = 3; // Number of columns for the stop list

/// Holds the state for the TUI.
struct TuiState {
    organ: Arc<Organ>,
    list_state: ListState,
    active_stops: BTreeSet<usize>,
    midi_log: VecDeque<String>,
    error_msg: Option<String>,
    items_per_column: usize,
    stops_count: usize,
}

impl TuiState {
    fn new(organ: Arc<Organ>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0)); // Select the first item
        let stops_count = organ.stops.len();
        let items_per_column = (stops_count + NUM_COLUMNS - 1) / NUM_COLUMNS;
        Self {
            organ,
            list_state,
            active_stops: BTreeSet::new(),
            midi_log: VecDeque::with_capacity(MIDI_LOG_CAPACITY),
            error_msg: None,
            items_per_column,
            stops_count,
        }
    }

    fn next_item(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.organ.stops.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn prev_item(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.organ.stops.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }
    fn next_col(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => (i + self.items_per_column).min(self.stops_count - 1),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn prev_col(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => i.saturating_sub(self.items_per_column),
            None => 0,
        };
        self.list_state.select(Some(i));
    }
    fn toggle_selected_stop(&mut self) -> (usize, bool) {
        if let Some(selected_index) = self.list_state.selected() {
            let is_now_active = if self.active_stops.contains(&selected_index) {
                self.active_stops.remove(&selected_index);
                false
            } else {
                self.active_stops.insert(selected_index);
                true
            };
            (selected_index, is_now_active)
        } else {
            (0, false) // Should not happen
        }
    }

        /// Activates all stops.
    fn select_all_stops(&mut self, audio_tx: &Sender<AppMessage>) -> Result<()> {
        for i in 0..self.stops_count {
            if self.active_stops.insert(i) {
                // Send message only if it wasn't already active
                audio_tx.send(AppMessage::StopToggle(i, true))?;
            }
        }
        Ok(())
    }

    /// Deactivates all stops.
    fn select_none_stops(&mut self, audio_tx: &Sender<AppMessage>) -> Result<()> {
        // Collect stops to deactivate to avoid modifying BTreeSet while iterating
        let stops_to_deactivate: Vec<usize> = self.active_stops.iter().copied().collect();
        for i in stops_to_deactivate {
            if self.active_stops.remove(&i) {
                // Send message only if it was active
                audio_tx.send(AppMessage::StopToggle(i, false))?;
            }
        }
        Ok(())
    }

    fn add_midi_log(&mut self, msg: String) {
        if self.midi_log.len() == MIDI_LOG_CAPACITY {
            self.midi_log.pop_front();
        }
        self.midi_log.push_back(msg);
    }
}

/// Runs the main TUI loop, blocking the main thread.
pub fn run_tui_loop(
    audio_tx: Sender<AppMessage>,
    tui_rx: Receiver<TuiMessage>,
    organ: Arc<Organ>,
) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app_state = TuiState::new(organ);

    loop {
        // Draw UI
        terminal.draw(|f| ui(f, &mut app_state))?;

        // Handle cross-thread messages (non-blocking)
        while let Ok(msg) = tui_rx.try_recv() {
            match msg {
                TuiMessage::MidiLog(log) => app_state.add_midi_log(log),
                TuiMessage::Error(err) => app_state.error_msg = Some(err),
            }
        }

        // Handle input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            // Send Quit message to audio thread
                            audio_tx.send(AppMessage::Quit)?;
                            break; // Exit TUI loop
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app_state.next_item();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app_state.prev_item();
                        }
                        KeyCode::Char('l') | KeyCode::Right => app_state.next_col(),
                        KeyCode::Char('h') | KeyCode::Left => app_state.prev_col(),
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            let (index, is_active) = app_state.toggle_selected_stop();
                            audio_tx.send(AppMessage::StopToggle(index, is_active))?;
                        }
                        KeyCode::Char('a') => {
                            app_state.select_all_stops(&audio_tx)?;
                        }
                        KeyCode::Char('n') => {
                            app_state.select_none_stops(&audio_tx)?;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    cleanup_terminal()?;
    Ok(())
}

/// Renders the UI frame.
fn ui(frame: &mut Frame, state: &mut TuiState) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(80), // Stops
            Constraint::Percentage(20), // MIDI Log
            Constraint::Length(1),      // Footer
        ])
        .split(frame.size());

    // --- Footer Help Text / Error ---
    let footer_widget = if let Some(err) = &state.error_msg {
        Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::White).bg(Color::Red))
    } else {
        let help_text = "Quit: q | Up: ↑/k | Down: ↓/j | Toggle: Space/Enter";
        Paragraph::new(help_text).alignment(Alignment::Center)
    };
    frame.render_widget(footer_widget, main_layout[2]);

    // --- Stop List (Multi-column) ---
    const NUM_COLUMNS: usize = 3;
    let stops_area = main_layout[0];

    // Create 3 columns
    let column_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34), // Col 1
            Constraint::Percentage(33), // Col 2
            Constraint::Percentage(33), // Col 3
        ])
        .split(stops_area);
    
    let selected_index = state.list_state.selected().unwrap_or(0);
    let stops_count = state.organ.stops.len();
    if stops_count == 0 {
        // Handle no stops
        let no_stops_msg = Paragraph::new("No stops loaded.")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(state.organ.name.as_str()));
        frame.render_widget(no_stops_msg, stops_area);
    } else {
        // Calculate items per column
        let items_per_column = (stops_count + NUM_COLUMNS - 1) / NUM_COLUMNS;
        
        let mut all_stops: Vec<_> = state.organ.stops.iter().enumerate().collect();
        
        // Create a list for each column
        for (col_idx, rect) in column_layout.iter().enumerate() {
            let start_idx = col_idx * items_per_column;
            let end_idx = (start_idx + items_per_column).min(stops_count);

            if start_idx >= end_idx {
                continue; // No items for this column
            }

            let column_items: Vec<ListItem> = all_stops[start_idx..end_idx].iter()
                .map(|(global_idx, stop)| {
                    let prefix = if state.active_stops.contains(global_idx) {
                        "[X] "
                    } else {
                        "[ ] "
                    };
                    let line = Line::from(format!("{}{}", prefix, stop.name));
                    
                    let style = if selected_index == *global_idx {
                        // Highlight selected
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else if state.active_stops.contains(global_idx) {
                        // Highlight active
                        Style::default().fg(Color::Green)
                    } else {
                        // Normal
                        Style::default()
                    };
                    ListItem::new(line).style(style)
                })
                .collect();
            
            let title = if col_idx == 0 { state.organ.name.as_str() } else { "" };
            let list_widget = List::new(column_items)
                .block(Block::default().borders(Borders::ALL).title(title));
                
            // We don't use render_stateful_widget because we handle selection manually
            frame.render_widget(list_widget, *rect);
        }
    }

    // --- MIDI Log ---
    let log_items: Vec<ListItem> = state.midi_log.iter()
        .map(|msg| ListItem::new(Line::from(msg.clone())))
        .collect();

    let log_widget = List::new(log_items)
        .block(Block::default().borders(Borders::ALL).title("MIDI Log"))
        .style(Style::default().fg(Color::Cyan));
    
    
    frame.render_widget(log_widget, main_layout[1]);
}

/// Helper to set up the terminal for TUI mode.
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    let mut stdout = stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

/// Helper to clean up the terminal.
fn cleanup_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

