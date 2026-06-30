use std::{error::Error, fmt, io};

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
pub struct QueryErrorDetails {
    pub severity: String,
    pub sqlstate: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub friendly_hint: Option<String>,
    pub position: Option<QueryErrorPosition>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct QueryErrorPosition {
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
            Self::Sqlx(err) => f.write_str(&format_query_error(err, None, None)),
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

pub fn format_query_error(
    err: &sqlx::Error,
    sql: Option<&str>,
    catalog: Option<&Catalog>,
) -> String {
    let Some(pg) = pg_error(err) else {
        return err.to_string();
    };

    let mut lines = vec![format!(
        "{} [{}]: {}",
        format!("{:?}", pg.severity()).to_ascii_lowercase(),
        pg.code(),
        pg.message()
    )];

    if let Some(detail) = pg.detail() {
        lines.push(format!("detail: {detail}"));
    }

    if let Some(query) = sql
        && let Some(caret) = error_caret(query, pg.position())
    {
        lines.push(caret);
    }

    if let Some(hint) = pg.hint() {
        lines.push(format!("hint: {hint}"));
    }

    if let Some(help) = friendly_hint(pg, catalog) {
        lines.push(format!("help: {help}"));
    }

    lines.join("\n")
}

pub fn query_error_details(
    err: &sqlx::Error,
    sql: Option<&str>,
    catalog: Option<&Catalog>,
) -> Option<QueryErrorDetails> {
    let pg = pg_error(err)?;

    Some(QueryErrorDetails {
        severity: format!("{:?}", pg.severity()).to_ascii_lowercase(),
        sqlstate: pg.code().to_owned(),
        message: pg.message().to_owned(),
        detail: pg.detail().map(str::to_owned),
        hint: pg.hint().map(str::to_owned),
        friendly_hint: friendly_hint(pg, catalog),
        position: sql.and_then(|query| error_position(query, pg.position())),
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

pub fn error_caret(sql: &str, position: Option<PgErrorPosition<'_>>) -> Option<String> {
    let position = original_error_position(position)?;

    let target = position.saturating_sub(1);
    let mut char_index = 0;

    for line in sql.lines() {
        let line_len = line.chars().count();
        if target <= char_index + line_len {
            let column = target.saturating_sub(char_index);
            return Some(format!("{line}\n{}^", " ".repeat(column)));
        }
        char_index += line_len + 1;
    }

    None
}

pub fn error_position(
    sql: &str,
    position: Option<PgErrorPosition<'_>>,
) -> Option<QueryErrorPosition> {
    let position = original_error_position(position)?;
    let target = position.saturating_sub(1);
    let mut char_index = 0;

    for (line_index, line) in sql.lines().enumerate() {
        let line_len = line.chars().count();
        if target <= char_index + line_len {
            return Some(QueryErrorPosition {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_caret_points_to_original_query_position() {
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
        assert_eq!(position, Some(QueryErrorPosition { line: 2, column: 6 }));
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
