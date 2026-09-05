mod edit;
mod history;
mod output;
mod statement;

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossterm::{
    cursor::{self, SetCursorStyle},
    execute,
};
use nu_ansi_term::{Color, Style};
use reedline::{
    ColumnarMenu, CursorConfig, FileBackedHistory, InputMode, ListMenu, MenuBuilder, Reedline,
    ReedlineMenu, Signal,
};
use sqlx::PgPool;

use crate::{
    catalog::SharedCatalog,
    completion::SqlCompleter,
    config::{
        ConfigEditMode, ConfigSource, HISTORY_MENU, KeyRemaps, KeybindingsConfig, TuiKeybindings,
    },
    errors::{AppError, AppResult},
    highlight::SqlHighlighter,
    meta::{self, CommandCompleter, CommandHighlighter, CommandOutcome, CommandValidator},
    named_sql::{NamedSqlContext, NamedStatementResult, PreparedNamedSql, execute_prepared},
    prompt::{CommandPrompt, DbPrompt},
    render::{DisplayMode, DisplayModeState, render_grid},
    sql::split_complete_statements,
    validator::SqlValidator,
};

use self::{
    edit::{CommandEditMode, SqlEditMode, command_inner_edit_mode, sql_inner_edit_mode},
    history::{HISTORY_LIMIT, persistent_history},
    output::render_meta_output,
    statement::{execute_statement, format_statement_error, render_statement_status},
};

const COMPLETION_MENU: &str = "completion_menu";
const COMMAND_COMPLETION_MENU: &str = "command_completion_menu";
const COMMAND_MODE_HOST_COMMAND: &str = "dbcrab:command-mode";
const COMMAND_CANCEL_HOST_COMMAND: &str = "dbcrab:cancel-command-mode";
const HISTORY_EXCLUSION_PREFIX: &str = " ";

#[derive(Clone, Copy)]
struct CommandContext<'a> {
    named_sql: &'a NamedSqlContext,
    config_source: &'a ConfigSource,
}

pub async fn run(
    pool: PgPool,
    catalog: SharedCatalog,
    edit_mode: ConfigEditMode,
    keybindings: KeybindingsConfig,
    config_source: ConfigSource,
    named_sql: NamedSqlContext,
) -> AppResult<()> {
    let display_mode = DisplayModeState::new();
    let command_mode_ready = Arc::new(AtomicBool::new(true));
    let mut sql_editor = build_sql_editor(
        catalog.clone(),
        edit_mode,
        &keybindings,
        display_mode.clone(),
        command_mode_ready.clone(),
        named_sql.history_context(),
    );
    let prompt = DbPrompt::new(display_mode.clone());
    let mut command_editor =
        build_command_editor(catalog.clone(), named_sql.clone(), edit_mode, &keybindings)?;
    let command_prompt = CommandPrompt;

    loop {
        command_mode_ready.store(true, Ordering::Relaxed);
        match sql_editor.read_line(&prompt)? {
            Signal::Success(input) => {
                if let Err(err) = sql_editor.sync_history() {
                    eprintln!("warning: failed to persist history: {err}");
                }

                let (statements, rest) = split_complete_statements(&input);
                debug_assert!(rest.trim().is_empty(), "validator submitted incomplete SQL");

                for statement in statements {
                    execute_statement(
                        &pool,
                        &catalog,
                        &statement,
                        &keybindings.tui,
                        &keybindings.remaps,
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
                    CommandContext {
                        named_sql: &named_sql,
                        config_source: &config_source,
                    },
                    &keybindings.tui,
                    &keybindings.remaps,
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

fn build_sql_editor(
    catalog: SharedCatalog,
    edit_mode: ConfigEditMode,
    keybindings: &KeybindingsConfig,
    display_mode: DisplayModeState,
    command_mode_ready: Arc<AtomicBool>,
    history_context: Option<&str>,
) -> Reedline {
    let completion_menu = Box::new(
        ColumnarMenu::default()
            .with_name(COMPLETION_MENU)
            .with_input_mode(InputMode::FullBuffer),
    );
    let sql_edit_mode = Box::new(SqlEditMode::new(
        sql_inner_edit_mode(edit_mode, keybindings),
        keybindings.remaps.clone(),
        keybindings.prompt.cycle_display.clone(),
        keybindings.prompt.command_mode.clone(),
        display_mode,
        command_mode_ready,
    ));

    let sql_editor = match persistent_history(history_context) {
        Some(history) => Reedline::create().with_history(history),
        None => Reedline::create(),
    };

    common_editor_settings(
        sql_editor
            .with_completer(Box::new(SqlCompleter::new(catalog)))
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_menu(ReedlineMenu::HistoryMenu(Box::new(
                ListMenu::default()
                    .with_name(HISTORY_MENU)
                    .with_only_buffer_difference(false),
            )))
            .with_edit_mode(sql_edit_mode)
            .with_highlighter(Box::new(SqlHighlighter))
            .with_validator(Box::new(SqlValidator))
            .with_history_exclusion_prefix(Some(HISTORY_EXCLUSION_PREFIX.to_owned())),
    )
}

fn build_command_editor(
    catalog: SharedCatalog,
    named_sql: NamedSqlContext,
    edit_mode: ConfigEditMode,
    keybindings: &KeybindingsConfig,
) -> AppResult<Reedline> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMMAND_COMPLETION_MENU));
    let history = Box::new(FileBackedHistory::new(HISTORY_LIMIT)?);

    Ok(common_editor_settings(
        Reedline::create()
            .with_history(history)
            .with_completer(Box::new(CommandCompleter::new(catalog, named_sql)))
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_menu(ReedlineMenu::HistoryMenu(Box::new(
                ListMenu::default()
                    .with_name(HISTORY_MENU)
                    .with_only_buffer_difference(false),
            )))
            .with_edit_mode(Box::new(CommandEditMode::new(
                command_inner_edit_mode(edit_mode, keybindings),
                keybindings.command.clone(),
                keybindings.remaps.clone(),
            )))
            .with_highlighter(Box::new(CommandHighlighter))
            .with_validator(Box::new(CommandValidator)),
    ))
}

fn common_editor_settings(editor: Reedline) -> Reedline {
    editor
        .with_quick_completions(true)
        .with_partial_completions(true)
        .with_cursor_config(CursorConfig {
            vi_insert: Some(SetCursorStyle::SteadyBar),
            vi_normal: Some(SetCursorStyle::SteadyBlock),
            emacs: None,
            ..CursorConfig::default()
        })
        .with_visual_selection_style(Style::new().on(Color::DarkGray))
        .use_bracketed_paste(true)
        .use_kitty_keyboard_enhancement(true)
}

async fn run_command_mode(
    editor: &mut Reedline,
    prompt: &CommandPrompt,
    pool: &PgPool,
    catalog: &SharedCatalog,
    context: CommandContext<'_>,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
) -> AppResult<bool> {
    move_to_current_prompt_line_start()?;

    loop {
        match editor.read_line(prompt)? {
            Signal::Success(input) if input.trim().is_empty() => return Ok(false),
            Signal::Success(input) => {
                match meta::execute(
                    &input,
                    pool,
                    catalog,
                    context.named_sql,
                    context.config_source,
                )
                .await
                {
                    Ok(CommandOutcome::None) => {}
                    Ok(CommandOutcome::Exit) => return Ok(true),
                    Ok(CommandOutcome::Output(output)) => {
                        render_meta_output(output, tui_keybindings, key_remaps, DisplayMode::Auto)
                            .await?;
                    }
                    Ok(CommandOutcome::Run(prepared)) => {
                        run_named_sql(pool, catalog, &prepared).await;
                    }
                    Ok(CommandOutcome::RunFailed(error)) => {
                        eprintln!("{error}");
                        eprintln!("FAILED");
                    }
                    Err(err) => eprintln!("{err}"),
                }
            }
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

async fn run_named_sql(pool: &PgPool, catalog: &SharedCatalog, prepared: &PreparedNamedSql) {
    let result = execute_prepared(pool, prepared, false, None, |output| match output.result {
        NamedStatementResult::Rows(grid) => println!("{}", render_grid(&grid)),
        NamedStatementResult::RowsAffected(rows_affected) => println!(
            "{}",
            render_statement_status(&output.statement, rows_affected)
        ),
    })
    .await;

    match result {
        Ok(_) => {}
        Err(failure) => {
            match (&failure.error, failure.statement.as_deref()) {
                (AppError::Sqlx(err), Some(statement)) => {
                    eprintln!("{}", format_statement_error(err, statement, catalog));
                }
                (error, _) => eprintln!("{error}"),
            }
            if failure.rolled_back {
                eprintln!("ROLLED BACK");
            } else {
                eprintln!("FAILED");
            }
        }
    }
}

fn move_to_current_prompt_line_start() -> io::Result<()> {
    // Let Reedline clear and repaint in one flush; pre-clearing here causes a visible blink.
    let mut stderr = io::stderr();
    execute!(stderr, cursor::MoveToColumn(0))
}
