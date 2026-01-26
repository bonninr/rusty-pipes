use crate::config::{LcdColor, LcdDisplayConfig, LcdLineType};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use rust_i18n::t;

/// Result of a keypress handling event.
pub enum LcdConfigAction {
    /// No navigation change, stay on screen.
    None,
    /// User requested to return to the previous menu (Esc).
    Back,
}

/// Helper state for the LCD TUI screen.
pub struct TuiLcdState {
    pub list_state: ListState,
    /// If `Some(row_index, field_index)`, the user is editing a specific field.
    /// field_index: 0=ID, 1=Color, 2=Line1, 3=Line2
    pub editing_field: Option<(usize, usize)>,
}

impl TuiLcdState {
    pub fn new() -> Self {
        let mut s = Self {
            list_state: ListState::default(),
            editing_field: None,
        };
        s.list_state.select(Some(0));
        s
    }
}

/// Handles keyboard input for the LCD config screen.
pub fn handle_input(
    event: KeyEvent,
    state: &mut TuiLcdState,
    lcd_displays: &mut Vec<LcdDisplayConfig>,
) -> LcdConfigAction {
    let display_count = lcd_displays.len();
    let total_rows = display_count + 1; // +1 for "Add New" button

    match event.code {
        KeyCode::Esc => {
            if state.editing_field.is_some() {
                state.editing_field = None;
                return LcdConfigAction::None;
            }
            return LcdConfigAction::Back;
        }

        // --- Navigation (Up/Down) ---
        KeyCode::Up | KeyCode::Char('k') => {
            if state.editing_field.is_none() {
                let i = state.list_state.selected().unwrap_or(0);
                let next = if i == 0 { total_rows - 1 } else { i - 1 };
                state.list_state.select(Some(next));
            } else {
                handle_field_edit(state, lcd_displays, true); // true = increment/next
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.editing_field.is_none() {
                let i = state.list_state.selected().unwrap_or(0);
                let next = (i + 1) % total_rows;
                state.list_state.select(Some(next));
            } else {
                handle_field_edit(state, lcd_displays, false); // false = decrement/prev
            }
        }

        KeyCode::Left | KeyCode::Char('h') => {
            if let Some((row_idx, col_idx)) = state.editing_field {
                let new_col = if col_idx == 0 { 3 } else { col_idx - 1 };
                state.editing_field = Some((row_idx, new_col));
            }
        }

        KeyCode::Right | KeyCode::Char('l') => {
            if let Some((row_idx, col_idx)) = state.editing_field {
                let new_col = (col_idx + 1) % 4;
                state.editing_field = Some((row_idx, new_col));
            }
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            let idx = state.list_state.selected().unwrap_or(0);

            if idx == display_count {
                // "Add New" selected
                let next_id = (lcd_displays.len() as u8 % 127) + 1;
                lcd_displays.push(LcdDisplayConfig {
                    id: next_id,
                    background_color: LcdColor::White,
                    line1: LcdLineType::OrganName,
                    line2: LcdLineType::SystemStatus,
                });
                // Select the new item
                state.list_state.select(Some(idx));
            } else {
                // Existing Display Selected
                if state.editing_field.is_some() {
                    // Confirm changes / Exit Edit Mode
                    state.editing_field = None;
                } else {
                    // Enter Edit Mode (Start at Field 0: ID)
                    state.editing_field = Some((idx, 0));
                }
            }
        }

        // Delete
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Backspace => {
            if state.editing_field.is_none() {
                let idx = state.list_state.selected().unwrap_or(0);
                if idx < display_count {
                    lcd_displays.remove(idx);
                    // Adjust selection if we deleted the last item
                    if idx >= lcd_displays.len() && !lcd_displays.is_empty() {
                        state.list_state.select(Some(lcd_displays.len() - 1));
                    }
                }
            }
        }

        _ => {}
    }
    LcdConfigAction::None
}

fn handle_field_edit(
    state: &mut TuiLcdState,
    lcd_displays: &mut Vec<LcdDisplayConfig>,
    forward: bool,
) {
    if let Some((row, col)) = state.editing_field {
        if let Some(display) = lcd_displays.get_mut(row) {
            match col {
                0 => {
                    // ID
                    let val = display.id as i16 + (if forward { 1 } else { -1 });
                    display.id = val.clamp(1, 127) as u8;
                }
                1 => {
                    // Color
                    // Cycle enum
                    display.background_color = cycle_color(&display.background_color, forward);
                }
                2 => {
                    // Line 1
                    display.line1 = cycle_line_type(&display.line1, forward);
                }
                3 => {
                    // Line 2
                    display.line2 = cycle_line_type(&display.line2, forward);
                }
                _ => {}
            }
        }
    }
}

fn cycle_color(c: &LcdColor, forward: bool) -> LcdColor {
    use LcdColor::*;
    let variants = [Off, White, Red, Green, Yellow, Blue, Magenta, Cyan];
    let idx = variants.iter().position(|x| x == c).unwrap_or(0);
    let next_idx = if forward {
        (idx + 1) % variants.len()
    } else {
        (idx + variants.len() - 1) % variants.len()
    };
    variants[next_idx].clone()
}

fn cycle_line_type(t: &LcdLineType, forward: bool) -> LcdLineType {
    use LcdLineType::*;
    let variants = [
        Empty,
        OrganName,
        SystemStatus,
        LastPreset,
        LastStopChange,
        MidiLog,
        Gain,
        ReverbMix,
        MidiPlayerStatus,
    ];
    let idx = variants.iter().position(|x| x == t).unwrap_or(0);
    let next_idx = if forward {
        (idx + 1) % variants.len()
    } else {
        (idx + variants.len() - 1) % variants.len()
    };
    variants[next_idx].clone()
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    state: &mut TuiLcdState,
    lcd_displays: &Vec<LcdDisplayConfig>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(t!("config.lcd_title"))
        .title_bottom(t!("config.lcd_tui_nav"));

    // Clear the area before rendering the block to avoid overlap
    frame.render_widget(Clear, area);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    let mut items = Vec::new();

    for (i, display) in lcd_displays.iter().enumerate() {
        // Determine if this row is being edited
        let is_editing_row = state.editing_field.map(|(r, _)| r == i).unwrap_or(false);
        let editing_col = if is_editing_row {
            state.editing_field.map(|(_, c)| c)
        } else {
            None
        };

        let id_str = format!("ID: {:<3}", display.id);
        let color_str = format!("Bg: {:?}", display.background_color);
        let l1_str = format!("L1: {:?}", display.line1);
        let l2_str = format!("L2: {:?}", display.line2);

        // Styling helpers
        let style_field = |col_idx: usize, txt: String| -> Span {
            if Some(col_idx) == editing_col {
                Span::styled(
                    txt,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw(txt)
            }
        };

        // We build a single line with fixed spacing
        // ID: 123 | Bg: Magenta | L1: OrganName      | L2: SystemStatus
        let spans = Line::from(vec![
            style_field(0, id_str),
            Span::raw(" | "),
            style_field(1, color_str),
            Span::raw(" | "),
            style_field(2, l1_str),
            Span::raw(" | "),
            style_field(3, l2_str),
        ]);

        items.push(ListItem::new(spans));
    }

    // Add "Add New" Button
    items.push(ListItem::new(Line::from(vec![Span::styled(
        t!("config.lcd_add"),
        Style::default().add_modifier(Modifier::ITALIC),
    )])));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, inner_area, &mut state.list_state);
}
