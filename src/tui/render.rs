use std::ops::Range;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row as TableRow, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
    },
};

use crate::{
    config::{TuiAction, TuiKeybindings},
    render::ResultGrid,
};

use super::state::{Focus, GridViewState, human_position};

pub(super) fn render_result_grid(
    frame: &mut Frame<'_>,
    grid: &ResultGrid,
    state: &mut GridViewState,
    keybindings: &TuiKeybindings,
) {
    let frame_area = frame.area();
    let areas = if state.preview_open {
        let table_height = Constraint::Percentage(preview_open_table_percentage(state.focus));
        Layout::vertical([table_height, Constraint::Min(1), Constraint::Length(1)])
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
            Paragraph::new("Result has no columns").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(pane_border_style(state.focus == Focus::Table)),
            ),
            table_area,
        );
    } else {
        let rows = grid.rows().iter().enumerate().map(|(row_index, _row)| {
            let dirty = state.row_is_dirty(row_index);
            TableRow::new(visible_columns.clone().map(|index| {
                let value = state
                    .display_cell_text(grid, row_index, index)
                    .unwrap_or("");
                let value = dirty_cell_text(dirty && index == visible_columns.start, value);
                Cell::from(compact_cell(&value)).style(state.cell_style(row_index, index))
            }))
            .style(state.row_style(row_index))
        });
        let header = TableRow::new(
            visible_columns
                .clone()
                .map(|index| Cell::from(grid.columns()[index].clone())),
        )
        .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let widths =
            (0..visible_columns.len()).map(|_| Constraint::Ratio(1, visible_columns.len() as u32));
        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(pane_border_style(state.focus == Focus::Table)),
            )
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
        let preview_block = Block::default()
            .borders(Borders::ALL)
            .border_style(pane_border_style(state.focus == Focus::Preview))
            .title(preview_title(grid, state));
        let inner_area = preview_block.inner(preview_area);
        state.set_visible_preview_rows(inner_area.height);
        let preview = state.preview_content(grid, inner_area.width);
        let content_area = preview_content_area(
            inner_area,
            state.preview_overflows(preview.line_count, inner_area.width),
        );
        let preview = if content_area.width == inner_area.width {
            preview
        } else {
            state.preview_content(grid, content_area.width)
        };
        let scroll_line_count =
            state.scrollable_preview_line_count(preview.line_count, content_area.width);
        state.ensure_edit_cursor_visible(content_area.width);
        state.clamp_preview_scroll(scroll_line_count);
        let mut scrollbar_state = preview_scrollbar_state(
            scroll_line_count,
            state.visible_preview_rows,
            state.preview_scroll,
        );
        let paragraph = Paragraph::new(preview.text)
            .wrap(Wrap { trim: false })
            .scroll((state.preview_scroll as u16, 0));
        frame.render_widget(preview_block, preview_area);
        frame.render_widget(paragraph, content_area);
        if let Some(scrollbar_state) = &mut scrollbar_state {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_style(Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM))
                .thumb_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            frame.render_stateful_widget(
                scrollbar,
                preview_scrollbar_area(inner_area),
                scrollbar_state,
            );
        }
        if let Some(position) = state.edit_cursor_position(content_area) {
            frame.set_cursor_position(position);
        }
    }

    frame.render_widget(
        Paragraph::new(status_line(grid, state, visible_columns, keybindings))
            .style(Style::new().fg(Color::DarkGray)),
        status_area,
    );
    render_toast(frame, state, frame_area);
}

fn preview_title(grid: &ResultGrid, state: &GridViewState) -> String {
    let title = grid
        .columns()
        .get(state.selected_col)
        .map_or_else(|| "Preview".to_owned(), ToOwned::to_owned);

    if state.is_editing() {
        format!("{title} [editing]")
    } else if state.selected_cell_editable(grid) {
        format!("{title} [editable]")
    } else {
        title
    }
}

fn dirty_cell_text(dirty: bool, value: &str) -> String {
    if dirty {
        format!("* {value}")
    } else {
        value.to_owned()
    }
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

pub(super) fn status_line(
    grid: &ResultGrid,
    state: &GridViewState,
    visible_columns: Range<usize>,
    keybindings: &TuiKeybindings,
) -> String {
    let row = human_position(state.selected_row, grid.row_count());
    let column = human_position(state.selected_col, grid.column_count());
    let first_visible_column = human_position(visible_columns.start, grid.column_count());
    let last_visible_column = visible_columns.end.min(grid.column_count());

    let mut controls = Vec::new();
    let selected_cell_editable = state.selected_cell_editable(grid);
    let selected_cell_nullable = selected_cell_editable
        && grid
            .column_update_info(state.selected_col)
            .is_some_and(|info| info.nullable);

    if state.is_editing() {
        push_control(&mut controls, keybindings, TuiAction::StagePreview, "stage");
        if selected_cell_nullable {
            push_control(&mut controls, keybindings, TuiAction::SetNull, "NULL");
        }
        controls.push("<esc> cancel".to_owned());
    } else {
        if grid.column_count() > 0 {
            push_control(
                &mut controls,
                keybindings,
                TuiAction::TogglePreview,
                "preview",
            );
        }
        if selected_cell_editable {
            push_control(&mut controls, keybindings, TuiAction::EditPreview, "edit");
        }
        if state.focus == Focus::Preview && selected_cell_nullable {
            push_control(&mut controls, keybindings, TuiAction::SetNull, "NULL");
        }
        if state.preview_open {
            push_control(
                &mut controls,
                keybindings,
                TuiAction::FocusNext,
                "switch pane",
            );
        }
        if state.row_is_dirty(state.selected_row) {
            push_control(
                &mut controls,
                keybindings,
                TuiAction::UpdateRow,
                "update row",
            );
        }
    }
    let status = format!(
        "row {row}/{}, column {column}/{} | visible columns {first_visible_column}-{last_visible_column}",
        grid.row_count(),
        grid.column_count(),
    );
    if controls.is_empty() {
        status
    } else {
        format!("{status} | {}", controls.join(" | "))
    }
}

fn push_control(
    controls: &mut Vec<String>,
    keybindings: &TuiKeybindings,
    action: TuiAction,
    label: &'static str,
) {
    if let Some(binding) = keybindings.display_binding_for(action) {
        controls.push(format!("<{binding}> {label}"));
    }
}

fn render_toast(frame: &mut Frame<'_>, state: &GridViewState, frame_area: Rect) {
    let Some(message) = &state.toast else {
        return;
    };
    let Some(area) = toast_area(frame_area, message) else {
        return;
    };
    let style = toast_style(message);
    let toast = Paragraph::new(message.as_str())
        .style(style)
        .block(Block::default().borders(Borders::ALL).border_style(style));

    frame.render_widget(Clear, area);
    frame.render_widget(toast, area);
}

pub(super) fn toast_area(frame_area: Rect, message: &str) -> Option<Rect> {
    if frame_area.width == 0 || frame_area.height < 4 {
        return None;
    }

    let width = (message.chars().count() as u16)
        .saturating_add(4)
        .min(frame_area.width)
        .max(frame_area.width.min(10));
    let height = 3;
    let x = frame_area
        .x
        .saturating_add(frame_area.width.saturating_sub(width));
    let y = frame_area
        .y
        .saturating_add(frame_area.height.saturating_sub(height + 1));

    Some(Rect::new(x, y, width, height))
}

fn toast_style(message: &str) -> Style {
    if message.starts_with("updated row") {
        Style::new().fg(Color::LightGreen)
    } else if message.starts_with("update failed") || message == "column is not nullable" {
        Style::new().fg(Color::LightRed)
    } else {
        Style::new().fg(Color::Yellow)
    }
}

pub(super) fn pane_border_style(active: bool) -> Style {
    if active {
        Style::default()
    } else {
        Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    }
}

pub(super) fn preview_scrollbar_state(
    line_count: usize,
    visible_rows: usize,
    scroll: usize,
) -> Option<ScrollbarState> {
    let visible_rows = visible_rows.max(1);
    if line_count <= visible_rows {
        return None;
    }

    let max_scroll = line_count.saturating_sub(visible_rows);
    let position = scroll
        .min(max_scroll)
        .saturating_mul(line_count.saturating_sub(1))
        / max_scroll;

    Some(
        ScrollbarState::new(line_count)
            .position(position)
            .viewport_content_length(visible_rows),
    )
}

pub(super) fn preview_content_area(mut inner_area: Rect, show_scrollbar: bool) -> Rect {
    if show_scrollbar && inner_area.width > 1 {
        inner_area.width = inner_area.width.saturating_sub(1);
    }

    inner_area
}

pub(super) fn preview_scrollbar_area(inner_area: Rect) -> Rect {
    Rect::new(
        inner_area.right().saturating_sub(1),
        inner_area.y,
        inner_area.width.min(1),
        inner_area.height,
    )
}

pub(super) fn preview_open_table_percentage(focus: Focus) -> u16 {
    match focus {
        Focus::Table => 50,
        Focus::Preview => 20,
    }
}
