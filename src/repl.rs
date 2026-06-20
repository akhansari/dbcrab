use reedline::{
    ColumnarMenu, Emacs, KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu,
    Signal, default_emacs_keybindings,
};
use sqlx::{AssertSqlSafe, PgPool};

use crate::{
    catalog::Catalog,
    completion::SqlCompleter,
    errors::{AppResult, format_query_error},
    highlight::SqlHighlighter,
    prompt::DbPrompt,
    render::render_rows,
    sql::{command_status, is_exit_statement, likely_returns_rows, split_complete_statements},
    validator::SqlValidator,
};

const COMPLETION_MENU: &str = "completion_menu";

pub async fn run(pool: PgPool, catalog: Catalog) -> AppResult<()> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMPLETION_MENU));
    let edit_mode = Box::new(Emacs::new(completion_keybindings()));

    let mut editor = Reedline::create()
        .with_completer(Box::new(SqlCompleter::new(catalog.clone())))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_highlighter(Box::new(SqlHighlighter))
        .with_validator(Box::new(SqlValidator))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .use_bracketed_paste(true);

    let prompt = DbPrompt::new();

    loop {
        match editor.read_line(&prompt)? {
            Signal::Success(input) => {
                let (statements, rest) = split_complete_statements(&input);
                debug_assert!(rest.trim().is_empty(), "validator submitted incomplete SQL");

                for statement in statements {
                    if is_exit_statement(&statement) {
                        return Ok(());
                    }
                    execute_statement(&pool, &catalog, &statement).await?;
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

async fn execute_statement(pool: &PgPool, catalog: &Catalog, statement: &str) -> AppResult<()> {
    if statement.trim().is_empty() {
        return Ok(());
    }

    if likely_returns_rows(statement) {
        match sqlx::query(AssertSqlSafe(statement.to_owned()))
            .fetch_all(pool)
            .await
        {
            Ok(rows) => println!("{}", render_rows(&rows)),
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
}
