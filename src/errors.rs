use std::{error::Error, fmt, io};

use nu_ansi_term::{Color, Style};
use sqlx::postgres::{PgDatabaseError, PgErrorPosition};

use crate::catalog::Catalog;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    Io(io::Error),
    Sqlx(sqlx::Error),
    Message(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SqlErrorDetails {
    pub severity: String,
    pub sqlstate: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub friendly_hint: Option<String>,
    pub position: Option<SqlErrorPosition>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SqlErrorPosition {
    pub line: usize,
    pub column: usize,
}

impl AppError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Sqlx(err) => f.write_str(&format_sql_error(err, None, None)),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl Error for AppError {}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Sqlx(err)
    }
}

impl From<reedline::ReedlineError> for AppError {
    fn from(err: reedline::ReedlineError) -> Self {
        Self::Message(format!("line editor error: {err}"))
    }
}

pub fn format_sql_error(err: &sqlx::Error, sql: Option<&str>, catalog: Option<&Catalog>) -> String {
    format_sql_error_with_color(err, sql, catalog, false)
}

pub fn format_colored_sql_error(
    err: &sqlx::Error,
    sql: Option<&str>,
    catalog: Option<&Catalog>,
) -> String {
    format_sql_error_with_color(err, sql, catalog, true)
}

fn format_sql_error_with_color(
    err: &sqlx::Error,
    sql: Option<&str>,
    catalog: Option<&Catalog>,
    color: bool,
) -> String {
    let Some(details) = sql_error_details(err, sql, catalog) else {
        return if color {
            format!(
                "{} {err}",
                Style::new().bold().fg(Color::Red).paint("error:")
            )
        } else {
            err.to_string()
        };
    };

    format_sql_error_details(&details, sql, color)
}

fn format_sql_error_details(details: &SqlErrorDetails, sql: Option<&str>, color: bool) -> String {
    let (severity, sqlstate) = if color {
        (
            Style::new()
                .bold()
                .fg(Color::Red)
                .paint(details.severity.as_str())
                .to_string(),
            Color::Red.paint(details.sqlstate.as_str()).to_string(),
        )
    } else {
        (details.severity.clone(), details.sqlstate.clone())
    };

    let mut lines = vec![format!("{severity} [{sqlstate}]: {}", details.message)];

    if let Some(detail) = &details.detail {
        let label = if color {
            Style::new().dimmed().paint("detail:").to_string()
        } else {
            "detail:".to_owned()
        };
        lines.push(format!("{label} {detail}"));
    }

    if let Some(sql) = sql
        && let Some(position) = details.position
        && let Some(caret) = format_error_caret(sql, position, color)
    {
        lines.push(caret);
    }

    if let Some(hint) = &details.hint {
        let label = if color {
            Color::Yellow.paint("hint:").to_string()
        } else {
            "hint:".to_owned()
        };
        lines.push(format!("{label} {hint}"));
    }

    if let Some(help) = &details.friendly_hint {
        let label = if color {
            Color::Cyan.paint("help:").to_string()
        } else {
            "help:".to_owned()
        };
        lines.push(format!("{label} {help}"));
    }

    lines.join("\n")
}

pub fn sql_error_details(
    err: &sqlx::Error,
    sql: Option<&str>,
    catalog: Option<&Catalog>,
) -> Option<SqlErrorDetails> {
    let pg = pg_error(err)?;

    Some(SqlErrorDetails {
        severity: format!("{:?}", pg.severity()).to_ascii_lowercase(),
        sqlstate: pg.code().to_owned(),
        message: pg.message().to_owned(),
        detail: pg.detail().map(str::to_owned),
        hint: pg.hint().map(str::to_owned),
        friendly_hint: friendly_hint(pg, catalog),
        position: sql.and_then(|sql| error_position(sql, pg.position())),
    })
}

fn pg_error(err: &sqlx::Error) -> Option<&PgDatabaseError> {
    err.as_database_error()
        .and_then(|db| db.as_error().downcast_ref::<PgDatabaseError>())
}

fn friendly_hint(pg: &PgDatabaseError, catalog: Option<&Catalog>) -> Option<String> {
    match pg.code() {
        "28P01" => Some("password authentication failed; check the password or pg_hba.conf".into()),
        "3D000" => Some(
            "database does not exist; verify the database name in the connection string".into(),
        ),
        "42601" => Some("syntax error; check nearby punctuation, keywords, and parentheses".into()),
        "42P01" => undefined_relation_hint(pg.message(), catalog),
        "42703" => undefined_column_hint(pg.message(), catalog),
        _ => None,
    }
}

fn undefined_relation_hint(message: &str, catalog: Option<&Catalog>) -> Option<String> {
    let name = first_quoted_fragment(message)?;
    let suggestion = catalog.and_then(|catalog| catalog.closest_relation(&name));
    Some(match suggestion {
        Some(relation) => format!("relation `{name}` was not found; did you mean `{relation}`?"),
        None => format!("relation `{name}` was not found; check schema qualification and spelling"),
    })
}

fn undefined_column_hint(message: &str, catalog: Option<&Catalog>) -> Option<String> {
    let name = first_quoted_fragment(message)?;
    let suggestion = catalog.and_then(|catalog| catalog.closest_column(&name));
    Some(match suggestion {
        Some(column) => format!("column `{name}` was not found; did you mean `{column}`?"),
        None => format!("column `{name}` was not found; check the selected table or alias"),
    })
}

fn first_quoted_fragment(message: &str) -> Option<String> {
    let start = message.find('"')? + 1;
    let end = message[start..].find('"')? + start;
    Some(message[start..end].to_owned())
}

#[cfg(test)]
pub fn error_caret(sql: &str, position: Option<PgErrorPosition<'_>>) -> Option<String> {
    format_error_caret(sql, error_position(sql, position)?, false)
}

pub fn error_position(
    sql: &str,
    position: Option<PgErrorPosition<'_>>,
) -> Option<SqlErrorPosition> {
    let position = original_error_position(position)?;
    let target = position.saturating_sub(1);
    let mut char_index = 0;

    for (line_index, line) in sql.lines().enumerate() {
        let line_len = line.chars().count();
        if target <= char_index + line_len {
            return Some(SqlErrorPosition {
                line: line_index + 1,
                column: target.saturating_sub(char_index) + 1,
            });
        }
        char_index += line_len + 1;
    }

    None
}

fn original_error_position(position: Option<PgErrorPosition<'_>>) -> Option<usize> {
    match position? {
        PgErrorPosition::Original(position) => Some(position),
        PgErrorPosition::Internal { .. } => None,
    }
}

fn format_error_caret(sql: &str, position: SqlErrorPosition, color: bool) -> Option<String> {
    let line = sql.lines().nth(position.line.checked_sub(1)?)?;
    let padding = " ".repeat(position.column.checked_sub(1)?);
    let caret = if color {
        Color::Red.paint("^").to_string()
    } else {
        "^".to_owned()
    };

    Some(format!("{line}\n{padding}{caret}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sql_error_details() -> SqlErrorDetails {
        SqlErrorDetails {
            severity: "error".to_owned(),
            sqlstate: "42P01".to_owned(),
            message: "relation \"customerss\" does not exist".to_owned(),
            detail: Some("The relation is missing.".to_owned()),
            hint: Some("Check the table name.".to_owned()),
            friendly_hint: Some("Did you mean `customers`?".to_owned()),
            position: Some(SqlErrorPosition {
                line: 1,
                column: 15,
            }),
        }
    }

    #[test]
    fn colored_sql_error_uses_semantic_accents() {
        // Given
        let details = test_sql_error_details();

        // When
        let rendered = format_sql_error_details(&details, Some("select * from customerss"), true);

        // Then
        assert_eq!(
            rendered,
            concat!(
                "\u{1b}[1;31merror\u{1b}[0m [\u{1b}[31m42P01\u{1b}[0m]: relation \"customerss\" does not exist\n",
                "\u{1b}[2mdetail:\u{1b}[0m The relation is missing.\n",
                "select * from customerss\n",
                "              \u{1b}[31m^\u{1b}[0m\n",
                "\u{1b}[33mhint:\u{1b}[0m Check the table name.\n",
                "\u{1b}[36mhelp:\u{1b}[0m Did you mean `customers`?",
            )
        );
    }

    #[test]
    fn plain_sql_error_preserves_existing_layout() {
        // Given
        let details = test_sql_error_details();

        // When
        let rendered = format_sql_error_details(&details, Some("select * from customerss"), false);

        // Then
        assert_eq!(
            rendered,
            concat!(
                "error [42P01]: relation \"customerss\" does not exist\n",
                "detail: The relation is missing.\n",
                "select * from customerss\n",
                "              ^\n",
                "hint: Check the table name.\n",
                "help: Did you mean `customers`?",
            )
        );
    }

    #[test]
    fn colored_unstructured_error_adds_red_error_label() {
        // Given
        let err = sqlx::Error::Protocol("connection closed".to_owned());

        // When
        let rendered = format_colored_sql_error(&err, None, None);

        // Then
        assert_eq!(
            rendered,
            "\u{1b}[1;31merror:\u{1b}[0m encountered unexpected or invalid data: connection closed"
        );
    }

    #[test]
    fn error_caret_points_to_original_sql_position() {
        // Given
        let sql = "select *\nfrom missing";

        // When
        let caret = error_caret(sql, Some(PgErrorPosition::Original(15)));

        // Then
        assert_eq!(caret.as_deref(), Some("from missing\n     ^"));
    }

    #[test]
    fn error_position_returns_line_and_column() {
        // Given
        let sql = "select *\nfrom missing";

        // When
        let position = error_position(sql, Some(PgErrorPosition::Original(15)));

        // Then
        assert_eq!(position, Some(SqlErrorPosition { line: 2, column: 6 }));
    }

    #[test]
    fn first_quoted_fragment_extracts_database_object_name() {
        // Given
        let message = "relation \"customerss\" does not exist";

        // When
        let fragment = first_quoted_fragment(message);

        // Then
        assert_eq!(fragment.as_deref(), Some("customerss"));
    }
}
