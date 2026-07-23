use std::io::{self, IsTerminal};

use sqlx::{AssertSqlSafe, PgPool, postgres::PgRow};

use crate::{
    catalog::SharedCatalog,
    config::{KeyRemaps, TuiKeybindings},
    errors::{AppResult, format_colored_sql_error, format_sql_error},
    render::{
        DisplayMode, ResultGrid, RowsDisplay, render_row_count, render_rows, render_rows_affected,
        render_rows_blank, rows_display,
    },
    sql::{first_keyword, likely_returns_rows},
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
            Ok(rows) => {
                let display =
                    add_update_metadata(statement_rows_display(&rows, display_mode), pool).await?;
                print_rows_display(display, tui_keybindings, key_remaps, Some(pool)).await?;
            }
            Err(err) => print_statement_error(&err, statement, catalog),
        }
    } else {
        match sqlx::query(AssertSqlSafe(statement.to_owned()))
            .execute(pool)
            .await
        {
            Ok(result) => println!(
                "{}",
                render_statement_status(statement, result.rows_affected())
            ),
            Err(err) => print_statement_error(&err, statement, catalog),
        }
    }

    Ok(())
}

fn print_statement_error(err: &sqlx::Error, statement: &str, catalog: &SharedCatalog) {
    eprintln!("{}", format_statement_error(err, statement, catalog));
}

pub(super) fn format_statement_error(
    err: &sqlx::Error,
    statement: &str,
    catalog: &SharedCatalog,
) -> String {
    let formatter = if io::stderr().is_terminal() {
        format_colored_sql_error
    } else {
        format_sql_error
    };
    catalog.read().map_or_else(
        |_| formatter(err, Some(statement), None),
        |catalog| formatter(err, Some(statement), Some(&catalog)),
    )
}

fn statement_rows_display(rows: &[PgRow], display_mode: DisplayMode) -> RowsDisplay {
    match display_mode {
        DisplayMode::Auto => rows_display(rows),
        DisplayMode::Inline => RowsDisplay::Inline(render_rows(rows)),
        DisplayMode::InlineBlank => RowsDisplay::Inline(render_rows_blank(rows)),
        DisplayMode::Tui if rows.is_empty() => RowsDisplay::Inline(render_row_count(0)),
        DisplayMode::Tui => RowsDisplay::Tui(ResultGrid::from_rows(rows)),
    }
}

async fn add_update_metadata(
    display: RowsDisplay,
    pool: &PgPool,
) -> Result<RowsDisplay, sqlx::Error> {
    match display {
        RowsDisplay::Tui(mut grid) => {
            grid.load_update_metadata(pool).await?;
            Ok(RowsDisplay::Tui(grid))
        }
        display => Ok(display),
    }
}

pub(super) fn render_statement_status(statement: &str, rows_affected: u64) -> String {
    if rows_affected > 0 || statement_reports_rows_affected(statement) {
        render_rows_affected(rows_affected)
    } else {
        "OK".to_owned()
    }
}

fn statement_reports_rows_affected(statement: &str) -> bool {
    first_keyword(statement).is_some_and(|keyword| {
        matches!(
            keyword.as_str(),
            "copy" | "delete" | "fetch" | "insert" | "merge" | "move" | "update"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_display_mode_keeps_empty_results_inline() {
        // Given
        let rows = [];

        // When
        let display = statement_rows_display(&rows, DisplayMode::Tui);

        // Then
        match display {
            RowsDisplay::Inline(output) => assert_eq!(output, render_row_count(0)),
            RowsDisplay::Tui(_) => panic!("empty results should not open TUI"),
        }
    }

    #[test]
    fn statement_status_renders_dml_rows_affected() {
        // Given
        let statement = "update users set active = false";

        // When
        let rendered = render_statement_status(statement, 2);

        // Then
        assert_eq!(rendered, "\u{1b}[2m(2 rows affected)\u{1b}[0m");
    }

    #[test]
    fn statement_status_renders_ok_for_ddl_without_affected_rows() {
        // Given
        let statement = "create table users(id bigint)";

        // When
        let rendered = render_statement_status(statement, 0);

        // Then
        assert_eq!(rendered, "OK");
    }
}
