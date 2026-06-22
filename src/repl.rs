use std::{env, ffi::OsString, path::PathBuf};

use reedline::{
    ColumnarMenu, EditMode, Emacs, FileBackedHistory, History, KeyCode, KeyModifiers, MenuBuilder,
    PromptEditMode, Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal,
    default_emacs_keybindings,
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
const HISTORY_LIMIT: usize = 1000;
const HISTORY_EXCLUSION_PREFIX: &str = " ";
const DEFAULT_HISTORY_FILE: &str = "history";

pub async fn run(
    pool: PgPool,
    catalog: Catalog,
    tui_keybindings: TuiKeybindings,
    history_context: Option<String>,
) -> AppResult<()> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMPLETION_MENU));
    let display_mode = DisplayModeState::new();
    let edit_mode = Box::new(DbEditMode::new(
        completion_keybindings(),
        display_mode.clone(),
    ));

    let editor = match persistent_history(history_context.as_deref()) {
        Some(history) => Reedline::create().with_history(history),
        None => Reedline::create(),
    };

    let mut editor = editor
        .with_completer(Box::new(SqlCompleter::new(catalog.clone())))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_highlighter(Box::new(SqlHighlighter))
        .with_validator(Box::new(SqlValidator))
        .with_history_exclusion_prefix(Some(HISTORY_EXCLUSION_PREFIX.to_owned()))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .use_bracketed_paste(true);

    let prompt = DbPrompt::new(display_mode.clone());

    loop {
        match editor.read_line(&prompt)? {
            Signal::Success(input) => {
                if let Err(err) = editor.sync_history() {
                    eprintln!("warning: failed to persist history: {err}");
                }

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

fn persistent_history(history_context: Option<&str>) -> Option<Box<dyn History>> {
    let Some(path) = default_history_path(history_context) else {
        eprintln!(
            "warning: persistent history disabled: set XDG_STATE_HOME, HOST_XDG_STATE_HOME, or HOME"
        );
        return None;
    };

    match FileBackedHistory::with_file(HISTORY_LIMIT, path.clone()) {
        Ok(history) => Some(Box::new(history)),
        Err(err) => {
            eprintln!(
                "warning: persistent history disabled: failed to open `{}`: {err}",
                path.display()
            );
            None
        }
    }
}

fn default_history_path(history_context: Option<&str>) -> Option<PathBuf> {
    default_history_path_from_env(history_context, |name| env::var_os(name))
}

fn default_history_path_from_env(
    history_context: Option<&str>,
    mut env_var: impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    state_home_history_path(env_var("XDG_STATE_HOME"), history_context)
        .or_else(|| state_home_history_path(env_var("HOST_XDG_STATE_HOME"), history_context))
        .or_else(|| home_history_path(env_var("HOME"), history_context))
}

fn state_home_history_path(
    path: Option<OsString>,
    history_context: Option<&str>,
) -> Option<PathBuf> {
    path.filter(|path| !path.is_empty())
        .map(|path| history_path_in_state_dir(PathBuf::from(path), history_context))
}

fn home_history_path(path: Option<OsString>, history_context: Option<&str>) -> Option<PathBuf> {
    path.filter(|path| !path.is_empty()).map(|path| {
        let state_dir = PathBuf::from(path).join(".local").join("state");
        history_path_in_state_dir(state_dir, history_context)
    })
}

fn history_path_in_state_dir(state_dir: PathBuf, history_context: Option<&str>) -> PathBuf {
    state_dir
        .join("dbcrab")
        .join("history")
        .join(history_file_name(history_context))
}

fn history_file_name(history_context: Option<&str>) -> String {
    match history_context {
        Some(context) => format!("{}.history", sanitize_history_context(context)),
        None => DEFAULT_HISTORY_FILE.to_owned(),
    }
}

fn sanitize_history_context(context: &str) -> String {
    let sanitized = context
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "context".to_owned()
    } else {
        sanitized
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
    use reedline::{HistoryItem, SearchDirection, SearchQuery};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn history_path_prefers_xdg_state_home() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/xdg-state")),
            "HOST_XDG_STATE_HOME" => Some(OsString::from("/host-state")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_uses_host_state_home_before_home() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::new()),
            "HOST_XDG_STATE_HOME" => Some(OsString::from("/host-state")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/host-state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_falls_back_to_home_local_state() {
        // Given
        let env_var = |name| match name {
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/home/alice")
                    .join(".local")
                    .join("state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_uses_context_file_inside_history_directory() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/xdg-state")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(Some("app"), env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-state")
                    .join("dbcrab")
                    .join("history")
                    .join("app.history")
            )
        );
    }

    #[test]
    fn history_context_is_sanitized_for_filename() {
        // Given
        let context = "billing/prod us";

        // When
        let file_name = history_file_name(Some(context));

        // Then
        assert_eq!(file_name, "billing_prod_us.history");
    }

    #[test]
    fn file_history_restores_multiline_entries() {
        // Given
        let path = temp_history_path("multiline");
        let input = "select *\nfrom users\nwhere id = 1;";
        {
            let mut history = FileBackedHistory::with_file(HISTORY_LIMIT, path.clone())
                .expect("history file should open");
            history
                .save(HistoryItem::from_command_line(input))
                .expect("history item should save");
            history.sync().expect("history should sync");
        }

        // When
        let history = FileBackedHistory::with_file(HISTORY_LIMIT, path.clone())
            .expect("history file should reopen");
        let entries = history
            .search(SearchQuery::everything(SearchDirection::Forward, None))
            .expect("history should search");

        // Then
        assert_eq!(entries[0].command_line, input);

        let _ = std::fs::remove_file(path);
    }

    fn temp_history_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "dbcrab-{name}-{}-{nanos}.history",
            std::process::id()
        ))
    }

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
