use nu_ansi_term::{Color, Style as AnsiStyle};
use std::io;

use crossterm::terminal;
use sqlx::{Column, Row, TypeInfo, postgres::PgRow};
use tabled::{
    builder::Builder,
    settings::{Format, Modify, Style as TableStyle, Width, object::Segment, peaker::Priority},
};

pub const LARGE_RESULT_WARNING_ROWS: usize = 1_000;
pub const MAX_CELL_HEIGHT: usize = 6;
const DEFAULT_TERMINAL_WIDTH: usize = 120;
const MIN_TABLE_WIDTH: usize = 20;

pub fn render_rows(rows: &[PgRow]) -> String {
    if rows.is_empty() {
        return row_count(0);
    }

    let table = if rows.len() == 1 {
        render_expanded_table(expanded_display_builder(&rows[0]), terminal_table_width())
    } else {
        render_table(rows_builder(rows), terminal_table_width())
    };

    let mut output = String::new();
    if rows.len() > LARGE_RESULT_WARNING_ROWS {
        output.push_str(&format!(
            "warning: rendering {} rows; add LIMIT for large exploratory queries\n",
            rows.len()
        ));
    }
    output.push_str(&table);
    output.push('\n');
    output.push_str(&row_count(rows.len()));
    output
}

fn rows_builder(rows: &[PgRow]) -> Builder {
    let columns = rows[0].columns();
    let mut builder = Builder::new();
    builder.push_record(columns.iter().map(|column| column.name().to_owned()));

    for row in rows {
        builder.push_record(
            columns
                .iter()
                .enumerate()
                .map(|(index, column)| render_cell(row, index, column.type_info().name())),
        );
    }

    builder
}

fn expanded_display_builder(row: &PgRow) -> Builder {
    expanded_builder(row.columns().iter().enumerate().map(|(index, column)| {
        (
            column.name().to_owned(),
            render_expanded_cell(row, index, column.type_info().name()),
        )
    }))
}

fn expanded_builder(fields: impl IntoIterator<Item = (String, String)>) -> Builder {
    let mut builder = Builder::new();

    for (name, value) in fields {
        builder.push_record([name, value]);
    }

    builder
}

fn render_table(builder: Builder, width: usize) -> String {
    let mut table = builder.build();
    table.with(TableStyle::modern());
    table.with(
        Width::wrap(width)
            .keep_words(true)
            .priority(Priority::max(false)),
    );
    table.with(Modify::new(Segment::all()).with(Format::content(limit_cell_height)));
    table.to_string()
}

fn render_expanded_table(builder: Builder, width: usize) -> String {
    let mut table = builder.build();
    table.with(TableStyle::modern().remove_horizontals());
    table.with(
        Width::wrap(width)
            .keep_words(true)
            .priority(Priority::max(false)),
    );
    table.to_string()
}

fn render_cell(row: &PgRow, index: usize, type_name: &str) -> String {
    let type_name = type_name.to_ascii_lowercase();
    match type_name.as_str() {
        "bool" | "boolean" => decode(row, index, |value: bool| value.to_string()),
        "char" | "int2" | "smallint" => decode(row, index, |value: i16| value.to_string()),
        "int4" | "integer" | "serial" => decode(row, index, |value: i32| value.to_string()),
        "int8" | "bigint" | "bigserial" => decode(row, index, |value: i64| value.to_string()),
        "float4" | "real" => decode(row, index, |value: f32| value.to_string()),
        "float8" | "double precision" => decode(row, index, |value: f64| value.to_string()),
        "text" | "varchar" | "character varying" | "bpchar" | "character" | "name" => {
            decode(row, index, |value: String| value)
        }
        "date" => decode(row, index, |value: sqlx::types::chrono::NaiveDate| {
            value.to_string()
        }),
        "time" | "time without time zone" => {
            decode(row, index, |value: sqlx::types::chrono::NaiveTime| {
                value.to_string()
            })
        }
        "timestamp" | "timestamp without time zone" => {
            decode(row, index, |value: sqlx::types::chrono::NaiveDateTime| {
                value.to_string()
            })
        }
        "timestamptz" | "timestamp with time zone" => decode(
            row,
            index,
            |value: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>| value.to_rfc3339(),
        ),
        "uuid" => decode(row, index, |value: sqlx::types::Uuid| value.to_string()),
        "json" | "jsonb" => decode(row, index, |value: sqlx::types::JsonValue| {
            value.to_string()
        }),
        "bytea" => decode(row, index, |value: Vec<u8>| format_bytes(&value)),
        unsupported => try_decode(row, index, |value: String| value)
            .unwrap_or_else(|| format!("<{unsupported}>")),
    }
}

fn render_expanded_cell(row: &PgRow, index: usize, type_name: &str) -> String {
    match type_name.to_ascii_lowercase().as_str() {
        "json" | "jsonb" => try_decode(row, index, format_json_value)
            .unwrap_or_else(|| format!("<{}>", row.columns()[index].type_info().name())),
        _ => render_cell(row, index, type_name),
    }
}

fn format_json_value(value: serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    highlight_json(&pretty)
}

fn highlight_json(json: &str) -> String {
    let mut output = String::new();
    let mut chars = json.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => {
                let end = json_string_end(json, index, &mut chars);
                let token = &json[index..end];
                let style = if next_non_whitespace(&json[end..]) == Some(':') {
                    AnsiStyle::new().fg(Color::Cyan)
                } else {
                    AnsiStyle::new().fg(Color::Green)
                };
                output.push_str(&style.paint(token).to_string());
            }
            '-' | '0'..='9' => {
                let end = json_number_end(json, index, &mut chars);
                output.push_str(
                    &AnsiStyle::new()
                        .fg(Color::Blue)
                        .paint(&json[index..end])
                        .to_string(),
                );
            }
            't' | 'f' | 'n' if starts_json_literal(&json[index..]) => {
                let end = json_literal_end(json, index, &mut chars);
                let style = if &json[index..end] == "null" {
                    AnsiStyle::new().dimmed().fg(Color::Purple)
                } else {
                    AnsiStyle::new().fg(Color::Purple)
                };
                output.push_str(&style.paint(&json[index..end]).to_string());
            }
            '{' | '}' | '[' | ']' | ':' | ',' => {
                output.push_str(&AnsiStyle::new().dimmed().paint(ch.to_string()).to_string())
            }
            _ => output.push(ch),
        }
    }

    output
}

fn json_string_end(
    json: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '"' => return index + ch.len_utf8(),
            _ => {}
        }
    }

    json.len().max(start + 1)
}

fn json_number_end(
    json: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut end = start + 1;

    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-') {
            chars.next();
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    end.min(json.len())
}

fn json_literal_end(
    json: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut end = start + 1;

    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_ascii_alphabetic() {
            chars.next();
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    end.min(json.len())
}

fn starts_json_literal(text: &str) -> bool {
    text.starts_with("true") || text.starts_with("false") || text.starts_with("null")
}

fn next_non_whitespace(text: &str) -> Option<char> {
    text.chars().find(|ch| !ch.is_whitespace())
}

fn limit_cell_height(content: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() <= MAX_CELL_HEIGHT {
        return content.to_owned();
    }

    let content_lines = MAX_CELL_HEIGHT.saturating_sub(1);
    let mut limited = lines
        .into_iter()
        .take(content_lines)
        .collect::<Vec<_>>()
        .join("\n");

    if !limited.is_empty() {
        limited.push('\n');
    }
    limited.push_str(&truncation_marker());
    limited
}

fn truncation_marker() -> String {
    AnsiStyle::new()
        .fg(Color::DarkGray)
        .paint("...")
        .to_string()
}

fn decode<T, F>(row: &PgRow, index: usize, render: F) -> String
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    F: FnOnce(T) -> String,
{
    try_decode(row, index, render)
        .unwrap_or_else(|| format!("<{}>", row.columns()[index].type_info().name()))
}

fn try_decode<T, F>(row: &PgRow, index: usize, render: F) -> Option<String>
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    F: FnOnce(T) -> String,
{
    match row.try_get::<Option<T>, _>(index) {
        Ok(Some(value)) => Some(render(value)),
        Ok(None) => Some("(null)".to_owned()),
        Err(_) => None,
    }
}

fn format_bytes(bytes: &[u8]) -> String {
    let mut output = String::from("\\x");
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        singular.to_owned()
    } else {
        format!("{singular}s")
    }
}

fn row_count(count: usize) -> String {
    AnsiStyle::new()
        .dimmed()
        .paint(format!("({count} {})", pluralize(count, "row")))
        .to_string()
}

fn terminal_table_width() -> usize {
    table_width_from_terminal_size(terminal::size())
}

fn table_width_from_terminal_size(size: io::Result<(u16, u16)>) -> usize {
    size.map(|(width, _)| usize::from(width))
        .unwrap_or(DEFAULT_TERMINAL_WIDTH)
        .saturating_sub(1)
        .max(MIN_TABLE_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralize_uses_singular_for_one() {
        // Given
        let count = 1;

        // When
        let word = pluralize(count, "row");

        // Then
        assert_eq!(word, "row");
    }

    #[test]
    fn format_bytes_uses_postgres_hex_prefix() {
        // Given
        let bytes = [0, 10, 255];

        // When
        let formatted = format_bytes(&bytes);

        // Then
        assert_eq!(formatted, "\\x000aff");
    }

    #[test]
    fn row_count_is_dimmed() {
        // Given
        let count = 2;

        // When
        let formatted = row_count(count);

        // Then
        assert_eq!(formatted, "\u{1b}[2m(2 rows)\u{1b}[0m");
    }

    #[test]
    fn expanded_display_omits_header() {
        // Given
        let fields = vec![
            ("id".to_owned(), "1".to_owned()),
            ("email".to_owned(), "user@example.com".to_owned()),
        ];

        // When
        let table = render_expanded_table(expanded_builder(fields), 80);

        // Then
        assert!(!table.contains("field"));
        assert!(!table.contains("value"));
        assert!(table.contains("id"));
        assert!(table.contains("1"));
        assert!(table.contains("email"));
        assert!(table.contains("user@example.com"));
    }

    #[test]
    fn expanded_display_does_not_limit_cell_height() {
        // Given
        let value = (1..=MAX_CELL_HEIGHT + 1)
            .map(|line| format!("line{line}"))
            .collect::<Vec<_>>()
            .join("\n");

        // When
        let table = render_expanded_table(expanded_builder([("payload".to_owned(), value)]), 80);

        // Then
        assert!(table.contains(&format!("line{}", MAX_CELL_HEIGHT + 1)));
    }

    #[test]
    fn limit_cell_height_leaves_short_content_unchanged() {
        // Given
        let content = "one\ntwo";

        // When
        let limited = limit_cell_height(content);

        // Then
        assert_eq!(limited, content);
    }

    #[test]
    fn limit_cell_height_marks_truncated_content() {
        // Given
        let content = (1..=MAX_CELL_HEIGHT + 1)
            .map(|line| format!("line{line}"))
            .collect::<Vec<_>>()
            .join("\n");

        // When
        let limited = limit_cell_height(&content);

        // Then
        assert_eq!(limited.lines().count(), MAX_CELL_HEIGHT);
        assert!(limited.contains("line1"));
        assert!(!limited.contains(&format!("line{}", MAX_CELL_HEIGHT + 1)));
        assert!(limited.ends_with(&truncation_marker()));
    }

    #[test]
    fn json_value_is_pretty_printed_and_highlighted() {
        // Given
        let value = serde_json::json!({
            "name": "Crab",
            "active": true,
            "count": 2,
            "missing": null
        });

        // When
        let formatted = format_json_value(value);

        // Then
        assert!(formatted.contains('\n'));
        assert!(formatted.contains("\u{1b}["));
        assert!(formatted.contains("\"name\""));
        assert!(formatted.contains("\"Crab\""));
        assert!(formatted.contains("true"));
        assert!(formatted.contains("null"));
    }

    #[test]
    fn table_width_leaves_one_column_for_terminal_wrap() {
        // Given
        let size = Ok((80, 24));

        // When
        let width = table_width_from_terminal_size(size);

        // Then
        assert_eq!(width, 79);
    }

    #[test]
    fn table_width_uses_fallback_when_terminal_size_is_unavailable() {
        // Given
        let size = Err(std::io::Error::other("no tty"));

        // When
        let width = table_width_from_terminal_size(size);

        // Then
        assert_eq!(width, DEFAULT_TERMINAL_WIDTH - 1);
    }

    #[test]
    fn table_width_keeps_a_minimum_for_tiny_terminals() {
        // Given
        let size = Ok((4, 24));

        // When
        let width = table_width_from_terminal_size(size);

        // Then
        assert_eq!(width, MIN_TABLE_WIDTH);
    }
}
