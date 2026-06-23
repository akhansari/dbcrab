use std::{
    env,
    ffi::OsString,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nu_ansi_term::{Color, Style as AnsiStyle};
use reedline::{
    ColumnarMenu, Completer, EditCommand, EditMode, Editor, Emacs, FileBackedHistory, History,
    KeyCode, KeyModifiers, Menu, MenuBuilder, MenuEvent, MenuSettings, Painter, PromptEditMode,
    Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal, Suggestion,
    default_emacs_keybindings,
};
use sqlx::{AssertSqlSafe, PgPool};

use crossterm::{
    cursor,
    event::{Event, KeyEvent},
    execute,
};

use crate::{
    catalog::SharedCatalog,
    completion::{SharedCompletionLineSnapshot, SqlCompleter, shared_completion_line_snapshot},
    config::TuiKeybindings,
    errors::{AppResult, format_query_error},
    highlight::SqlHighlighter,
    meta::{self, CommandCompleter, CommandHighlighter, CommandOutcome, CommandValidator},
    prompt::{CommandPrompt, DbPrompt},
    render::{
        DisplayMode, DisplayModeState, ResultGrid, RowsDisplay, grid_display, render_grid,
        render_row_count, render_rows, rows_display,
    },
    sql::{command_status, likely_returns_rows, split_complete_statements},
    tui::show_result_grid,
    validator::SqlValidator,
};

const COMPLETION_MENU: &str = "completion_menu";
const COMMAND_COMPLETION_MENU: &str = "command_completion_menu";
const COMMAND_MODE_HOST_COMMAND: &str = "dbcrab:command-mode";
const COMMAND_CANCEL_HOST_COMMAND: &str = "dbcrab:cancel-command-mode";
const HISTORY_LIMIT: usize = 1000;
const HISTORY_EXCLUSION_PREFIX: &str = " ";
const DEFAULT_HISTORY_FILE: &str = "history";

struct FullBufferCompletionMenu<M> {
    inner: M,
    completion_line: SharedCompletionLineSnapshot,
}

impl<M> FullBufferCompletionMenu<M> {
    fn new(inner: M, completion_line: SharedCompletionLineSnapshot) -> Self {
        Self {
            inner,
            completion_line,
        }
    }

    // Reedline passes completers only `buffer[..cursor]`. SQL completion needs the
    // whole line to resolve relations and aliases that appear after the cursor.
    fn capture_editor_buffer(&self, editor: &Editor) {
        if let Ok(mut snapshot) = self.completion_line.write() {
            snapshot.update(editor.get_buffer());
        }
    }
}

impl<M: Menu> Menu for FullBufferCompletionMenu<M> {
    fn settings(&self) -> &MenuSettings {
        self.inner.settings()
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn menu_event(&mut self, event: MenuEvent) {
        self.inner.menu_event(event);
    }

    fn can_quick_complete(&self) -> bool {
        self.inner.can_quick_complete()
    }

    fn can_partially_complete(
        &mut self,
        values_updated: bool,
        editor: &mut Editor,
        completer: &mut dyn Completer,
    ) -> bool {
        self.capture_editor_buffer(editor);
        self.inner
            .can_partially_complete(values_updated, editor, completer)
    }

    fn update_values(&mut self, editor: &mut Editor, completer: &mut dyn Completer) {
        self.capture_editor_buffer(editor);
        self.inner.update_values(editor, completer);
    }

    fn update_working_details(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        painter: &Painter,
    ) {
        self.capture_editor_buffer(editor);
        self.inner
            .update_working_details(editor, completer, painter);
    }

    fn replace_in_buffer(&self, editor: &mut Editor) {
        self.inner.replace_in_buffer(editor);
    }

    fn menu_required_lines(&self, terminal_columns: u16) -> u16 {
        self.inner.menu_required_lines(terminal_columns)
    }

    fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String {
        self.inner.menu_string(available_lines, use_ansi_coloring)
    }

    fn min_rows(&self) -> u16 {
        self.inner.min_rows()
    }

    fn get_values(&self) -> &[Suggestion] {
        self.inner.get_values()
    }

    fn set_cursor_pos(&mut self, pos: (u16, u16)) {
        self.inner.set_cursor_pos(pos);
    }
}

pub async fn run(
    pool: PgPool,
    catalog: SharedCatalog,
    tui_keybindings: TuiKeybindings,
    history_context: Option<String>,
) -> AppResult<()> {
    let completion_line = shared_completion_line_snapshot();
    let completion_menu = Box::new(FullBufferCompletionMenu::new(
        ColumnarMenu::default().with_name(COMPLETION_MENU),
        completion_line.clone(),
    ));
    let display_mode = DisplayModeState::new();
    let command_mode_ready = Arc::new(AtomicBool::new(true));
    let edit_mode = Box::new(DbEditMode::new(
        completion_keybindings(),
        display_mode.clone(),
        command_mode_ready.clone(),
    ));

    let editor = match persistent_history(history_context.as_deref()) {
        Some(history) => Reedline::create().with_history(history),
        None => Reedline::create(),
    };

    let mut editor = editor
        .with_completer(Box::new(
            SqlCompleter::new(catalog.clone()).with_completion_line(completion_line),
        ))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_highlighter(Box::new(SqlHighlighter))
        .with_validator(Box::new(SqlValidator))
        .with_history_exclusion_prefix(Some(HISTORY_EXCLUSION_PREFIX.to_owned()))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .use_bracketed_paste(true);

    let prompt = DbPrompt::new(display_mode.clone());
    let mut command_editor = command_editor(catalog.clone())?;
    let command_prompt = CommandPrompt;

    loop {
        command_mode_ready.store(true, Ordering::Relaxed);
        match editor.read_line(&prompt)? {
            Signal::Success(input) => {
                if let Err(err) = editor.sync_history() {
                    eprintln!("warning: failed to persist history: {err}");
                }

                let (statements, rest) = split_complete_statements(&input);
                debug_assert!(rest.trim().is_empty(), "validator submitted incomplete SQL");

                for statement in statements {
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
            Signal::HostCommand(command) if command == COMMAND_MODE_HOST_COMMAND => {
                if run_command_mode(
                    &mut command_editor,
                    &command_prompt,
                    &pool,
                    &catalog,
                    &tui_keybindings,
                )
                .await?
                {
                    return Ok(());
                }
            }
            Signal::HostCommand(_) | Signal::ExternalBreak(_) => {}
            _ => {}
        }
    }
}

fn command_editor(catalog: SharedCatalog) -> AppResult<Reedline> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMMAND_COMPLETION_MENU));
    let history = Box::new(FileBackedHistory::new(HISTORY_LIMIT)?);

    Ok(Reedline::create()
        .with_history(history)
        .with_completer(Box::new(CommandCompleter::new(catalog)))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(Box::new(CommandEditMode::new(
            command_completion_keybindings(),
        )))
        .with_highlighter(Box::new(CommandHighlighter))
        .with_validator(Box::new(CommandValidator))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .use_bracketed_paste(true))
}

async fn run_command_mode(
    editor: &mut Reedline,
    prompt: &CommandPrompt,
    pool: &PgPool,
    catalog: &SharedCatalog,
    tui_keybindings: &TuiKeybindings,
) -> AppResult<bool> {
    move_to_current_prompt_line_start()?;

    loop {
        match editor.read_line(prompt)? {
            Signal::Success(input) if input.trim().is_empty() => return Ok(false),
            Signal::Success(input) => match meta::execute(&input, pool, catalog).await {
                Ok(CommandOutcome::None) => {}
                Ok(CommandOutcome::Exit) => return Ok(true),
                Ok(CommandOutcome::Output(output)) => {
                    render_meta_output(output, tui_keybindings, DisplayMode::Auto)?;
                }
                Err(err) => eprintln!("{err}"),
            },
            Signal::CtrlC => {
                println!("^C");
            }
            Signal::CtrlD => return Ok(false),
            Signal::HostCommand(command) if command == COMMAND_CANCEL_HOST_COMMAND => {
                return Ok(false);
            }
            Signal::HostCommand(_) | Signal::ExternalBreak(_) => return Ok(false),
            _ => return Ok(false),
        }
    }
}

fn move_to_current_prompt_line_start() -> io::Result<()> {
    // Let Reedline clear and repaint in one flush; pre-clearing here causes a visible blink.
    let mut stderr = io::stderr();
    execute!(stderr, cursor::MoveToColumn(0))
}

fn render_meta_output(
    output: meta::MetaOutput,
    tui_keybindings: &TuiKeybindings,
    display_mode: DisplayMode,
) -> AppResult<()> {
    if let [section] = output.sections.as_slice() {
        render_meta_section(section, tui_keybindings, display_mode)?;
        return Ok(());
    }

    for (index, section) in output.sections.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{}", render_section_title(&section.title));
        println!("{}", render_grid(&section.grid));
    }

    Ok(())
}

fn render_section_title(title: &str) -> String {
    AnsiStyle::new()
        .bold()
        .fg(Color::Cyan)
        .paint(title)
        .to_string()
}

fn render_meta_section(
    section: &meta::MetaSection,
    tui_keybindings: &TuiKeybindings,
    display_mode: DisplayMode,
) -> AppResult<()> {
    match grid_rows_display(section.grid.clone(), display_mode) {
        RowsDisplay::Inline(output) => println!("{output}"),
        RowsDisplay::Tui(grid) => {
            show_result_grid(&grid, tui_keybindings)?;
            println!("{}", render_row_count(grid.row_count()));
        }
    }

    Ok(())
}

fn grid_rows_display(grid: ResultGrid, display_mode: DisplayMode) -> RowsDisplay {
    match display_mode {
        DisplayMode::Auto => grid_display(grid),
        DisplayMode::Inline => RowsDisplay::Inline(render_grid(&grid)),
        DisplayMode::Full if grid.row_count() == 0 => RowsDisplay::Inline(render_row_count(0)),
        DisplayMode::Full => RowsDisplay::Tui(grid),
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
    default_history_path_from_env(history_context, env::var_os)
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
    command_mode_ready: Arc<AtomicBool>,
    tracked_sql_input: TrackedSqlInput,
}

impl DbEditMode {
    fn new(
        keybindings: reedline::Keybindings,
        display_mode: DisplayModeState,
        command_mode_ready: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner: Emacs::new(keybindings),
            display_mode,
            command_mode_ready,
            tracked_sql_input: TrackedSqlInput::default(),
        }
    }
}

impl EditMode for DbEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = Event::from(event);
        if self.command_mode_ready.load(Ordering::Relaxed) {
            self.tracked_sql_input.reset();
        }
        if is_command_mode_trigger(&event, &self.command_mode_ready) {
            self.command_mode_ready.store(false, Ordering::Relaxed);
            return ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned());
        }

        if is_display_mode_toggle_event(&event) {
            self.display_mode.cycle();
            return ReedlineEvent::Repaint;
        }

        let reedline_event = self.inner.parse_event(
            ReedlineRawEvent::try_from(event).expect("raw reedline event remains valid"),
        );
        self.tracked_sql_input.apply_event(&reedline_event);
        self.command_mode_ready
            .store(self.tracked_sql_input.is_known_empty(), Ordering::Relaxed);

        reedline_event
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

#[derive(Default)]
struct TrackedSqlInput {
    char_len: Option<usize>,
}

impl TrackedSqlInput {
    fn reset(&mut self) {
        self.char_len = Some(0);
    }

    fn is_known_empty(&self) -> bool {
        self.char_len == Some(0)
    }

    fn apply_event(&mut self, event: &ReedlineEvent) {
        match event {
            ReedlineEvent::Edit(commands) => {
                for command in commands {
                    self.apply_command(command);
                }
            }
            ReedlineEvent::Multiple(events) | ReedlineEvent::UntilFound(events) => {
                for event in events {
                    self.apply_event(event);
                }
            }
            _ => {}
        }
    }

    fn apply_command(&mut self, command: &EditCommand) {
        let Some(char_len) = self.char_len.as_mut() else {
            return;
        };

        match command {
            EditCommand::InsertChar(_) | EditCommand::InsertNewline => *char_len += 1,
            EditCommand::InsertString(value) => *char_len += value.chars().count(),
            EditCommand::Backspace | EditCommand::Delete | EditCommand::CutChar => {
                *char_len = char_len.saturating_sub(1);
            }
            EditCommand::Clear => *char_len = 0,
            EditCommand::ReplaceChars(count, value) => {
                *char_len = char_len.saturating_sub(*count) + value.chars().count();
            }
            EditCommand::ReplaceChar(_)
            | EditCommand::Complete
            | EditCommand::MoveToStart { .. }
            | EditCommand::MoveToLineStart { .. }
            | EditCommand::MoveToLineNonBlankStart { .. }
            | EditCommand::MoveToEnd { .. }
            | EditCommand::MoveToLineEnd { .. }
            | EditCommand::MoveLineUp { .. }
            | EditCommand::MoveLineDown { .. }
            | EditCommand::MoveLeft { .. }
            | EditCommand::MoveRight { .. }
            | EditCommand::MoveWordLeft { .. }
            | EditCommand::MoveBigWordLeft { .. }
            | EditCommand::MoveWordRight { .. }
            | EditCommand::MoveWordRightStart { .. }
            | EditCommand::MoveBigWordRightStart { .. }
            | EditCommand::MoveWordRightEnd { .. }
            | EditCommand::MoveBigWordRightEnd { .. }
            | EditCommand::MoveToPosition { .. }
            | EditCommand::SelectAll
            | EditCommand::CopySelection
            | EditCommand::CopyFromStart
            | EditCommand::CopyFromStartLinewise
            | EditCommand::CopyFromLineStart
            | EditCommand::CopyFromLineNonBlankStart
            | EditCommand::CopyToEnd
            | EditCommand::CopyToEndLinewise
            | EditCommand::CopyToLineEnd
            | EditCommand::CopyCurrentLine
            | EditCommand::CopyWordLeft
            | EditCommand::CopyBigWordLeft
            | EditCommand::CopyWordRight
            | EditCommand::CopyBigWordRight => {}
            _ => self.char_len = None,
        }
    }
}

struct CommandEditMode {
    inner: Emacs,
}

impl CommandEditMode {
    fn new(keybindings: reedline::Keybindings) -> Self {
        Self {
            inner: Emacs::new(keybindings),
        }
    }
}

impl EditMode for CommandEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = Event::from(event);
        if is_command_cancel_event(&event) {
            return ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned());
        }

        self.inner.parse_event(
            ReedlineRawEvent::try_from(event).expect("raw reedline event remains valid"),
        )
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

fn is_command_mode_trigger(event: &Event, command_mode_ready: &AtomicBool) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent {
            code: KeyCode::Char(':'),
            modifiers,
            ..
        }) if command_mode_ready.load(Ordering::Relaxed)
            && (modifiers.is_empty() || *modifiers == KeyModifiers::SHIFT)
    )
}

fn is_command_cancel_event(event: &Event) -> bool {
    is_command_escape_event(event) || is_command_ctrl_d_event(event)
}

fn is_command_ctrl_d_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent {
            code: KeyCode::Char('d'),
            modifiers,
            ..
        }) if modifiers.contains(KeyModifiers::CONTROL)
    )
}

fn is_command_escape_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent {
            code: KeyCode::Esc,
            ..
        })
    )
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
    catalog: &SharedCatalog,
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
                format_query_error(&err, Some(statement), Some(&catalog_snapshot(catalog)))
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
                format_query_error(&err, Some(statement), Some(&catalog_snapshot(catalog)))
            ),
        }
    }

    Ok(())
}

fn catalog_snapshot(catalog: &SharedCatalog) -> crate::catalog::Catalog {
    catalog
        .read()
        .expect("completion catalog is not poisoned")
        .clone()
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
    completion_keybindings_for(COMPLETION_MENU)
}

fn command_completion_keybindings() -> reedline::Keybindings {
    completion_keybindings_for(COMMAND_COMPLETION_MENU)
}

fn completion_keybindings_for(menu_name: &str) -> reedline::Keybindings {
    let mut keybindings = default_emacs_keybindings();
    let completion_event = completion_event(menu_name);

    keybindings.add_binding(KeyModifiers::NONE, KeyCode::Tab, completion_event.clone());
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char(' '), completion_event);

    keybindings
}

fn completion_event(menu_name: &str) -> ReedlineEvent {
    ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu(menu_name.to_owned()),
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
        assert_eq!(event, Some(completion_event(COMPLETION_MENU)));
    }

    #[test]
    fn ctrl_space_opens_completion_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char(' '));

        // Then
        assert_eq!(event, Some(completion_event(COMPLETION_MENU)));
    }

    #[test]
    fn alt_v_toggles_display_mode_in_place() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(
            completion_keybindings(),
            display_mode.clone(),
            Arc::new(AtomicBool::new(true)),
        );
        let event = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::ALT,
        )))
        .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, ReedlineEvent::Repaint);
        assert_eq!(display_mode.get(), DisplayMode::Full);
    }

    #[test]
    fn first_colon_enters_command_mode() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(
            completion_keybindings(),
            display_mode,
            Arc::new(AtomicBool::new(true)),
        );
        let event = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
        )))
        .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn colon_after_sql_text_stays_in_sql_mode() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(
            completion_keybindings(),
            display_mode,
            Arc::new(AtomicBool::new(true)),
        );

        // When
        let _ = edit_mode.parse_event(raw_char_event('s'));
        let colon_event = edit_mode.parse_event(raw_char_event(':'));

        // Then
        assert_eq!(
            colon_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar(':')])
        );
    }

    #[test]
    fn colon_enters_command_mode_after_erasing_sql_text() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(
            completion_keybindings(),
            display_mode,
            Arc::new(AtomicBool::new(true)),
        );

        // When
        for ch in "select *".chars() {
            let _ = edit_mode.parse_event(raw_char_event(ch));
        }
        for _ in 0.."select *".chars().count() {
            let _ = edit_mode.parse_event(raw_key_event(KeyCode::Backspace, KeyModifiers::NONE));
        }
        let colon_event = edit_mode.parse_event(raw_char_event(':'));

        // Then
        assert_eq!(
            colon_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn ctrl_l_does_not_block_command_mode_trigger() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = DbEditMode::new(
            completion_keybindings(),
            display_mode,
            Arc::new(AtomicBool::new(true)),
        );
        let clear = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        )))
        .expect("key event should be valid");
        let colon = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
        )))
        .expect("key event should be valid");

        // When
        let clear_event = edit_mode.parse_event(clear);
        let colon_event = edit_mode.parse_event(colon);

        // Then
        assert_eq!(clear_event, ReedlineEvent::ClearScreen);
        assert_eq!(
            colon_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    fn raw_char_event(ch: char) -> ReedlineRawEvent {
        raw_key_event(
            KeyCode::Char(ch),
            if ch == ':' {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            },
        )
    }

    fn raw_key_event(code: KeyCode, modifiers: KeyModifiers) -> ReedlineRawEvent {
        ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(code, modifiers)))
            .expect("key event should be valid")
    }

    #[test]
    fn command_mode_esc_cancels_command_mode() {
        // Given
        let mut edit_mode = CommandEditMode::new(command_completion_keybindings());
        let event =
            ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
                .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn command_mode_ctrl_d_cancels_command_mode() {
        // Given
        let mut edit_mode = CommandEditMode::new(command_completion_keybindings());
        let event = raw_key_event(KeyCode::Char('d'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn command_mode_ctrl_c_does_not_cancel_command_mode() {
        // Given
        let mut edit_mode = CommandEditMode::new(command_completion_keybindings());
        let event = raw_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, ReedlineEvent::CtrlC);
    }

    #[test]
    fn section_title_is_rendered_as_header() {
        // Given
        let title = "Columns";

        // When
        let rendered = render_section_title(title);

        // Then
        assert_eq!(rendered, "\u{1b}[1;36mColumns\u{1b}[0m");
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
