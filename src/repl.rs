use reedline::{
    ColumnarMenu, EditMode, Emacs, KeyCode, KeyModifiers, MenuBuilder, PromptEditMode, Reedline,
    ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal, default_emacs_keybindings,
};
use sqlx::{AssertSqlSafe, PgPool};

use crossterm::event::{Event, KeyEvent};

use crate::{
    catalog::Catalog,
    completion::SqlCompleter,
    config::TuiKeybindings,
    errors::{AppResult, format_query_error},
    highlight::SqlHighlighter,
    prompt::DbPrompt,
    render::{
        DisplayMode, DisplayModeState, ResultGrid, RowsDisplay, render_row_count, render_rows,
        rows_display,
    },
    sql::{command_status, is_exit_statement, likely_returns_rows, split_complete_statements},
    tui::show_result_grid,
    validator::SqlValidator,
};

const COMPLETION_MENU: &str = "completion_menu";

pub async fn run(pool: PgPool, catalog: Catalog, tui_keybindings: TuiKeybindings) -> AppResult<()> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMPLETION_MENU));
    let display_mode = DisplayModeState::new();
    let edit_mode = Box::new(DbEditMode::new(
        completion_keybindings(),
        display_mode.clone(),
    ));

    let mut editor = Reedline::create()
        .with_completer(Box::new(SqlCompleter::new(catalog.clone())))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_highlighter(Box::new(SqlHighlighter))
        .with_validator(Box::new(SqlValidator))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .use_bracketed_paste(true);

    let prompt = DbPrompt::new(display_mode.clone());

    loop {
        match editor.read_line(&prompt)? {
            Signal::Success(input) => {
                let (statements, rest) = split_complete_statements(&input);
                debug_assert!(rest.trim().is_empty(), "validator submitted incomplete SQL");

                for statement in statements {
                    if is_exit_statement(&statement) {
                        return Ok(());
                    }
                    execute_statement(
                        &pool,
                        &catalog,
                        &statement,
                        &tui_keybindings,
                        display_mode.get(),
                    )
                    .await?;
                }
            }
            Signal::CtrlD => {
                println!();
                return Ok(());
            }
            Signal::CtrlC => {
                println!("^C");
            }
            Signal::HostCommand(_) | Signal::ExternalBreak(_) => {}
            _ => {}
        }
    }
}

struct DbEditMode {
    inner: Emacs,
    display_mode: DisplayModeState,
}

impl DbEditMode {
    fn new(keybindings: reedline::Keybindings, display_mode: DisplayModeState) -> Self {
        Self {
            inner: Emacs::new(keybindings),
            display_mode,
        }
    }
}

impl EditMode for DbEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = Event::from(event);
        if is_display_mode_toggle_event(&event) {
            self.display_mode.cycle();
            return ReedlineEvent::Repaint;
        }

        self.inner.parse_event(
            ReedlineRawEvent::try_from(event).expect("raw reedline event remains valid"),
        )
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

fn is_display_mode_toggle_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::ALT,
            ..
        }) if ch.eq_ignore_ascii_case(&'v')
    )
}

async fn execute_statement(
    pool: &PgPool,
    catalog: &Catalog,
    statement: &str,
    tui_keybindings: &TuiKeybindings,
    display_mode: DisplayMode,
) -> AppResult<()> {
    if statement.trim().is_empty() {
        return Ok(());
    }

    if likely_returns_rows(statement) {
        match sqlx::query(AssertSqlSafe(statement.to_owned()))
            .fetch_all(pool)
            .await
        {
            Ok(rows) => match statement_rows_display(&rows, display_mode) {
                RowsDisplay::Inline(output) => println!("{output}"),
                RowsDisplay::Tui(grid) => {
                    show_result_grid(&grid, tui_keybindings)?;
                    println!("{}", render_row_count(grid.row_count()));
                }
            },
            Err(err) => eprintln!(
                "{}",
                format_query_error(&err, Some(statement), Some(catalog))
            ),
        }
    } else {
        match sqlx::query(AssertSqlSafe(statement.to_owned()))
            .execute(pool)
            .await
        {
            Ok(result) => println!("{}", command_status(statement, result.rows_affected())),
            Err(err) => eprintln!(
                "{}",
                format_query_error(&err, Some(statement), Some(catalog))
            ),
        }
    }

    Ok(())
}

fn statement_rows_display(
    rows: &[sqlx::postgres::PgRow],
    display_mode: DisplayMode,
) -> RowsDisplay {
    match display_mode {
        DisplayMode::Auto => rows_display(rows),
        DisplayMode::Inline => RowsDisplay::Inline(render_rows(rows)),
        DisplayMode::Full if rows.is_empty() => RowsDisplay::Inline(render_row_count(0)),
        DisplayMode::Full => RowsDisplay::Tui(ResultGrid::from_rows(rows)),
    }
}

fn completion_keybindings() -> reedline::Keybindings {
    let mut keybindings = default_emacs_keybindings();
    let completion_event = completion_event();

    keybindings.add_binding(KeyModifiers::NONE, KeyCode::Tab, completion_event.clone());
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char(' '), completion_event);

    keybindings
}

fn completion_event() -> ReedlineEvent {
    ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu(COMPLETION_MENU.to_owned()),
        ReedlineEvent::MenuNext,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_opens_completion_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::NONE, KeyCode::Tab);

        // Then
        assert_eq!(event, Some(completion_event()));
    }

    #[test]
    fn ctrl_space_opens_completion_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char(' '));

        // Then
        assert_eq!(event, Some(completion_event()));
    }

    #[test]
    fn alt_v_toggles_display_mode_in_place() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(completion_keybindings(), display_mode.clone());
        let event = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::ALT,
        )))
        .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, ReedlineEvent::Repaint);
        assert_eq!(display_mode.get(), DisplayMode::Inline);
    }

    #[test]
    fn full_display_mode_keeps_empty_results_inline() {
        // Given
        let rows = [];

        // When
        let display = statement_rows_display(&rows, DisplayMode::Full);

        // Then
        match display {
            RowsDisplay::Inline(output) => assert_eq!(output, render_row_count(0)),
            RowsDisplay::Tui(_) => panic!("empty results should not open TUI"),
        }
    }
}
