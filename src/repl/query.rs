use sqlx::{AssertSqlSafe, PgPool, postgres::PgRow};

use crate::{
    catalog::SharedCatalog,
    config::{KeyRemaps, TuiKeybindings},
    errors::{AppResult, format_query_error},
    render::{DisplayMode, ResultGrid, RowsDisplay, render_row_count, render_rows, rows_display},
    sql::{command_status, likely_returns_rows},
};

use super::output::print_rows_display;

pub(super) async fn execute_statement(
    pool: &PgPool,
    catalog: &SharedCatalog,
    statement: &str,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
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
            Ok(rows) => print_rows_display(
                statement_rows_display(&rows, display_mode),
                tui_keybindings,
                key_remaps,
            )?,
            Err(err) => print_query_error(&err, statement, catalog),
        }
    } else {
        match sqlx::query(AssertSqlSafe(statement.to_owned()))
            .execute(pool)
            .await
        {
            Ok(result) => println!("{}", command_status(statement, result.rows_affected())),
            Err(err) => print_query_error(&err, statement, catalog),
        }
    }

    Ok(())
}

fn print_query_error(err: &sqlx::Error, statement: &str, catalog: &SharedCatalog) {
    eprintln!("{}", format_statement_error(err, statement, catalog));
}

fn format_statement_error(err: &sqlx::Error, statement: &str, catalog: &SharedCatalog) -> String {
    catalog.read().map_or_else(
        |_| format_query_error(err, Some(statement), None),
        |catalog| format_query_error(err, Some(statement), Some(&catalog)),
    )
}

fn statement_rows_display(rows: &[PgRow], display_mode: DisplayMode) -> RowsDisplay {
    match display_mode {
        DisplayMode::Auto => rows_display(rows),
        DisplayMode::Inline => RowsDisplay::Inline(render_rows(rows)),
        DisplayMode::Full if rows.is_empty() => RowsDisplay::Inline(render_row_count(0)),
        DisplayMode::Full => RowsDisplay::Tui(ResultGrid::from_rows(rows)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
