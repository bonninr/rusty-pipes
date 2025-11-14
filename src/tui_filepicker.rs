use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},

};
use ratatui::{
    prelude::*,

    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{

    time::Duration,
    fs, // <-- ADDED
    path::{Path, PathBuf},
};

use crate::tui::{setup_terminal, cleanup_terminal};

/// Holds state for the TUI file picker.
struct TuiFilePickerState {
    current_path: PathBuf,
    entries: Vec<PathBuf>, // Just store the paths
    list_state: ListState,
    error_msg: Option<String>,
}

impl TuiFilePickerState {
    fn new() -> Result<Self> {
        let current_path = std::env::current_dir()?;
        let mut state = Self {
            current_path,
            entries: Vec::new(),
            list_state: ListState::default(),
            error_msg: None,
        };
        state.load_entries()?; // Load initial entries
        Ok(state)
    }
    
    /// Helper to check for allowed extensions
    fn is_allowed_file(path: &Path) -> bool {
        if !path.is_file() { return false; }
        let ext = path.extension().and_then(|s| s.to_str());
        matches!(ext, Some("organ") | Some("Organ_Hauptwerk_xml"))
    }

    /// Read the current directory and fill the entries list
    fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        self.list_state.select(None); // Deselect
        self.error_msg = None;

        match fs::read_dir(&self.current_path) {
            Ok(entries) => {
                // 1. Collect all valid paths
                let mut paths: Vec<PathBuf> = entries
                    .filter_map(Result::ok) // Ignore read errors on individual entries
                    .map(|e| e.path())
                    .filter(|p| {
                        // Show directories and allowed files
                        p.is_dir() || Self::is_allowed_file(p)
                    })
                    .collect();

                // 2. Sort them: directories first, then files
                paths.sort_by(|a, b| {
                    if a.is_dir() && !b.is_dir() {
                        std::cmp::Ordering::Less
                    } else if !a.is_dir() && b.is_dir() {
                        std::cmp::Ordering::Greater
                    } else {
                        a.file_name().cmp(&b.file_name())
                    }
                });
                
                self.entries = paths;

                if !self.entries.is_empty() {
                    self.list_state.select(Some(0));
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Error reading directory: {}", e));
            }
        }
        Ok(())
    }

    /// Get the currently selected path, if any
    fn get_selected_path(&self) -> Option<&PathBuf> {
        self.list_state.selected().and_then(|i| self.entries.get(i))
    }

    /// Move selection to the next item
    fn next_item(&mut self) {
        if self.entries.is_empty() { return; }
        let i = self.list_state.selected().map_or(0, |i| (i + 1) % self.entries.len());
        self.list_state.select(Some(i));
    }

    /// Move selection to the previous item
    fn prev_item(&mut self) {
        if self.entries.is_empty() { return; }
        let len = self.entries.len();
        let i = self.list_state.selected().map_or(0, |i| (i + len - 1) % len);
        self.list_state.select(Some(i));
    }
    
    /// Called on 'Enter'. If a file, returns it. If a dir, navigates into it.
    fn activate_selected(&mut self) -> Result<Option<PathBuf>> {
        if let Some(path) = self.get_selected_path().cloned() {
            if path.is_dir() {
                self.current_path = path;
                self.load_entries()?;
            } else if Self::is_allowed_file(&path) {
                // Found it!
                return Ok(Some(path));
            }
        }
        Ok(None)
    }
    
    /// Navigates up to the parent directory
    fn go_up(&mut self) -> Result<()> {
        if let Some(parent) = self.current_path.parent() {
            self.current_path = parent.to_path_buf();
            self.load_entries()?;
        }
        Ok(())
    }
}

/// Runs a TUI loop to browse for an organ file.
/// Returns the path if selected, or None if the user quits.
pub fn run_tui_file_picker_loop() -> Result<Option<PathBuf>> {
    let mut terminal = setup_terminal()?;
    let mut state = TuiFilePickerState::new()?;
    
    let result: Option<PathBuf> = loop { // Assign loop result to a variable
        terminal.draw(|f| draw_file_picker_ui(f, &mut state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break None; // Quit
                        }
                        KeyCode::Down | KeyCode::Char('j') => state.next_item(),
                        KeyCode::Up | KeyCode::Char('k') => state.prev_item(),
                        KeyCode::PageDown => {
                            for _ in 0..5 { state.next_item(); }
                        }
                        KeyCode::PageUp => {
                            for _ in 0..5 { state.prev_item(); }
                        }
                        KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => {
                            if let Err(e) = state.go_up() {
                                state.error_msg = Some(format!("Error: {}", e));
                            }
                        },
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                            match state.activate_selected() {
                                Ok(Some(file_path)) => break Some(file_path), // File selected!
                                Ok(None) => {}, // Was a directory, state updated
                                Err(e) => state.error_msg = Some(format!("Error: {}", e)),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }; // Loop ends, `result` is set

    cleanup_terminal()?; // Clean up *before* returning
    Ok(result) // Return the result
}

/// Renders the File Picker UI.
fn draw_file_picker_ui(frame: &mut Frame, state: &mut TuiFilePickerState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // File list
            Constraint::Length(1), // Footer
            Constraint::Length(1), // Error
        ])
        .split(frame.area());

    // Header
    let header_block = Block::default().borders(Borders::ALL)
        .title("Select Organ File (q to quit)");
    let header_text = Paragraph::new(format!("Current Path: {}", state.current_path.display()))
        .block(header_block);
    frame.render_widget(header_text, layout[0]);

    // File List
    let items: Vec<ListItem> = state.entries.iter()
        .map(|path| {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            let line = if path.is_dir() {
                Line::styled(format!("[{}/]", file_name), Style::default().fg(Color::Cyan))
            } else {
                Line::from(file_name.into_owned())
            };
            ListItem::new(line)
        })
        .collect();

    let list_widget = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Entries"))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
        .highlight_symbol("» ");
    
    frame.render_stateful_widget(list_widget, layout[1], &mut state.list_state);

    // Footer
    let footer_text = "Nav: ↑/↓/PgUp/PgDown | Enter/→: Select | ←/Backspace: Up | q: Quit";
    frame.render_widget(Paragraph::new(footer_text).alignment(Alignment::Center), layout[2]);

    // Error
    if let Some(err) = &state.error_msg {
        frame.render_widget(
            Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red)),
            layout[3]
        );
    }
}