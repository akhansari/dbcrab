mod edit;
mod history;
mod menu;
mod output;
mod query;

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
use reedline::{
    ColumnarMenu, CursorConfig, FileBackedHistory, ListMenu, MenuBuilder, Reedline, ReedlineMenu,
    Signal,
};
use sqlx::PgPool;

use crate::{
    catalog::SharedCatalog,
    completion::{SqlCompleter, shared_completion_line_snapshot},
    config::{ConfigEditMode, HISTORY_MENU, KeyRemaps, KeybindingsConfig, TuiKeybindings},
    errors::AppResult,
    highlight::SqlHighlighter,
    meta::{self, CommandCompleter, CommandHighlighter, CommandOutcome, CommandValidator},
    prompt::{CommandPrompt, DbPrompt},
    render::{DisplayMode, DisplayModeState},
    sql::split_complete_statements,
    validator::SqlValidator,
};

use self::{
    edit::{CommandEditMode, SqlEditMode, command_inner_edit_mode, sql_inner_edit_mode},
    history::{HISTORY_LIMIT, persistent_history},
    menu::FullBufferCompletionMenu,
    output::render_meta_output,
    query::execute_statement,
};

const COMPLETION_MENU: &str = "completion_menu";
const COMMAND_COMPLETION_MENU: &str = "command_completion_menu";
const COMMAND_MODE_HOST_COMMAND: &str = "dbcrab:command-mode";
const COMMAND_CANCEL_HOST_COMMAND: &str = "dbcrab:cancel-command-mode";
const HISTORY_EXCLUSION_PREFIX: &str = " ";

pub async fn run(
    pool: PgPool,
    catalog: SharedCatalog,
    edit_mode: ConfigEditMode,
    keybindings: KeybindingsConfig,
    history_context: Option<String>,
) -> AppResult<()> {
    let display_mode = DisplayModeState::new();
    let command_mode_ready = Arc::new(AtomicBool::new(true));
    let mut sql_editor = build_sql_editor(
        catalog.clone(),
        edit_mode,
        &keybindings,
        display_mode.clone(),
        command_mode_ready.clone(),
        history_context.as_deref(),
    );
    let prompt = DbPrompt::new(display_mode.clone());
    let mut command_editor = build_command_editor(catalog.clone(), edit_mode, &keybindings)?;
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
    let completion_line = shared_completion_line_snapshot();
    let completion_menu = Box::new(FullBufferCompletionMenu::new(
        ColumnarMenu::default().with_name(COMPLETION_MENU),
        completion_line.clone(),
    ));
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
            .with_completer(Box::new(
                SqlCompleter::new(catalog).with_completion_line(completion_line),
            ))
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
    edit_mode: ConfigEditMode,
    keybindings: &KeybindingsConfig,
) -> AppResult<Reedline> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMMAND_COMPLETION_MENU));
    let history = Box::new(FileBackedHistory::new(HISTORY_LIMIT)?);

    Ok(common_editor_settings(
        Reedline::create()
            .with_history(history)
            .with_completer(Box::new(CommandCompleter::new(catalog)))
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
        })
        .use_bracketed_paste(true)
}

async fn run_command_mode(
    editor: &mut Reedline,
    prompt: &CommandPrompt,
    pool: &PgPool,
    catalog: &SharedCatalog,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
) -> AppResult<bool> {
    move_to_current_prompt_line_start()?;

    loop {
        match editor.read_line(prompt)? {
            Signal::Success(input) if input.trim().is_empty() => return Ok(false),
            Signal::Success(input) => match meta::execute(&input, pool, catalog).await {
                Ok(CommandOutcome::None) => {}
                Ok(CommandOutcome::Exit) => return Ok(true),
                Ok(CommandOutcome::Output(output)) => {
                    render_meta_output(output, tui_keybindings, key_remaps, DisplayMode::Auto)
                        .await?;
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
