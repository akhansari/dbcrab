use std::time::Instant;

use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, PgPool};

use crate::{
    catalog::{Catalog, SharedCatalog},
    errors::{AppError, AppResult, query_error_details},
    meta::{self, CommandOutcome, MetaOutput, MetaSection},
    render::{CellValue, ResultGrid},
    sql::{command_status, is_read_only_statement, likely_returns_rows, split_complete_statements},
};

pub const DEFAULT_MAX_ROWS: usize = 100;
pub const DEFAULT_STATEMENT_TIMEOUT: &str = "10s";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AgentFormat {
    Compact,
    ColumnJson,
}

impl AgentFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "compact" => Some(Self::Compact),
            "column-json" => Some(Self::ColumnJson),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AgentOptions {
    pub format: AgentFormat,
    pub max_rows: usize,
    pub read_only: bool,
    pub statement_timeout: String,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            format: AgentFormat::Compact,
            max_rows: DEFAULT_MAX_ROWS,
            read_only: true,
            statement_timeout: DEFAULT_STATEMENT_TIMEOUT.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AgentOutput {
    Statement(AgentStatementOutput),
    Meta(AgentMetaOutput),
}

#[derive(Debug, Clone)]
pub struct AgentStatementOutput {
    elapsed_ms: u128,
    result: AgentStatementResult,
}

#[derive(Debug, Clone)]
enum AgentStatementResult {
    Rows(ResultGrid),
    Status {
        status: String,
        rows_affected: Option<u64>,
    },
}

#[derive(Debug, Clone)]
pub struct AgentMetaOutput {
    elapsed_ms: u128,
    output: MetaOutput,
}

pub async fn execute_sql(
    pool: &PgPool,
    statement: &str,
    options: &AgentOptions,
) -> AppResult<AgentOutput> {
    let statement = single_statement(statement)?;
    if options.read_only && !is_read_only_statement(&statement) {
        return Err(AppError::message(
            "read-only agent mode only allows SELECT, WITH, SHOW, VALUES, TABLE, and EXPLAIN; pass --allow-write to run mutating SQL",
        ));
    }

    let started = Instant::now();
    let mut tx = pool.begin().await?;
    if options.read_only {
        sqlx::query("set transaction read only")
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("select pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(&options.statement_timeout)
        .execute(&mut *tx)
        .await?;

    let result = if likely_returns_rows(&statement) {
        let rows = sqlx::query(AssertSqlSafe(statement.clone()))
            .fetch_all(&mut *tx)
            .await?;
        AgentStatementResult::Rows(ResultGrid::from_rows(&rows))
    } else {
        let result = sqlx::query(AssertSqlSafe(statement.clone()))
            .execute(&mut *tx)
            .await?;
        AgentStatementResult::Status {
            status: command_status(&statement, result.rows_affected()),
            rows_affected: Some(result.rows_affected()),
        }
    };

    if options.read_only {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }

    Ok(AgentOutput::Statement(AgentStatementOutput {
        elapsed_ms: started.elapsed().as_millis(),
        result,
    }))
}

pub async fn execute_command(
    pool: &PgPool,
    catalog: &SharedCatalog,
    command: &str,
) -> AppResult<AgentOutput> {
    let started = Instant::now();
    let outcome = meta::execute_unattended(command, pool, catalog).await?;
    let elapsed_ms = started.elapsed().as_millis();

    Ok(match outcome {
        CommandOutcome::Output(output) => AgentOutput::Meta(AgentMetaOutput { elapsed_ms, output }),
        CommandOutcome::None => status_output(elapsed_ms, "OK", None),
        CommandOutcome::Exit => status_output(elapsed_ms, "EXIT", None),
    })
}

pub fn render_output(output: &AgentOutput, options: &AgentOptions) -> String {
    match options.format {
        AgentFormat::Compact => render_output_compact(output, options.max_rows),
        AgentFormat::ColumnJson => render_output_column_json(output, options.max_rows),
    }
}

pub fn render_error(
    err: &AppError,
    statement: Option<&str>,
    catalog: Option<&Catalog>,
    format: AgentFormat,
) -> String {
    match format {
        AgentFormat::Compact => render_error_compact(err, statement, catalog),
        AgentFormat::ColumnJson => render_error_column_json(err, statement, catalog),
    }
}

pub fn agent_guide() -> &'static str {
    r#"DBCrab agent guide:
- Prefer DBCrab for PostgreSQL inspection/querying.
- SQL: dbcrab <conn> -e "<one SQL statement>"
- Meta: dbcrab <conn> -: "<command>"
- Inspect unknown DBs before querying. To know available meta commands, run: `dbcrab <conn> -: help`.
- Defaults: read-only, --format compact, --max-rows 100, --statement-timeout 10s.
- In compact output, rows are tab-separated; null is \N; check truncated=true.
- If truncated=true, narrow the SQL or rerun with --max-rows N.
- Repair errors from sqlstate, message, detail, hint, friendly_hint, and position.
- Never use --allow-write unless the user explicitly asks for mutation.
"#
}

fn status_output(
    elapsed_ms: u128,
    status: impl Into<String>,
    rows_affected: Option<u64>,
) -> AgentOutput {
    AgentOutput::Statement(AgentStatementOutput {
        elapsed_ms,
        result: AgentStatementResult::Status {
            status: status.into(),
            rows_affected,
        },
    })
}

fn single_statement(input: &str) -> AppResult<String> {
    let (mut statements, rest) = split_complete_statements(input);
    let rest = rest.trim();

    match (statements.len(), rest.is_empty()) {
        (0, false) => Ok(rest.to_owned()),
        (1, true) => Ok(statements.remove(0)),
        (0, true) => Err(AppError::message("agent mode requires one SQL statement")),
        _ => Err(AppError::message(
            "agent mode accepts exactly one SQL statement per --execute call",
        )),
    }
}

fn render_output_compact(output: &AgentOutput, max_rows: usize) -> String {
    match output {
        AgentOutput::Statement(statement) => render_statement_compact(statement, max_rows),
        AgentOutput::Meta(meta) => render_meta_compact(meta, max_rows),
    }
}

fn render_statement_compact(output: &AgentStatementOutput, max_rows: usize) -> String {
    match &output.result {
        AgentStatementResult::Rows(grid) => {
            render_grid_compact(None, grid, output.elapsed_ms, max_rows)
        }
        AgentStatementResult::Status {
            status,
            rows_affected,
        } => {
            let mut parts = vec![
                "ok".to_owned(),
                format!("status={}", compact_header_value(status)),
                format!("elapsed_ms={}", output.elapsed_ms),
            ];
            if let Some(rows_affected) = rows_affected {
                parts.insert(2, format!("rows_affected={rows_affected}"));
            }
            format!("{}\n", parts.join(" "))
        }
    }
}

fn render_meta_compact(output: &AgentMetaOutput, max_rows: usize) -> String {
    let mut rendered = format!(
        "ok sections={} elapsed_ms={}\n",
        output.output.sections.len(),
        output.elapsed_ms
    );

    for (index, section) in output.output.sections.iter().enumerate() {
        if index > 0 {
            rendered.push('\n');
        }
        rendered.push_str(&render_section_compact(section, max_rows));
    }

    rendered
}

fn render_section_compact(section: &MetaSection, max_rows: usize) -> String {
    let mut rendered = format!("section {}\n", compact_header_value(&section.title));
    rendered.push_str(&render_grid_compact(None, &section.grid, 0, max_rows));
    rendered
}

fn render_grid_compact(
    title: Option<&str>,
    grid: &ResultGrid,
    elapsed_ms: u128,
    max_rows: usize,
) -> String {
    let returned_rows = returned_rows(grid, max_rows);
    let mut rendered = String::new();
    let mut header = vec![
        "ok".to_owned(),
        format!("rows={}", grid.row_count()),
        format!("returned={returned_rows}"),
        format!("truncated={}", is_truncated(grid, max_rows)),
    ];
    if let Some(title) = title {
        header.push(format!("title={}", compact_header_value(title)));
    }
    if elapsed_ms > 0 {
        header.push(format!("elapsed_ms={elapsed_ms}"));
    }
    rendered.push_str(&header.join(" "));
    rendered.push('\n');

    if grid.column_count() > 0 {
        rendered.push_str("cols:");
        for (name, data_type) in grid.columns().iter().zip(grid.column_types()) {
            rendered.push(' ');
            rendered.push_str(&compact_label(name));
            rendered.push(':');
            rendered.push_str(&compact_label(data_type));
        }
        rendered.push_str("\n---\n");

        for row in grid.rows().iter().take(max_rows) {
            rendered.push_str(&row.iter().map(compact_cell).collect::<Vec<_>>().join("\t"));
            rendered.push('\n');
        }
    }

    rendered
}

fn render_output_column_json(output: &AgentOutput, max_rows: usize) -> String {
    match output {
        AgentOutput::Statement(statement) => render_statement_column_json(statement, max_rows),
        AgentOutput::Meta(meta) => render_meta_column_json(meta, max_rows),
    }
}

fn render_statement_column_json(output: &AgentStatementOutput, max_rows: usize) -> String {
    let value = match &output.result {
        AgentStatementResult::Rows(grid) => grid_json("rows", grid, output.elapsed_ms, max_rows),
        AgentStatementResult::Status {
            status,
            rows_affected,
        } => json!({
            "ok": true,
            "kind": "status",
            "status": status,
            "rows_affected": rows_affected,
            "elapsed_ms": output.elapsed_ms,
        }),
    };

    format_json_line(value)
}

fn render_meta_column_json(output: &AgentMetaOutput, max_rows: usize) -> String {
    format_json_line(json!({
        "ok": true,
        "kind": "meta",
        "sections": output.output.sections.iter().map(|section| {
            json!({
                "title": section.title,
                "result": grid_json("rows", &section.grid, 0, max_rows),
            })
        }).collect::<Vec<_>>(),
        "elapsed_ms": output.elapsed_ms,
    }))
}

fn grid_json(kind: &str, grid: &ResultGrid, elapsed_ms: u128, max_rows: usize) -> Value {
    json!({
        "ok": true,
        "kind": kind,
        "columns": columns_json(grid),
        "rows": rows_json(grid, max_rows),
        "row_count": grid.row_count(),
        "returned_rows": returned_rows(grid, max_rows),
        "truncated": is_truncated(grid, max_rows),
        "elapsed_ms": elapsed_ms,
    })
}

fn columns_json(grid: &ResultGrid) -> Vec<Value> {
    grid.columns()
        .iter()
        .zip(grid.column_types())
        .map(|(name, data_type)| json!([name, data_type]))
        .collect()
}

fn rows_json(grid: &ResultGrid, max_rows: usize) -> Vec<Value> {
    grid.rows()
        .iter()
        .take(max_rows)
        .map(|row| {
            Value::Array(
                row.iter()
                    .zip(grid.column_types())
                    .map(|(cell, data_type)| cell_json(cell, data_type))
                    .collect(),
            )
        })
        .collect()
}

fn cell_json(cell: &CellValue, data_type: &str) -> Value {
    let Some(text) = cell.as_text() else {
        return Value::Null;
    };

    match data_type.to_ascii_lowercase().as_str() {
        "bool" | "boolean" => text
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| json!(text)),
        "char" | "int2" | "smallint" | "int4" | "integer" | "serial" | "int8" | "bigint"
        | "bigserial" => text
            .parse::<i64>()
            .map(|value| json!(value))
            .unwrap_or_else(|_| json!(text)),
        "float4" | "real" | "float8" | "double precision" => text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| json!(text)),
        "json" | "jsonb" => serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
        _ => json!(text),
    }
}

fn render_error_compact(
    err: &AppError,
    statement: Option<&str>,
    catalog: Option<&Catalog>,
) -> String {
    match err {
        AppError::Sqlx(err) => {
            if let Some(details) = query_error_details(err, statement, catalog) {
                let mut rendered = format!(
                    "error sqlstate={} severity={} message={}\n",
                    compact_header_value(&details.sqlstate),
                    compact_header_value(&details.severity),
                    compact_header_value(&details.message)
                );
                if let Some(position) = details.position {
                    rendered.push_str(&format!("position={}:{}\n", position.line, position.column));
                }
                if let Some(detail) = details.detail {
                    rendered.push_str(&format!("detail={}\n", compact_header_value(&detail)));
                }
                if let Some(hint) = details.hint {
                    rendered.push_str(&format!("hint={}\n", compact_header_value(&hint)));
                }
                if let Some(friendly_hint) = details.friendly_hint {
                    rendered.push_str(&format!(
                        "friendly_hint={}\n",
                        compact_header_value(&friendly_hint)
                    ));
                }
                rendered
            } else {
                format!("error message={}\n", compact_header_value(&err.to_string()))
            }
        }
        err => format!("error message={}\n", compact_header_value(&err.to_string())),
    }
}

fn render_error_column_json(
    err: &AppError,
    statement: Option<&str>,
    catalog: Option<&Catalog>,
) -> String {
    let error = match err {
        AppError::Sqlx(err) => query_error_details(err, statement, catalog).map_or_else(
            || json!({ "message": err.to_string() }),
            |details| {
                json!({
                    "sqlstate": details.sqlstate,
                    "severity": details.severity,
                    "message": details.message,
                    "detail": details.detail,
                    "hint": details.hint,
                    "friendly_hint": details.friendly_hint,
                    "position": details.position.map(|position| json!({
                        "line": position.line,
                        "column": position.column,
                    })),
                })
            },
        ),
        err => json!({ "message": err.to_string() }),
    };

    format_json_line(json!({
        "ok": false,
        "error": error,
    }))
}

fn returned_rows(grid: &ResultGrid, max_rows: usize) -> usize {
    grid.row_count().min(max_rows)
}

fn is_truncated(grid: &ResultGrid, max_rows: usize) -> bool {
    grid.row_count() > max_rows
}

fn compact_cell(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => "\\N".to_owned(),
        CellValue::Text(text) => text
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r"),
    }
}

fn compact_label(value: &str) -> String {
    if is_compact_label(value) {
        value.to_owned()
    } else {
        compact_header_value(value)
    }
}

fn compact_header_value(value: &str) -> String {
    if is_compact_header_value(value) {
        value.to_owned()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"<unprintable>\"".to_owned())
    }
}

fn is_compact_label(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn is_compact_header_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@'))
}

fn format_json_line(value: Value) -> String {
    let mut rendered = value.to_string();
    rendered.push('\n');
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_result_uses_typed_columns_and_tabs() {
        // Given
        let output = AgentOutput::Statement(AgentStatementOutput {
            elapsed_ms: 8,
            result: AgentStatementResult::Rows(ResultGrid::new(
                vec!["id".to_owned(), "email".to_owned()],
                vec!["int8".to_owned(), "text".to_owned()],
                vec![vec!["1".to_owned(), "a@example.com".to_owned()]],
            )),
        });
        let options = AgentOptions::default();

        // When
        let rendered = render_output(&output, &options);

        // Then
        assert_eq!(
            rendered,
            "ok rows=1 returned=1 truncated=false elapsed_ms=8\ncols: id:int8 email:text\n---\n1\ta@example.com\n"
        );
    }

    #[test]
    fn compact_result_escapes_cells_and_marks_nulls() {
        // Given
        let row = vec![CellValue::Text("one\ntwo".to_owned()), CellValue::Null];
        let output = AgentOutput::Statement(AgentStatementOutput {
            elapsed_ms: 1,
            result: AgentStatementResult::Rows(ResultGrid::from_cells(
                vec!["note".to_owned(), "missing".to_owned()],
                vec!["text".to_owned(), "text".to_owned()],
                vec![None, None],
                vec![row],
            )),
        });
        let options = AgentOptions::default();

        // When
        let rendered = render_output(&output, &options);

        // Then
        assert!(rendered.contains("one\\ntwo\t\\N"));
    }

    #[test]
    fn column_json_result_keeps_columnar_rows() {
        // Given
        let output = AgentOutput::Statement(AgentStatementOutput {
            elapsed_ms: 8,
            result: AgentStatementResult::Rows(ResultGrid::new(
                vec!["id".to_owned(), "active".to_owned()],
                vec!["int8".to_owned(), "bool".to_owned()],
                vec![vec!["1".to_owned(), "true".to_owned()]],
            )),
        });
        let options = AgentOptions {
            format: AgentFormat::ColumnJson,
            ..AgentOptions::default()
        };

        // When
        let rendered = render_output(&output, &options);

        // Then
        assert_eq!(
            serde_json::from_str::<Value>(&rendered).expect("column JSON should parse"),
            json!({
                "ok": true,
                "kind": "rows",
                "columns": [["id", "int8"], ["active", "bool"]],
                "rows": [[1, true]],
                "row_count": 1,
                "returned_rows": 1,
                "truncated": false,
                "elapsed_ms": 8,
            })
        );
    }

    #[test]
    fn column_json_result_reports_truncation() {
        // Given
        let output = AgentOutput::Statement(AgentStatementOutput {
            elapsed_ms: 1,
            result: AgentStatementResult::Rows(ResultGrid::new(
                vec!["id".to_owned()],
                vec!["int8".to_owned()],
                vec![vec!["1".to_owned()], vec!["2".to_owned()]],
            )),
        });
        let options = AgentOptions {
            format: AgentFormat::ColumnJson,
            max_rows: 1,
            ..AgentOptions::default()
        };

        // When
        let rendered = render_output(&output, &options);

        // Then
        assert_eq!(
            serde_json::from_str::<Value>(&rendered).expect("column JSON should parse"),
            json!({
                "ok": true,
                "kind": "rows",
                "columns": [["id", "int8"]],
                "rows": [[1]],
                "row_count": 2,
                "returned_rows": 1,
                "truncated": true,
                "elapsed_ms": 1,
            })
        );
    }

    #[test]
    fn single_statement_accepts_unterminated_sql() {
        // Given
        let input = "select 1";

        // When
        let statement = single_statement(input);

        // Then
        assert_eq!(
            statement.expect("single statement should parse"),
            "select 1"
        );
    }

    #[test]
    fn single_statement_rejects_multiple_statements() {
        // Given
        let input = "select 1; select 2;";

        // When
        let statement = single_statement(input);

        // Then
        assert!(statement.is_err());
    }
}
