use std::{collections::HashMap, mem, ops::Range};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
};

use crate::{
    config::{TuiAction, TuiKeybindings},
    render::{CellValue, ResultGrid},
};

use super::{
    edit::PreviewEdit,
    preview::{PreviewCache, PreviewContent, preview_content_for_value, preview_plain_text},
};

const TARGET_COLUMN_WIDTH: u16 = 24;

#[derive(Debug, Default, Clone)]
pub(super) struct GridViewState {
    pub(super) selected_row: usize,
    pub(super) selected_col: usize,
    pub(super) col_offset: usize,
    pub(super) visible_rows: usize,
    pub(super) visible_cols: usize,
    pub(super) visible_preview_rows: usize,
    pub(super) preview_scroll: usize,
    pub(super) preview_open: bool,
    pub(super) focus: Focus,
    pub(super) preview_cache: Option<PreviewCache>,
    pub(super) staged: HashMap<(usize, usize), CellValue>,
    pub(super) mode: ViewMode,
    pub(super) toast: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(super) enum ViewMode {
    #[default]
    Browse,
    Edit(Box<PreviewEdit>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum TuiRequest {
    Continue,
    Quit,
    UpdateSelectedRow,
    YankCell(String),
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub(super) enum Focus {
    #[default]
    Table,
    Preview,
}

impl GridViewState {
    pub(super) fn new() -> Self {
        Self {
            visible_rows: 1,
            visible_cols: 1,
            visible_preview_rows: 1,
            ..Self::default()
        }
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        grid: &ResultGrid,
        keybindings: &TuiKeybindings,
    ) -> TuiRequest {
        self.clear_toast();

        if self.is_editing() {
            self.handle_edit_key(key, grid, keybindings);
            self.clamp_to_grid(grid);
            return TuiRequest::Continue;
        }

        let Some(action) = keybindings.action_for(key) else {
            return TuiRequest::Continue;
        };

        match action {
            TuiAction::Quit => return TuiRequest::Quit,
            TuiAction::TogglePreview => self.toggle_preview(),
            TuiAction::FocusNext => self.focus_next(),
            TuiAction::EditCell => self.start_edit(grid),
            TuiAction::StageChange => {}
            TuiAction::StageNull if self.focus == Focus::Preview => self.stage_null(grid),
            TuiAction::StageNull => {}
            TuiAction::UpdateRow => return TuiRequest::UpdateSelectedRow,
            TuiAction::YankCell => return self.yank_selected_cell(grid),
            _ if self.focus == Focus::Preview => self.apply_preview_action(action),
            _ => self.apply_table_action(action, grid),
        }
        self.clamp_to_grid(grid);
        TuiRequest::Continue
    }

    pub(super) fn preview_content(&mut self, grid: &ResultGrid, width: u16) -> PreviewContent {
        if let Some(cache) = &self.preview_cache
            && cache.row == self.selected_row
            && cache.column == self.selected_col
            && cache.width == width
            && cache.editing == self.is_editing()
        {
            return cache.content.clone();
        }

        let value = self
            .display_cell_text(grid, self.selected_row, self.selected_col)
            .unwrap_or("");
        let type_name = grid.column_type(self.selected_col).unwrap_or_default();
        let content = preview_content_for_value(value, type_name, width);
        self.preview_cache = Some(PreviewCache {
            row: self.selected_row,
            column: self.selected_col,
            width,
            editing: self.is_editing(),
            content: content.clone(),
        });

        content
    }

    pub(super) fn is_editing(&self) -> bool {
        matches!(self.mode, ViewMode::Edit(_))
    }

    pub(super) fn active_edit(&self) -> Option<&PreviewEdit> {
        match &self.mode {
            ViewMode::Browse => None,
            ViewMode::Edit(edit) => Some(edit),
        }
    }

    fn active_edit_mut(&mut self) -> Option<&mut PreviewEdit> {
        match &mut self.mode {
            ViewMode::Browse => None,
            ViewMode::Edit(edit) => Some(edit),
        }
    }

    pub(super) fn set_toast(&mut self, message: impl Into<String>) {
        self.toast = Some(message.into());
    }

    pub(super) fn clear_toast(&mut self) {
        self.toast = None;
    }

    pub(super) fn invalidate_preview(&mut self) {
        self.preview_cache = None;
    }

    pub(super) fn display_cell_text<'a>(
        &'a self,
        grid: &'a ResultGrid,
        row: usize,
        column: usize,
    ) -> Option<&'a str> {
        if let Some(edit) = self.active_edit()
            && edit.row == row
            && edit.column == column
        {
            return Some(&edit.text);
        }

        self.staged
            .get(&(row, column))
            .map(CellValue::display)
            .or_else(|| grid.cell(row, column))
    }

    pub(super) fn selected_cell_editable(&self, grid: &ResultGrid) -> bool {
        grid.row_count() > 0 && grid.is_column_editable(self.selected_col)
    }

    pub(super) fn row_is_dirty(&self, row: usize) -> bool {
        self.staged.keys().any(|(staged_row, _)| *staged_row == row)
    }

    pub(super) fn cell_style(&self, row: usize, column: usize) -> Style {
        if self.staged.contains_key(&(row, column)) {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }
    }

    pub(super) fn row_style(&self, row: usize) -> Style {
        if self.row_is_dirty(row) {
            Style::new().fg(Color::Yellow)
        } else {
            Style::default()
        }
    }

    fn start_edit(&mut self, grid: &ResultGrid) {
        if !self.selected_cell_editable(grid) {
            return;
        }

        self.preview_open = true;
        self.focus = Focus::Preview;
        let type_name = grid.column_type(self.selected_col).unwrap_or_default();
        let value = self
            .edit_cell_text(grid, self.selected_row, self.selected_col)
            .unwrap_or("");
        let text = preview_plain_text(value, type_name);
        self.mode = ViewMode::Edit(Box::new(PreviewEdit::new(
            self.selected_row,
            self.selected_col,
            text,
        )));
        self.preview_scroll = 0;
        self.invalidate_preview();
    }

    fn edit_cell_text<'a>(
        &'a self,
        grid: &'a ResultGrid,
        row: usize,
        column: usize,
    ) -> Option<&'a str> {
        match self
            .staged
            .get(&(row, column))
            .or_else(|| grid.cell_value(row, column))
        {
            Some(CellValue::Text(value)) => Some(value),
            Some(CellValue::Null) => Some(""),
            None => None,
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent, grid: &ResultGrid, keybindings: &TuiKeybindings) {
        if keybindings.matches_action(TuiAction::StageChange, key) {
            self.stage_edit_buffer(grid);
            return;
        }

        if keybindings.matches_action(TuiAction::StageNull, key) {
            self.stage_null(grid);
            return;
        }

        if key.code == KeyCode::Esc {
            self.mode = ViewMode::Browse;
            self.invalidate_preview();
            return;
        }

        let Some(edit) = self.active_edit_mut() else {
            return;
        };

        let text_changed = edit.textarea.input(key);

        if text_changed {
            edit.sync_text();
            self.invalidate_preview();
        }
    }

    fn stage_edit_buffer(&mut self, grid: &ResultGrid) {
        let ViewMode::Edit(edit) = mem::take(&mut self.mode) else {
            return;
        };
        let edit = *edit;
        let row = edit.row;
        let column = edit.column;
        let value = CellValue::Text(edit.text);
        self.stage_value(grid, row, column, value, "staged change");
        self.focus = Focus::Table;
    }

    fn stage_null(&mut self, grid: &ResultGrid) {
        if !self.selected_cell_editable(grid) {
            return;
        }

        if grid
            .column_update_info(self.selected_col)
            .is_some_and(|info| !info.nullable)
        {
            self.set_toast("column is not nullable");
            return;
        }

        self.mode = ViewMode::Browse;
        self.stage_value(
            grid,
            self.selected_row,
            self.selected_col,
            CellValue::Null,
            "staged NULL",
        );
        self.focus = Focus::Table;
    }

    fn stage_value(
        &mut self,
        grid: &ResultGrid,
        row: usize,
        column: usize,
        value: CellValue,
        message: &'static str,
    ) {
        if grid.cell_value(row, column) == Some(&value) {
            self.staged.remove(&(row, column));
            self.set_toast("cleared staged change");
        } else {
            self.staged.insert((row, column), value);
            self.set_toast(message);
        }
        self.invalidate_preview();
    }

    fn yank_selected_cell(&mut self, grid: &ResultGrid) -> TuiRequest {
        if let Some(text) = self.display_cell_text(grid, self.selected_row, self.selected_col) {
            TuiRequest::YankCell(text.to_owned())
        } else {
            self.set_toast("nothing to yank");
            TuiRequest::Continue
        }
    }

    pub(super) fn edit_cursor_position(&self, content_area: Rect) -> Option<Position> {
        let edit = self.active_edit()?;
        if content_area.width == 0 || content_area.height == 0 {
            return None;
        }

        let content_height = usize::from(content_area.height).max(1);
        let (x, y) = edit.cursor_visual_position(content_area.width);
        let y = y.checked_sub(self.preview_scroll)?;

        (y < content_height).then(|| {
            Position::new(
                content_area.x.saturating_add(x as u16),
                content_area.y.saturating_add(y as u16),
            )
        })
    }

    pub(super) fn preview_overflows(&self, line_count: usize, content_width: u16) -> bool {
        self.scrollable_preview_line_count(line_count, content_width)
            > self.visible_preview_rows.max(1)
    }

    pub(super) fn scrollable_preview_line_count(
        &self,
        line_count: usize,
        content_width: u16,
    ) -> usize {
        line_count.max(
            self.edit_cursor_visual_y(content_width)
                .map_or(0, |y| y + 1),
        )
    }

    pub(super) fn ensure_edit_cursor_visible(&mut self, content_width: u16) {
        let Some(cursor_y) = self.edit_cursor_visual_y(content_width) else {
            return;
        };
        let visible_rows = self.visible_preview_rows.max(1);

        if cursor_y < self.preview_scroll {
            self.preview_scroll = cursor_y;
        } else if cursor_y >= self.preview_scroll.saturating_add(visible_rows) {
            self.preview_scroll = cursor_y + 1 - visible_rows;
        }
    }

    fn edit_cursor_visual_y(&self, content_width: u16) -> Option<usize> {
        let edit = self.active_edit()?;
        let (_, y) = edit.cursor_visual_position(content_width);
        Some(y)
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
            TuiAction::TogglePreview
            | TuiAction::FocusNext
            | TuiAction::EditCell
            | TuiAction::StageChange
            | TuiAction::StageNull
            | TuiAction::UpdateRow
            | TuiAction::YankCell
            | TuiAction::Quit => {}
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
            TuiAction::StageNull => {}
            TuiAction::StageChange | TuiAction::UpdateRow | TuiAction::YankCell => {}
            TuiAction::TogglePreview
            | TuiAction::FocusNext
            | TuiAction::EditCell
            | TuiAction::Quit => {}
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

    pub(super) fn clamp_to_grid(&mut self, grid: &ResultGrid) {
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

    pub(super) fn visible_columns(&mut self, column_count: usize, area_width: u16) -> Range<usize> {
        if column_count == 0 {
            self.visible_cols = 1;
            return 0..0;
        }

        let visible_count = visible_column_count(column_count, area_width);
        self.visible_cols = visible_count;
        self.ensure_selected_column_visible(column_count, visible_count);
        self.col_offset..self.col_offset + visible_count
    }

    pub(super) fn set_visible_rows(&mut self, row_count: usize, area_height: u16) {
        self.visible_rows = visible_row_count(row_count, area_height);
    }

    pub(super) fn set_visible_preview_rows(&mut self, content_height: u16) {
        self.visible_preview_rows = usize::from(content_height).max(1);
    }

    pub(super) fn clamp_preview_scroll(&mut self, line_count: usize) {
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

pub(super) fn human_position(index: usize, total: usize) -> usize {
    if total == 0 { 0 } else { index + 1 }
}

pub(super) fn half_page(visible_count: usize) -> usize {
    (visible_count / 2).max(1)
}

pub(super) fn visible_row_count(row_count: usize, area_height: u16) -> usize {
    let body_height = usize::from(area_height.saturating_sub(3)).max(1);
    body_height.min(row_count.max(1))
}

pub(super) fn visible_column_count(column_count: usize, area_width: u16) -> usize {
    let usable_width = area_width.saturating_sub(2).max(1);
    let target_width = TARGET_COLUMN_WIDTH.min(usable_width).max(1);

    (usable_width / target_width)
        .max(1)
        .min(column_count as u16) as usize
}
