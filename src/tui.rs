use std::{io, ops::Range};

use ansitok::{AnsiColor, ElementKind, Output, VisualAttribute, parse_ansi, parse_ansi_sgr};
use crossterm::{
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self as crossterm_terminal, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    DefaultTerminal, Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, HighlightSpacing, Paragraph, Row, Table, TableState, Wrap},
};

use crate::{
    config::{TuiAction, TuiKeybindings},
    render::{ResultGrid, format_json_value},
};

const TARGET_COLUMN_WIDTH: u16 = 24;

pub fn show_result_grid(grid: &ResultGrid, keybindings: &TuiKeybindings) -> io::Result<()> {
    let mut session = TerminalSession::start()?;
    let result = run_result_grid(&mut session.terminal, grid, keybindings);
    let restore_result = session.restore();

    match (result, restore_result) {
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
    }
}

struct TerminalSession {
    terminal: DefaultTerminal,
    restored: bool,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        crossterm_terminal::enable_raw_mode()?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = crossterm_terminal::disable_raw_mode();
            return Err(err);
        }

        let backend = CrosstermBackend::new(io::stdout());
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(err) => {
                let _ = restore_terminal();
                return Err(err);
            }
        };

        Ok(Self {
            terminal,
            restored: false,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        self.restored = true;
        restore_terminal()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.restored {
            let _ = restore_terminal();
        }
    }
}

fn restore_terminal() -> io::Result<()> {
    let raw_result = crossterm_terminal::disable_raw_mode();
    let screen_result = execute!(io::stdout(), LeaveAlternateScreen);

    match (raw_result, screen_result) {
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn run_result_grid(
    terminal: &mut DefaultTerminal,
    grid: &ResultGrid,
    keybindings: &TuiKeybindings,
) -> io::Result<()> {
    let mut state = GridViewState::new();
    state.clamp_to_grid(grid);

    loop {
        terminal.draw(|frame| render_result_grid(frame, grid, &mut state))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        if state.handle_key(key, grid, keybindings) {
            break;
        }
    }

    Ok(())
}

fn render_result_grid(frame: &mut Frame<'_>, grid: &ResultGrid, state: &mut GridViewState) {
    let frame_area = frame.area();
    let areas = if state.preview_open {
        Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame_area)
    } else {
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(frame_area)
    };
    let table_area = areas[0];
    let preview_area = state.preview_open.then_some(areas[1]);
    let status_area = if state.preview_open {
        areas[2]
    } else {
        areas[1]
    };
    state.set_visible_rows(grid.row_count(), table_area.height);
    let visible_columns = state.visible_columns(grid.column_count(), table_area.width);

    if grid.column_count() == 0 {
        frame.render_widget(
            Paragraph::new("Result has no columns").block(Block::default().borders(Borders::ALL)),
            table_area,
        );
    } else {
        let rows =
            grid.rows().iter().map(|row| {
                Row::new(visible_columns.clone().map(|index| {
                    Cell::from(compact_cell(row.get(index).map_or("", String::as_str)))
                }))
            });
        let header = Row::new(
            visible_columns
                .clone()
                .map(|index| Cell::from(grid.columns()[index].clone())),
        )
        .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let widths =
            (0..visible_columns.len()).map(|_| Constraint::Ratio(1, visible_columns.len() as u32));
        let title = format!(
            "Result set ({} rows, {} columns){}",
            grid.row_count(),
            grid.column_count(),
            focus_marker(state.focus == Focus::Table)
        );
        let table = Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title))
            .column_spacing(1)
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always)
            .row_highlight_style(Style::new().bg(Color::DarkGray))
            .column_highlight_style(Style::new().fg(Color::Yellow))
            .cell_highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        let mut table_state = TableState::default();
        if grid.row_count() > 0 {
            table_state.select(Some(state.selected_row));
        }
        table_state.select_column(Some(state.selected_col - visible_columns.start));
        frame.render_stateful_widget(table, table_area, &mut table_state);
    }

    if let Some(preview_area) = preview_area {
        state.set_visible_preview_rows(preview_area.height);
        let preview = state.preview_content(grid, preview_area.width);
        state.clamp_preview_scroll(preview.line_count);
        let title = format!("Preview{}", focus_marker(state.focus == Focus::Preview));
        let paragraph = Paragraph::new(preview.text)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((state.preview_scroll as u16, 0));
        frame.render_widget(paragraph, preview_area);
    }

    frame.render_widget(
        Paragraph::new(status_line(grid, state, visible_columns))
            .style(Style::new().fg(Color::DarkGray)),
        status_area,
    );
}

fn compact_cell(value: &str) -> String {
    let mut compact = String::new();

    for ch in value.chars() {
        match ch {
            '\n' => compact.push_str("\\n"),
            '\r' => compact.push_str("\\r"),
            '\t' => compact.push_str("\\t"),
            ch if ch.is_control() => {}
            ch => compact.push(ch),
        }
    }

    compact
}

fn status_line(grid: &ResultGrid, state: &GridViewState, visible_columns: Range<usize>) -> String {
    let row = human_position(state.selected_row, grid.row_count());
    let column = human_position(state.selected_col, grid.column_count());
    let first_visible_column = human_position(visible_columns.start, grid.column_count());
    let last_visible_column = visible_columns.end.min(grid.column_count());

    let focus = if state.preview_open {
        match state.focus {
            Focus::Table => "table",
            Focus::Preview => "preview",
        }
    } else {
        "table"
    };

    format!(
        "row {row}/{}, column {column}/{} | visible columns {first_visible_column}-{last_visible_column} | focus: {focus} | configured keys move/preview/focus/quit",
        grid.row_count(),
        grid.column_count()
    )
}

fn human_position(index: usize, total: usize) -> usize {
    if total == 0 { 0 } else { index + 1 }
}

fn focus_marker(active: bool) -> &'static str {
    if active { " [active]" } else { "" }
}

#[derive(Debug, Clone)]
struct PreviewContent {
    text: Text<'static>,
    line_count: usize,
}

#[derive(Debug, Clone)]
struct PreviewCache {
    row: usize,
    column: usize,
    width: u16,
    content: PreviewContent,
}

fn preview_content(grid: &ResultGrid, state: &GridViewState, width: u16) -> PreviewContent {
    let value = grid
        .cell(state.selected_row, state.selected_col)
        .unwrap_or_default();
    let type_name = grid.column_type(state.selected_col).unwrap_or_default();
    let text = preview_text(value, type_name);
    let line_count = wrapped_line_count(&text, width.saturating_sub(2).max(1));

    PreviewContent { text, line_count }
}

fn wrapped_line_count(text: &Text<'_>, width: u16) -> usize {
    let width = usize::from(width).max(1);
    text.lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn preview_text(value: &str, type_name: &str) -> Text<'static> {
    if is_json_type(type_name)
        && let Ok(json) = serde_json::from_str(value)
    {
        return ansi_to_text(&format_json_value(json));
    }

    Text::from(value.to_owned())
}

fn is_json_type(type_name: &str) -> bool {
    matches!(type_name.to_ascii_lowercase().as_str(), "json" | "jsonb")
}

fn ansi_to_text(input: &str) -> Text<'static> {
    let mut lines = vec![Line::default()];
    let mut style = Style::default();

    for token in parse_ansi(input) {
        match token.kind() {
            ElementKind::Text => push_styled_text(&mut lines, &input[token.range()], style),
            ElementKind::Sgr => apply_sgr(&mut style, &input[token.range()]),
            _ => {}
        }
    }

    Text::from(lines)
}

fn push_styled_text(lines: &mut Vec<Line<'static>>, text: &str, style: Style) {
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }

        if !line.is_empty() {
            lines
                .last_mut()
                .expect("text has at least one line")
                .spans
                .push(Span::styled(line.to_owned(), style));
        }
    }
}

fn apply_sgr(style: &mut Style, sgr: &str) {
    for item in parse_ansi_sgr(sgr) {
        match item {
            Output::Escape(attr) => apply_visual_attribute(style, attr),
            Output::Text(_) => {}
        }
    }
}

fn apply_visual_attribute(style: &mut Style, attr: VisualAttribute) {
    match attr {
        VisualAttribute::Bold => style.add_modifier.insert(Modifier::BOLD),
        VisualAttribute::Faint => style.add_modifier.insert(Modifier::DIM),
        VisualAttribute::Italic => style.add_modifier.insert(Modifier::ITALIC),
        VisualAttribute::Underline | VisualAttribute::DoubleUnderline => {
            style.add_modifier.insert(Modifier::UNDERLINED)
        }
        VisualAttribute::Inverse => style.add_modifier.insert(Modifier::REVERSED),
        VisualAttribute::Hide => style.add_modifier.insert(Modifier::HIDDEN),
        VisualAttribute::Crossedout => style.add_modifier.insert(Modifier::CROSSED_OUT),
        VisualAttribute::FgColor(color) => style.fg = ansi_color(color),
        VisualAttribute::BgColor(color) => style.bg = ansi_color(color),
        VisualAttribute::Reset(code) => apply_sgr_reset(style, code),
        _ => {}
    }
}

fn apply_sgr_reset(style: &mut Style, code: u8) {
    match code {
        0 => *style = Style::default(),
        22 => {
            style.add_modifier.remove(Modifier::BOLD | Modifier::DIM);
            style.sub_modifier.insert(Modifier::BOLD | Modifier::DIM);
        }
        23 => {
            style.add_modifier.remove(Modifier::ITALIC);
            style.sub_modifier.insert(Modifier::ITALIC);
        }
        24 => {
            style.add_modifier.remove(Modifier::UNDERLINED);
            style.sub_modifier.insert(Modifier::UNDERLINED);
        }
        27 => {
            style.add_modifier.remove(Modifier::REVERSED);
            style.sub_modifier.insert(Modifier::REVERSED);
        }
        29 => {
            style.add_modifier.remove(Modifier::CROSSED_OUT);
            style.sub_modifier.insert(Modifier::CROSSED_OUT);
        }
        39 => style.fg = None,
        49 => style.bg = None,
        _ => {}
    }
}

fn ansi_color(color: AnsiColor) -> Option<Color> {
    match color {
        AnsiColor::Bit4(code) => ansi_4bit_color(code),
        AnsiColor::Bit8(code) => Some(Color::Indexed(code)),
        AnsiColor::Bit24 { r, g, b } => Some(Color::Rgb(r, g, b)),
    }
}

fn ansi_4bit_color(code: u8) -> Option<Color> {
    Some(match code {
        30 | 40 => Color::Black,
        31 | 41 => Color::Red,
        32 | 42 => Color::Green,
        33 | 43 => Color::Yellow,
        34 | 44 => Color::Blue,
        35 | 45 => Color::Magenta,
        36 | 46 => Color::Cyan,
        37 | 47 => Color::Gray,
        90 | 100 => Color::DarkGray,
        91 | 101 => Color::LightRed,
        92 | 102 => Color::LightGreen,
        93 | 103 => Color::LightYellow,
        94 | 104 => Color::LightBlue,
        95 | 105 => Color::LightMagenta,
        96 | 106 => Color::LightCyan,
        97 | 107 => Color::White,
        39 | 49 => return None,
        _ => return None,
    })
}

#[derive(Debug, Default, Clone)]
struct GridViewState {
    selected_row: usize,
    selected_col: usize,
    col_offset: usize,
    visible_rows: usize,
    visible_cols: usize,
    visible_preview_rows: usize,
    preview_scroll: usize,
    preview_open: bool,
    focus: Focus,
    preview_cache: Option<PreviewCache>,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
enum Focus {
    #[default]
    Table,
    Preview,
}

impl GridViewState {
    fn new() -> Self {
        Self {
            visible_rows: 1,
            visible_cols: 1,
            visible_preview_rows: 1,
            ..Self::default()
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        grid: &ResultGrid,
        keybindings: &TuiKeybindings,
    ) -> bool {
        let Some(action) = keybindings.action_for(key) else {
            return false;
        };

        match action {
            TuiAction::Quit => return true,
            TuiAction::TogglePreview => self.toggle_preview(),
            TuiAction::FocusNext => self.focus_next(),
            _ if self.focus == Focus::Preview => self.apply_preview_action(action),
            _ => self.apply_table_action(action, grid),
        }
        self.clamp_to_grid(grid);
        false
    }

    fn preview_content(&mut self, grid: &ResultGrid, width: u16) -> PreviewContent {
        if let Some(cache) = &self.preview_cache
            && cache.row == self.selected_row
            && cache.column == self.selected_col
            && cache.width == width
        {
            return cache.content.clone();
        }

        let content = preview_content(grid, self, width);
        self.preview_cache = Some(PreviewCache {
            row: self.selected_row,
            column: self.selected_col,
            width,
            content: content.clone(),
        });

        content
    }

    fn apply_table_action(&mut self, action: TuiAction, grid: &ResultGrid) {
        let previous_cell = (self.selected_row, self.selected_col);

        match action {
            TuiAction::Left => self.move_left_by(1),
            TuiAction::Up => self.move_up_by(1),
            TuiAction::Right => self.move_right_by(1, grid.column_count()),
            TuiAction::Down => self.move_down_by(1, grid.row_count()),
            TuiAction::HalfPageLeft => self.move_left_by(half_page(self.visible_cols)),
            TuiAction::HalfPageUp => self.move_up_by(half_page(self.visible_rows)),
            TuiAction::HalfPageRight => {
                self.move_right_by(half_page(self.visible_cols), grid.column_count())
            }
            TuiAction::HalfPageDown => {
                self.move_down_by(half_page(self.visible_rows), grid.row_count())
            }
            TuiAction::FullPageLeft => self.move_left_by(self.visible_cols),
            TuiAction::FullPageUp => self.move_up_by(self.visible_rows),
            TuiAction::FullPageRight => self.move_right_by(self.visible_cols, grid.column_count()),
            TuiAction::FullPageDown => self.move_down_by(self.visible_rows, grid.row_count()),
            TuiAction::TogglePreview | TuiAction::FocusNext | TuiAction::Quit => {}
        }

        if previous_cell != (self.selected_row, self.selected_col) {
            self.preview_scroll = 0;
        }
    }

    fn apply_preview_action(&mut self, action: TuiAction) {
        match action {
            TuiAction::Left | TuiAction::Up => self.scroll_preview_up_by(1),
            TuiAction::Right | TuiAction::Down => self.scroll_preview_down_by(1),
            TuiAction::HalfPageLeft | TuiAction::HalfPageUp => {
                self.scroll_preview_up_by(half_page(self.visible_preview_rows))
            }
            TuiAction::HalfPageRight | TuiAction::HalfPageDown => {
                self.scroll_preview_down_by(half_page(self.visible_preview_rows))
            }
            TuiAction::FullPageLeft | TuiAction::FullPageUp => {
                self.scroll_preview_up_by(self.visible_preview_rows)
            }
            TuiAction::FullPageRight | TuiAction::FullPageDown => {
                self.scroll_preview_down_by(self.visible_preview_rows)
            }
            _ => {}
        }
    }

    fn toggle_preview(&mut self) {
        self.preview_open = !self.preview_open;
        self.focus = Focus::Table;
        self.preview_scroll = 0;
    }

    fn focus_next(&mut self) {
        if self.preview_open {
            self.focus = match self.focus {
                Focus::Table => Focus::Preview,
                Focus::Preview => Focus::Table,
            };
        }
    }

    fn move_up_by(&mut self, amount: usize) {
        self.selected_row = self.selected_row.saturating_sub(amount);
    }

    fn move_down_by(&mut self, amount: usize, row_count: usize) {
        if row_count > 0 {
            self.selected_row = self.selected_row.saturating_add(amount).min(row_count - 1);
        }
    }

    fn move_left_by(&mut self, amount: usize) {
        self.selected_col = self.selected_col.saturating_sub(amount);
    }

    fn move_right_by(&mut self, amount: usize, column_count: usize) {
        if column_count > 0 {
            self.selected_col = self
                .selected_col
                .saturating_add(amount)
                .min(column_count - 1);
        }
    }

    fn scroll_preview_up_by(&mut self, amount: usize) {
        self.preview_scroll = self.preview_scroll.saturating_sub(amount);
    }

    fn scroll_preview_down_by(&mut self, amount: usize) {
        self.preview_scroll = self.preview_scroll.saturating_add(amount);
    }

    fn clamp_to_grid(&mut self, grid: &ResultGrid) {
        if grid.row_count() == 0 {
            self.selected_row = 0;
        } else {
            self.selected_row = self.selected_row.min(grid.row_count() - 1);
        }

        if grid.column_count() == 0 {
            self.selected_col = 0;
            self.col_offset = 0;
        } else {
            self.selected_col = self.selected_col.min(grid.column_count() - 1);
            self.col_offset = self.col_offset.min(grid.column_count() - 1);
        }
    }

    fn visible_columns(&mut self, column_count: usize, area_width: u16) -> Range<usize> {
        if column_count == 0 {
            self.visible_cols = 1;
            return 0..0;
        }

        let visible_count = visible_column_count(column_count, area_width);
        self.visible_cols = visible_count;
        self.ensure_selected_column_visible(column_count, visible_count);
        self.col_offset..self.col_offset + visible_count
    }

    fn set_visible_rows(&mut self, row_count: usize, area_height: u16) {
        self.visible_rows = visible_row_count(row_count, area_height);
    }

    fn set_visible_preview_rows(&mut self, area_height: u16) {
        self.visible_preview_rows = visible_row_count(usize::MAX, area_height);
    }

    fn clamp_preview_scroll(&mut self, line_count: usize) {
        let max_scroll = line_count.saturating_sub(self.visible_preview_rows.max(1));
        self.preview_scroll = self.preview_scroll.min(max_scroll);
    }

    fn ensure_selected_column_visible(&mut self, column_count: usize, visible_count: usize) {
        let visible_count = visible_count.max(1).min(column_count);

        if self.selected_col < self.col_offset {
            self.col_offset = self.selected_col;
        } else if self.selected_col >= self.col_offset + visible_count {
            self.col_offset = self.selected_col + 1 - visible_count;
        }

        self.col_offset = self.col_offset.min(column_count - visible_count);
    }
}

fn half_page(visible_count: usize) -> usize {
    (visible_count / 2).max(1)
}

fn visible_row_count(row_count: usize, area_height: u16) -> usize {
    let body_height = usize::from(area_height.saturating_sub(3)).max(1);
    body_height.min(row_count.max(1))
}

fn visible_column_count(column_count: usize, area_width: u16) -> usize {
    let usable_width = area_width.saturating_sub(2).max(1);
    let target_width = TARGET_COLUMN_WIDTH.min(usable_width).max(1);

    (usable_width / target_width)
        .max(1)
        .min(column_count as u16) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn right_arrow_moves_selection_one_column() {
        // Given
        let grid = test_grid(3, 2);
        let mut state = GridViewState::new();
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.selected_col, 1);
    }

    #[test]
    fn down_arrow_moves_selection_one_row() {
        // Given
        let grid = test_grid(3, 2);
        let mut state = GridViewState::new();
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.selected_row, 1);
    }

    #[test]
    fn navigation_does_not_move_past_last_cell() {
        // Given
        let grid = test_grid(2, 2);
        let mut state = GridViewState {
            selected_row: 1,
            selected_col: 1,
            col_offset: 0,
            ..GridViewState::new()
        };
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.selected_col, 1);
    }

    #[test]
    fn selected_column_stays_inside_visible_window() {
        // Given
        let mut state = GridViewState {
            selected_row: 0,
            selected_col: 3,
            col_offset: 0,
            ..GridViewState::new()
        };

        // When
        let visible_columns = state.visible_columns(5, 74);

        // Then
        assert_eq!(visible_columns, 1..4);
        assert_eq!(state.col_offset, 1);
    }

    #[test]
    fn visible_column_count_uses_terminal_width() {
        // Given
        let column_count = 10;
        let area_width = 74;

        // When
        let visible_columns = visible_column_count(column_count, area_width);

        // Then
        assert_eq!(visible_columns, 3);
    }

    #[test]
    fn half_page_down_moves_by_half_visible_rows() {
        // Given
        let grid = test_grid(3, 20);
        let mut state = GridViewState::new();
        state.set_visible_rows(grid.row_count(), 13);
        let key = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.selected_row, 5);
    }

    #[test]
    fn full_page_right_moves_by_visible_columns() {
        // Given
        let grid = test_grid(10, 2);
        let mut state = GridViewState::new();
        state.visible_columns(grid.column_count(), 74);
        let key = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.selected_col, 3);
    }

    #[test]
    fn quit_keys_exit_viewer() {
        // Given
        let grid = test_grid(2, 2);
        let mut state = GridViewState::new();
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(should_exit);
    }

    #[test]
    fn enter_toggles_preview_without_moving_focus_from_table() {
        // Given
        let grid = test_grid(2, 2);
        let mut state = GridViewState::new();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert!(state.preview_open);
        assert_eq!(state.focus, Focus::Table);
    }

    #[test]
    fn tab_switches_focus_when_preview_is_open() {
        // Given
        let grid = test_grid(2, 2);
        let mut state = GridViewState::new();
        state.preview_open = true;
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.focus, Focus::Preview);
    }

    #[test]
    fn preview_focus_uses_down_binding_to_scroll_preview() {
        // Given
        let grid = test_grid(2, 2);
        let mut state = GridViewState::new();
        state.preview_open = true;
        state.focus = Focus::Preview;
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let keybindings = TuiKeybindings::default();

        // When
        let should_exit = state.handle_key(key, &grid, &keybindings);

        // Then
        assert!(!should_exit);
        assert_eq!(state.preview_scroll, 1);
    }

    #[test]
    fn preview_content_updates_when_selected_cell_changes() {
        // Given
        let grid = ResultGrid::new(
            vec!["a".to_owned(), "b".to_owned()],
            vec!["int4".to_owned(), "int4".to_owned()],
            vec![vec!["one".to_owned(), "two".to_owned()]],
        );
        let mut state = GridViewState::new();

        // When
        let first = state.preview_content(&grid, 40);
        state.selected_col = 1;
        let second = state.preview_content(&grid, 40);

        // Then
        assert_eq!(plain_text(&first.text), "one");
        assert_eq!(plain_text(&second.text), "two");
    }

    #[test]
    fn json_preview_is_pretty_printed_and_colored() {
        // Given
        let value = r#"{"name":"Crab","active":true}"#;

        // When
        let text = preview_text(value, "jsonb");

        // Then
        assert!(plain_text(&text).contains('\n'));
        assert!(
            text.lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.style.fg.is_some()))
        );
    }

    #[test]
    fn text_preview_displays_value_without_external_rendering() {
        // Given
        let value = "# Title";

        // When
        let text = preview_text(value, "text");

        // Then
        assert_eq!(plain_text(&text), "# Title");
    }

    #[test]
    fn non_text_preview_displays_value() {
        // Given
        let value = "42";

        // When
        let text = preview_text(value, "int4");

        // Then
        assert_eq!(plain_text(&text), "42");
    }

    fn test_grid(column_count: usize, row_count: usize) -> ResultGrid {
        ResultGrid::new(
            (1..=column_count)
                .map(|index| format!("column{index}"))
                .collect(),
            vec!["text".to_owned(); column_count],
            (1..=row_count)
                .map(|row| {
                    (1..=column_count)
                        .map(|column| format!("r{row}c{column}"))
                        .collect()
                })
                .collect(),
        )
    }

    fn plain_text(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
