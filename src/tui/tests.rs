use super::{
    edit::PreviewEdit,
    preview::preview_text,
    render::{
        pane_border_style, preview_content_area, preview_open_table_percentage,
        preview_scrollbar_area, preview_scrollbar_state, status_line, toast_area,
    },
    state::{Focus, GridViewState, TuiRequest, ViewMode, visible_column_count},
    update::build_update_statement,
};
use crate::{
    config::TuiKeybindings,
    render::{CellValue, ColumnUpdateInfo, ResultGrid},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier},
    text::Text,
};
use ratatui_textarea::{CursorMove, DataCursor};
use std::ops::Range;

#[test]
fn right_arrow_moves_selection_one_column() {
    // Given
    let grid = test_grid(3, 2);
    let mut state = GridViewState::new();
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
fn status_line_hides_inactive_preview_switch_control() {
    // Given
    let grid = test_grid(3, 2);
    let state = GridViewState::new();

    // When
    let status = status_line_with_default(&grid, &state, 0..3);

    // Then
    assert_eq!(
        status,
        "row 1/2, column 1/3 | visible columns 1-3 | <enter> preview | <y> yank"
    );
}

#[test]
fn status_line_shows_preview_switch_control_when_preview_is_open() {
    // Given
    let grid = test_grid(3, 2);
    let mut state = GridViewState::new();
    state.preview_open = true;

    // When
    let status = status_line_with_default(&grid, &state, 0..3);

    // Then
    assert_eq!(
        status,
        "row 1/2, column 1/3 | visible columns 1-3 | <enter> preview | <y> yank | <tab> switch pane"
    );
}

#[test]
fn status_line_shows_update_control_for_dirty_row() {
    // Given
    let grid = test_grid(3, 2);
    let mut state = GridViewState::new();
    state
        .staged
        .insert((0, 1), CellValue::Text("changed".to_owned()));

    // When
    let status = status_line_with_default(&grid, &state, 0..3);

    // Then
    assert_eq!(
        status,
        "row 1/2, column 1/3 | visible columns 1-3 | <enter> preview | <y> yank | <ctrl-u> update row"
    );
}

#[test]
fn status_line_shows_null_control_for_nullable_preview_cell() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    state.preview_open = true;
    state.focus = Focus::Preview;

    // When
    let status = status_line_with_default(&grid, &state, 0..2);

    // Then
    assert_eq!(
        status,
        "row 1/1, column 2/2 | visible columns 1-2 | <enter> preview | <y> yank | <c> edit | <ctrl-x> NULL | <tab> switch pane"
    );
}

#[test]
fn status_line_hides_null_control_from_table_focus() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    state.preview_open = true;

    // When
    let status = status_line_with_default(&grid, &state, 0..2);

    // Then
    assert_eq!(
        status,
        "row 1/1, column 2/2 | visible columns 1-2 | <enter> preview | <y> yank | <c> edit | <tab> switch pane"
    );
}

#[test]
fn status_line_shows_stage_null_and_cancel_while_editing_nullable_cell() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    state.mode = ViewMode::Edit(Box::new(PreviewEdit::new(0, 1, "Alice".to_owned())));

    // When
    let status = status_line_with_default(&grid, &state, 0..2);

    // Then
    assert_eq!(
        status,
        "row 1/1, column 2/2 | visible columns 1-2 | <ctrl-s> stage | <ctrl-x> NULL | <esc> cancel"
    );
}

#[test]
fn status_line_omits_toast_message() {
    // Given
    let grid = test_grid(3, 2);
    let mut state = GridViewState::new();
    state.set_toast("staged change");

    // When
    let status = status_line_with_default(&grid, &state, 0..3);

    // Then
    assert_eq!(
        status,
        "row 1/2, column 1/3 | visible columns 1-3 | <enter> preview | <y> yank"
    );
}

#[test]
fn toast_area_positions_message_above_status_line() {
    // Given
    let frame_area = Rect::new(0, 0, 80, 24);

    // When
    let area = toast_area(frame_area, "saved");

    // Then
    assert_eq!(area, Some(Rect::new(70, 20, 10, 3)));
}

#[test]
fn next_key_clears_existing_toast() {
    // Given
    let grid = test_grid(2, 2);
    let mut state = GridViewState::new();
    state.set_toast("staged change");
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(state.toast.is_none());
}

#[test]
fn pane_border_style_dims_only_inactive_panes() {
    // When
    let active = pane_border_style(true);
    let inactive = pane_border_style(false);

    // Then
    assert_eq!(active.fg, None);
    assert!(!active.add_modifier.contains(Modifier::DIM));
    assert_eq!(inactive.fg, Some(Color::DarkGray));
    assert!(inactive.add_modifier.contains(Modifier::DIM));
}

#[test]
fn preview_scrollbar_is_hidden_when_content_fits() {
    // Given
    let line_count = 3;
    let visible_rows = 3;

    // When
    let scrollbar_state = preview_scrollbar_state(line_count, visible_rows, 0);

    // Then
    assert!(scrollbar_state.is_none());
}

#[test]
fn preview_scrollbar_maps_bottom_scroll_to_last_position() {
    // Given
    let line_count = 10;
    let visible_rows = 3;
    let scroll = 7;

    // When
    let scrollbar_state = preview_scrollbar_state(line_count, visible_rows, scroll);

    // Then
    assert_eq!(scrollbar_state.map(|state| state.get_position()), Some(9));
}

#[test]
fn preview_content_area_reserves_one_column_for_scrollbar() {
    // Given
    let inner_area = Rect::new(1, 2, 20, 5);

    // When
    let content_area = preview_content_area(inner_area, true);

    // Then
    assert_eq!(content_area, Rect::new(1, 2, 19, 5));
}

#[test]
fn preview_scrollbar_area_uses_inner_right_edge() {
    // Given
    let inner_area = Rect::new(1, 2, 20, 5);

    // When
    let scrollbar_area = preview_scrollbar_area(inner_area);

    // Then
    assert_eq!(scrollbar_area, Rect::new(20, 2, 1, 5));
}

#[test]
fn editing_cursor_below_view_scrolls_preview_down() {
    // Given
    let mut edit = PreviewEdit::new(0, 0, "one\ntwo\nthree".to_owned());
    edit.textarea.move_cursor(CursorMove::Bottom);
    let mut state = GridViewState::new();
    state.visible_preview_rows = 2;
    state.mode = ViewMode::Edit(Box::new(edit));

    // When
    state.ensure_edit_cursor_visible(80);

    // Then
    assert_eq!(state.preview_scroll, 1);
}

#[test]
fn editing_cursor_above_view_scrolls_preview_up() {
    // Given
    let mut state = GridViewState::new();
    state.visible_preview_rows = 2;
    state.preview_scroll = 2;
    state.mode = ViewMode::Edit(Box::new(PreviewEdit::new(
        0,
        0,
        "one\ntwo\nthree".to_owned(),
    )));

    // When
    state.ensure_edit_cursor_visible(80);

    // Then
    assert_eq!(state.preview_scroll, 0);
}

#[test]
fn preview_overflow_includes_wrapped_edit_cursor_row() {
    // Given
    let mut state = GridViewState::new();
    state.visible_preview_rows = 1;
    let mut edit = PreviewEdit::new(0, 0, "ab".to_owned());
    edit.textarea.move_cursor(CursorMove::End);
    state.mode = ViewMode::Edit(Box::new(edit));

    // When
    let overflows = state.preview_overflows(1, 2);

    // Then
    assert!(overflows);
}

#[test]
fn preview_focus_gives_preview_more_vertical_space() {
    // Then
    assert_eq!(preview_open_table_percentage(Focus::Table), 50);
    assert_eq!(preview_open_table_percentage(Focus::Preview), 20);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Quit);
}

#[test]
fn y_yanks_selected_cell() {
    // Given
    let grid = test_grid(2, 2);
    let mut state = GridViewState {
        selected_row: 1,
        selected_col: 1,
        ..GridViewState::new()
    };
    let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::YankCell("r2c2".to_owned()));
}

#[test]
fn y_yanks_staged_cell_value() {
    // Given
    let grid = test_grid(2, 2);
    let mut state = GridViewState::new();
    state
        .staged
        .insert((0, 0), CellValue::Text("changed".to_owned()));
    let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::YankCell("changed".to_owned()));
}

#[test]
fn enter_toggles_preview_without_moving_focus_from_table() {
    // Given
    let grid = test_grid(2, 2);
    let mut state = GridViewState::new();
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
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
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(state.preview_scroll, 1);
}

#[test]
fn c_on_editable_preview_starts_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(state.is_editing());
    assert_eq!(
        state.active_edit().map(|edit| edit.textarea.cursor()),
        Some(DataCursor(0, 0))
    );
}

#[test]
fn c_on_editable_table_cell_opens_preview_and_starts_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(state.preview_open);
    assert_eq!(state.focus, Focus::Preview);
    assert!(state.is_editing());
    assert_eq!(
        state.active_edit().map(|edit| edit.textarea.cursor()),
        Some(DataCursor(0, 0))
    );
}

#[test]
fn c_on_empty_editable_grid_does_not_start_editing() {
    // Given
    let grid = empty_editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(!state.preview_open);
    assert!(!state.is_editing());
}

#[test]
fn enter_on_preview_focus_toggles_preview_without_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(!state.preview_open);
    assert!(!state.is_editing());
}

#[test]
fn ctrl_s_stages_preview_edit() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let keybindings = TuiKeybindings::default();
    state.handle_key(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );
    state.handle_key(
        KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );

    // When
    let request = state.handle_key(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        &grid,
        &keybindings,
    );

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(
        state.staged.get(&(0, 1)),
        Some(&CellValue::Text("!Alice".to_owned()))
    );
    assert!(!state.is_editing());
    assert_eq!(state.focus, Focus::Table);
    assert!(state.row_is_dirty(0));
}

#[test]
fn ctrl_u_uses_textarea_undo_while_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let keybindings = TuiKeybindings::default();
    state.handle_key(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );
    state.handle_key(
        KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );

    // When
    let request = state.handle_key(
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        &grid,
        &keybindings,
    );

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(
        state.active_edit().map(|edit| edit.text.as_str()),
        Some("Alice")
    );
    assert!(state.staged.is_empty());
}

#[test]
fn ctrl_a_uses_textarea_line_start_while_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let keybindings = TuiKeybindings::default();
    state.handle_key(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );
    state.handle_key(
        KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
        &grid,
        &keybindings,
    );

    // When
    let request = state.handle_key(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        &grid,
        &keybindings,
    );

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(
        state.active_edit().map(|edit| edit.textarea.cursor()),
        Some(DataCursor(0, 0))
    );
    assert!(state.staged.is_empty());
}

#[test]
fn ctrl_x_stages_null_while_editing() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let keybindings = TuiKeybindings::default();
    state.handle_key(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        &grid,
        &keybindings,
    );

    // When
    let request = state.handle_key(
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        &grid,
        &keybindings,
    );

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert!(!state.is_editing());
    assert_eq!(state.focus, Focus::Table);
    assert_eq!(state.staged.get(&(0, 1)), Some(&CellValue::Null));
}

#[test]
fn ctrl_x_stages_null_from_editable_preview() {
    // Given
    let grid = editable_grid();
    let mut state = GridViewState::new();
    state.preview_open = true;
    state.focus = Focus::Preview;
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(state.focus, Focus::Table);
    assert_eq!(state.staged.get(&(0, 1)), Some(&CellValue::Null));
}

#[test]
fn editing_null_cell_starts_with_empty_preview_and_edit_buffer() {
    // Given
    let grid = null_editable_grid();
    let mut state = GridViewState::new();
    state.selected_col = 1;
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    let keybindings = TuiKeybindings::default();

    // When
    let request = state.handle_key(key, &grid, &keybindings);
    let preview = state.preview_content(&grid, 40);

    // Then
    assert_eq!(request, TuiRequest::Continue);
    assert_eq!(state.active_edit().map(|edit| edit.text.as_str()), Some(""));
    assert_eq!(plain_text(&preview.text), "");
}

#[test]
fn primary_key_cells_are_not_editable() {
    // Given
    let grid = editable_grid();
    let state = GridViewState::new();

    // When
    let editable = state.selected_cell_editable(&grid);

    // Then
    assert!(!editable);
}

#[test]
fn update_statement_updates_selected_row_by_primary_key() {
    // Given
    let grid = editable_grid();
    let changes = [(1, CellValue::Text("Alicia".to_owned()))];

    // When
    let statement = build_update_statement(&grid, 0, &changes)
        .expect("statement should build for editable row");

    // Then
    assert_eq!(
        statement.sql,
        "update public.users set name = $1::text::text where id = $2::text::integer returning name"
    );
    assert_eq!(statement.binds, vec!["Alicia".to_owned(), "1".to_owned()]);
    assert_eq!(statement.returning_columns, vec![1]);
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

fn editable_grid() -> ResultGrid {
    ResultGrid::new(
        vec!["id".to_owned(), "name".to_owned()],
        vec!["int4".to_owned(), "text".to_owned()],
        vec![vec!["1".to_owned(), "Alice".to_owned()]],
    )
    .with_column_update_info(vec![
        Some(column_update_info(1, "id", "integer", false, true)),
        Some(column_update_info(2, "name", "text", true, false)),
    ])
}

fn null_editable_grid() -> ResultGrid {
    ResultGrid::from_cells(
        vec!["id".to_owned(), "name".to_owned()],
        vec!["int4".to_owned(), "text".to_owned()],
        vec![None, None],
        vec![vec![CellValue::Text("1".to_owned()), CellValue::Null]],
    )
    .with_column_update_info(vec![
        Some(column_update_info(1, "id", "integer", false, true)),
        Some(column_update_info(2, "name", "text", true, false)),
    ])
}

fn empty_editable_grid() -> ResultGrid {
    ResultGrid::new(
        vec!["id".to_owned(), "name".to_owned()],
        vec!["int4".to_owned(), "text".to_owned()],
        Vec::new(),
    )
    .with_column_update_info(vec![
        Some(column_update_info(1, "id", "integer", false, true)),
        Some(column_update_info(2, "name", "text", true, false)),
    ])
}

fn column_update_info(
    attribute_no: i16,
    column_name: &str,
    data_type: &str,
    nullable: bool,
    primary_key: bool,
) -> ColumnUpdateInfo {
    ColumnUpdateInfo {
        relation_id: 42,
        relation_schema: "public".to_owned(),
        relation_name: "users".to_owned(),
        attribute_no,
        column_name: column_name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        primary_key,
        primary_key_attributes: vec![1],
    }
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

fn status_line_with_default(
    grid: &ResultGrid,
    state: &GridViewState,
    visible_columns: Range<usize>,
) -> String {
    status_line(grid, state, visible_columns, &TuiKeybindings::default())
}
