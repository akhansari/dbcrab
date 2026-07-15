use nu_ansi_term::{Color, Style as AnsiStyle};
use std::{
    collections::HashMap,
    io::{self, IsTerminal},
    sync::{Arc, Mutex},
};

use crossterm::terminal;
use sqlx::{AssertSqlSafe, Column, PgPool, Row, TypeInfo, postgres::PgRow};
use tabled::{
    builder::Builder,
    settings::{
        Color as TableColor, Format, Modify, Style as TableStyle, Width,
        object::{Columns, Rows, Segment},
        peaker::Priority,
        style::BorderColor,
    },
};

pub const LARGE_RESULT_WARNING_ROWS: usize = 1_000;
pub const MAX_CELL_HEIGHT: usize = 6;
const DEFAULT_TERMINAL_WIDTH: usize = 120;
const DEFAULT_TERMINAL_HEIGHT: usize = 24;
const MIN_TABLE_WIDTH: usize = 20;
const MIN_INLINE_COLUMN_WIDTH: usize = 12;
const INLINE_TABLE_OVERHEAD_ROWS: usize = 4;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResultGrid {
    columns: Vec<String>,
    column_types: Vec<String>,
    column_origins: Vec<Option<ColumnOrigin>>,
    column_update_info: Vec<Option<ColumnUpdateInfo>>,
    rows: Vec<Vec<CellValue>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CellValue {
    Null,
    Text(String),
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct ColumnOrigin {
    pub relation_id: i64,
    pub attribute_no: i16,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ColumnUpdateInfo {
    pub relation_id: i64,
    pub relation_schema: String,
    pub relation_name: String,
    pub attribute_no: i16,
    pub column_name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub primary_key_attributes: Vec<i16>,
}

impl CellValue {
    pub fn display(&self) -> &str {
        match self {
            Self::Null => "(null)",
            Self::Text(value) => value,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Null => None,
            Self::Text(value) => Some(value),
        }
    }
}

impl ResultGrid {
    pub fn new(columns: Vec<String>, column_types: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let column_count = columns.len();
        Self {
            columns,
            column_types,
            column_origins: vec![None; column_count],
            column_update_info: vec![None; column_count],
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(CellValue::Text).collect())
                .collect(),
        }
    }

    pub fn from_cells(
        columns: Vec<String>,
        column_types: Vec<String>,
        column_origins: Vec<Option<ColumnOrigin>>,
        rows: Vec<Vec<CellValue>>,
    ) -> Self {
        let column_count = columns.len();
        Self {
            columns,
            column_types,
            column_origins,
            column_update_info: vec![None; column_count],
            rows,
        }
    }

    pub fn from_rows(rows: &[PgRow]) -> Self {
        if rows.is_empty() {
            return Self::new(Vec::new(), Vec::new(), Vec::new());
        }

        let columns = rows[0].columns();
        let column_names = columns
            .iter()
            .map(|column| column.name().to_owned())
            .collect::<Vec<_>>();
        let column_types = columns
            .iter()
            .map(|column| column.type_info().name().to_owned())
            .collect::<Vec<_>>();
        let column_origins = columns
            .iter()
            .map(|column| {
                Some(ColumnOrigin {
                    relation_id: i64::from(column.relation_id()?.0),
                    attribute_no: column.relation_attribute_no()?,
                })
            })
            .collect::<Vec<_>>();

        let rows = rows
            .iter()
            .map(|row| {
                row.columns()
                    .iter()
                    .enumerate()
                    .map(|(index, column)| render_cell_value(row, index, column.type_info().name()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Self::from_cells(column_names, column_types, column_origins, rows)
    }

    pub fn from_records(
        columns: impl IntoIterator<Item = impl Into<String>>,
        rows: impl IntoIterator<Item = impl IntoIterator<Item = impl Into<String>>>,
    ) -> Self {
        let columns = columns.into_iter().map(Into::into).collect::<Vec<_>>();
        let column_types = vec!["text".to_owned(); columns.len()];
        let rows = rows
            .into_iter()
            .map(|row| row.into_iter().map(Into::into).collect::<Vec<_>>())
            .collect::<Vec<_>>();

        Self::new(columns, column_types, rows)
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn column_types(&self) -> &[String] {
        &self.column_types
    }

    pub fn rows(&self) -> &[Vec<CellValue>] {
        &self.rows
    }

    pub fn column_update_info(&self, index: usize) -> Option<&ColumnUpdateInfo> {
        self.column_update_info.get(index).and_then(Option::as_ref)
    }

    pub fn primary_key_columns_for_relation(&self, relation_id: i64) -> Option<Vec<usize>> {
        let primary_key_attributes = self
            .column_update_info
            .iter()
            .flatten()
            .find(|info| info.relation_id == relation_id)?
            .primary_key_attributes
            .clone();

        if primary_key_attributes.is_empty() {
            return None;
        }

        primary_key_attributes
            .into_iter()
            .map(|attribute_no| {
                self.column_update_info.iter().position(|info| {
                    info.as_ref().is_some_and(|info| {
                        info.relation_id == relation_id && info.attribute_no == attribute_no
                    })
                })
            })
            .collect()
    }

    pub fn is_column_editable(&self, index: usize) -> bool {
        self.column_update_info(index).is_some_and(|info| {
            !info.primary_key
                && self
                    .primary_key_columns_for_relation(info.relation_id)
                    .is_some()
        })
    }

    pub fn column_type(&self, index: usize) -> Option<&str> {
        self.column_types.get(index).map(String::as_str)
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.cell_value(row, column).map(CellValue::display)
    }

    pub fn cell_value(&self, row: usize, column: usize) -> Option<&CellValue> {
        self.rows.get(row).and_then(|row| row.get(column))
    }

    pub fn set_cell_value(&mut self, row: usize, column: usize, value: CellValue) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|row| row.get_mut(column)) {
            *cell = value;
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub async fn load_update_metadata(&mut self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let relation_ids = relation_ids(&self.column_origins);
        if relation_ids.is_empty() {
            return Ok(());
        }

        let rows = sqlx::query(AssertSqlSafe(UPDATE_METADATA_SQL.to_owned()))
            .bind(&relation_ids)
            .fetch_all(pool)
            .await?;
        let metadata = rows
            .into_iter()
            .map(|row| {
                Ok((
                    (
                        row.try_get::<i64, _>("relation_id")?,
                        row.try_get::<i16, _>("attribute_no")?,
                    ),
                    ColumnUpdateInfo {
                        relation_id: row.try_get("relation_id")?,
                        relation_schema: row.try_get("relation_schema")?,
                        relation_name: row.try_get("relation_name")?,
                        attribute_no: row.try_get("attribute_no")?,
                        column_name: row.try_get("column_name")?,
                        data_type: row.try_get("data_type")?,
                        nullable: row.try_get("nullable")?,
                        primary_key: row.try_get("primary_key")?,
                        primary_key_attributes: row.try_get("primary_key_attributes")?,
                    },
                ))
            })
            .collect::<Result<HashMap<_, _>, sqlx::Error>>()?;

        self.column_update_info = self
            .column_origins
            .iter()
            .map(|origin| {
                origin.and_then(|origin| {
                    metadata
                        .get(&(origin.relation_id, origin.attribute_no))
                        .cloned()
                })
            })
            .collect();

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_column_update_info(
        mut self,
        column_update_info: Vec<Option<ColumnUpdateInfo>>,
    ) -> Self {
        self.column_update_info = column_update_info;
        self
    }
}

const UPDATE_METADATA_SQL: &str = r#"
select c.oid::bigint as relation_id,
       n.nspname as relation_schema,
       c.relname as relation_name,
       a.attnum::int2 as attribute_no,
       a.attname as column_name,
       pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type,
       not a.attnotnull as nullable,
       exists (
           select 1
           from pg_catalog.pg_index i
           where i.indrelid = c.oid
             and i.indisprimary
             and a.attnum = any(i.indkey)
       ) as primary_key,
       coalesce(pk.primary_key_attributes, array[]::int2[]) as primary_key_attributes
from pg_catalog.pg_class c
join pg_catalog.pg_namespace n on n.oid = c.relnamespace
join pg_catalog.pg_attribute a on a.attrelid = c.oid
left join lateral (
    select array_agg(key.attnum::int2 order by key.ordinality) as primary_key_attributes
    from pg_catalog.pg_index i
    cross join lateral unnest(i.indkey) with ordinality as key(attnum, ordinality)
    where i.indrelid = c.oid
      and i.indisprimary
) pk on true
where c.oid::bigint = any($1::bigint[])
  and c.relkind in ('r', 'p')
  and a.attnum > 0
  and not a.attisdropped
"#;

fn relation_ids(origins: &[Option<ColumnOrigin>]) -> Vec<i64> {
    let mut relation_ids = Vec::new();
    for relation_id in origins.iter().flatten().map(|origin| origin.relation_id) {
        if !relation_ids.contains(&relation_id) {
            relation_ids.push(relation_id);
        }
    }
    relation_ids
}

pub enum RowsDisplay {
    Inline(String),
    Tui(ResultGrid),
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub enum DisplayMode {
    #[default]
    Auto,
    Inline,
    InlineBlank,
    Tui,
}

impl DisplayMode {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Tui,
            Self::Tui => Self::Inline,
            Self::Inline => Self::InlineBlank,
            Self::InlineBlank => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Inline => "inline",
            Self::InlineBlank => "inline-blank",
            Self::Tui => "tui",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InlineTableStyle {
    Modern,
    Blank,
}

#[derive(Debug, Default, Clone)]
pub struct DisplayModeState {
    mode: Arc<Mutex<DisplayMode>>,
}

impl DisplayModeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> DisplayMode {
        *self
            .mode
            .lock()
            .expect("display mode state is not poisoned")
    }

    #[cfg(test)]
    pub(crate) fn set(&self, mode: DisplayMode) {
        *self
            .mode
            .lock()
            .expect("display mode state is not poisoned") = mode;
    }

    pub fn cycle(&self) -> DisplayMode {
        let mut mode = self
            .mode
            .lock()
            .expect("display mode state is not poisoned");
        *mode = mode.next();
        *mode
    }
}

pub fn render_rows(rows: &[PgRow]) -> String {
    render_rows_with_style(rows, InlineTableStyle::Modern)
}

pub fn render_rows_blank(rows: &[PgRow]) -> String {
    render_rows_with_style(rows, InlineTableStyle::Blank)
}

fn render_rows_with_style(rows: &[PgRow], style: InlineTableStyle) -> String {
    if rows.is_empty() {
        return render_row_count(0);
    }

    let table = if rows.len() == 1 {
        render_expanded_table_with_style(
            expanded_display_builder(&rows[0]),
            terminal_table_width(),
            style,
        )
    } else {
        render_table_with_style(rows_builder(rows), terminal_table_width(), style)
    };

    let mut output = String::new();
    if rows.len() > LARGE_RESULT_WARNING_ROWS {
        output.push_str(&format!(
            "warning: rendering {} rows; narrow the statement's result set when possible\n",
            rows.len()
        ));
    }
    output.push_str(&table);
    output.push('\n');
    output.push_str(&render_row_count(rows.len()));
    output
}

pub fn render_grid(grid: &ResultGrid) -> String {
    render_grid_with_style(grid, InlineTableStyle::Modern)
}

pub fn render_grid_blank(grid: &ResultGrid) -> String {
    render_grid_with_style(grid, InlineTableStyle::Blank)
}

fn render_grid_with_style(grid: &ResultGrid, style: InlineTableStyle) -> String {
    if grid.row_count() == 0 {
        return render_row_count(0);
    }

    let table = if grid.row_count() == 1 {
        render_expanded_table_with_style(grid_expanded_builder(grid), terminal_table_width(), style)
    } else {
        render_table_with_style(grid_builder(grid), terminal_table_width(), style)
    };

    let mut output = String::new();
    if grid.row_count() > LARGE_RESULT_WARNING_ROWS {
        output.push_str(&format!(
            "warning: rendering {} rows; add a filter for large metadata results\n",
            grid.row_count()
        ));
    }
    output.push_str(&table);
    output.push('\n');
    output.push_str(&render_row_count(grid.row_count()));
    output
}

pub fn rows_display(rows: &[PgRow]) -> RowsDisplay {
    if rows.is_empty() {
        return RowsDisplay::Inline(render_row_count(0));
    }

    let grid = ResultGrid::from_rows(rows);
    let dimensions = terminal_dimensions();
    let interactive = io::stdout().is_terminal();

    if should_use_tui(&grid, None, dimensions, interactive) {
        return RowsDisplay::Tui(grid);
    }

    let inline = render_rows(rows);
    if should_use_tui(&grid, Some(&inline), dimensions, interactive) {
        RowsDisplay::Tui(grid)
    } else {
        RowsDisplay::Inline(inline)
    }
}

pub fn grid_display(grid: ResultGrid) -> RowsDisplay {
    if grid.row_count() == 0 {
        return RowsDisplay::Inline(render_row_count(0));
    }

    let dimensions = terminal_dimensions();
    let interactive = io::stdout().is_terminal();

    if should_use_tui(&grid, None, dimensions, interactive) {
        return RowsDisplay::Tui(grid);
    }

    let inline = render_grid(&grid);
    if should_use_tui(&grid, Some(&inline), dimensions, interactive) {
        RowsDisplay::Tui(grid)
    } else {
        RowsDisplay::Inline(inline)
    }
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

fn grid_builder(grid: &ResultGrid) -> Builder {
    let mut builder = Builder::new();
    builder.push_record(grid.columns().iter().cloned());

    for row in grid.rows() {
        builder.push_record(row.iter().map(|cell| cell.display().to_owned()));
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

fn grid_expanded_builder(grid: &ResultGrid) -> Builder {
    expanded_builder(grid.columns().iter().enumerate().map(|(index, column)| {
        (
            column.clone(),
            grid.cell(0, index).unwrap_or_default().to_owned(),
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

#[cfg(test)]
fn render_table(builder: Builder, width: usize) -> String {
    render_table_with_style(builder, width, InlineTableStyle::Modern)
}

fn render_table_with_style(builder: Builder, width: usize, style: InlineTableStyle) -> String {
    let mut table = builder.build();
    match style {
        InlineTableStyle::Modern => table.with(TableStyle::modern_rounded()),
        InlineTableStyle::Blank => table.with(TableStyle::blank()),
    };
    table.with(
        Width::wrap(width)
            .keep_words(true)
            .priority(Priority::max(false)),
    );
    table.with(Modify::new(Segment::all()).with(Format::content(limit_cell_height)));
    table.with(Modify::new(Segment::all()).with(BorderColor::filled(dim_border_color())));
    table.with(Modify::new(Rows::first()).with(TableColor::FG_YELLOW | TableColor::BOLD));
    table.to_string()
}

#[cfg(test)]
fn render_expanded_table(builder: Builder, width: usize) -> String {
    render_expanded_table_with_style(builder, width, InlineTableStyle::Modern)
}

fn render_expanded_table_with_style(
    builder: Builder,
    width: usize,
    style: InlineTableStyle,
) -> String {
    let mut table = builder.build();
    match style {
        InlineTableStyle::Modern => table.with(TableStyle::modern_rounded().remove_horizontals()),
        InlineTableStyle::Blank => table.with(TableStyle::blank()),
    };
    table.with(
        Width::wrap(width)
            .keep_words(true)
            .priority(Priority::max(false)),
    );
    table.with(Modify::new(Segment::all()).with(BorderColor::filled(dim_border_color())));
    table.with(Modify::new(Columns::first()).with(TableColor::FG_YELLOW | TableColor::BOLD));
    table.to_string()
}

fn dim_border_color() -> TableColor {
    TableColor::FG_BRIGHT_BLACK | TableColor::new("\u{1b}[2m", "\u{1b}[22m")
}

fn render_cell(row: &PgRow, index: usize, type_name: &str) -> String {
    render_cell_value(row, index, type_name)
        .display()
        .to_owned()
}

pub(crate) fn render_cell_value(row: &PgRow, index: usize, type_name: &str) -> CellValue {
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
            .unwrap_or_else(|| CellValue::Text(format!("<{unsupported}>"))),
    }
}

fn render_expanded_cell(row: &PgRow, index: usize, type_name: &str) -> String {
    match type_name.to_ascii_lowercase().as_str() {
        "json" | "jsonb" => try_decode(row, index, format_json_value)
            .unwrap_or_else(|| {
                CellValue::Text(format!("<{}>", row.columns()[index].type_info().name()))
            })
            .display()
            .to_owned(),
        _ => render_cell(row, index, type_name),
    }
}

pub(crate) fn format_json_value(value: serde_json::Value) -> String {
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

fn decode<T, F>(row: &PgRow, index: usize, render: F) -> CellValue
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    F: FnOnce(T) -> String,
{
    try_decode(row, index, render).unwrap_or_else(|| {
        CellValue::Text(format!("<{}>", row.columns()[index].type_info().name()))
    })
}

fn try_decode<T, F>(row: &PgRow, index: usize, render: F) -> Option<CellValue>
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    F: FnOnce(T) -> String,
{
    match row.try_get::<Option<T>, _>(index) {
        Ok(Some(value)) => Some(CellValue::Text(render(value))),
        Ok(None) => Some(CellValue::Null),
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

pub fn render_row_count(count: usize) -> String {
    AnsiStyle::new()
        .dimmed()
        .paint(format!("({count} {})", pluralize(count, "row")))
        .to_string()
}

pub fn render_rows_affected(count: u64) -> String {
    let row_word = if count == 1 { "row" } else { "rows" };

    AnsiStyle::new()
        .dimmed()
        .paint(format!("({count} {row_word} affected)"))
        .to_string()
}

fn terminal_table_width() -> usize {
    terminal_dimensions().table_width()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TerminalDimensions {
    width: usize,
    height: usize,
}

impl TerminalDimensions {
    fn table_width(self) -> usize {
        self.width.saturating_sub(1).max(MIN_TABLE_WIDTH)
    }
}

fn terminal_dimensions() -> TerminalDimensions {
    dimensions_from_terminal_size(terminal::size())
}

fn dimensions_from_terminal_size(size: io::Result<(u16, u16)>) -> TerminalDimensions {
    size.map(|(width, height)| TerminalDimensions {
        width: usize::from(width),
        height: usize::from(height),
    })
    .unwrap_or(TerminalDimensions {
        width: DEFAULT_TERMINAL_WIDTH,
        height: DEFAULT_TERMINAL_HEIGHT,
    })
}

fn should_use_tui(
    grid: &ResultGrid,
    inline: Option<&str>,
    dimensions: TerminalDimensions,
    interactive: bool,
) -> bool {
    interactive
        && (columns_too_narrow(grid, dimensions.width)
            || rows_too_tall(grid, dimensions.height)
            || inline.is_some_and(|output| output.lines().count() > dimensions.height))
}

fn columns_too_narrow(grid: &ResultGrid, width: usize) -> bool {
    let column_count = grid.column_count();
    column_count > 1 && width / column_count < MIN_INLINE_COLUMN_WIDTH
}

fn rows_too_tall(grid: &ResultGrid, height: usize) -> bool {
    let body_rows = if grid.row_count() == 1 {
        grid.column_count()
    } else {
        grid.row_count()
    };

    body_rows.saturating_add(INLINE_TABLE_OVERHEAD_ROWS) > height
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
    fn display_mode_cycles_auto_tui_inline_inline_blank() {
        // Given
        let mode = DisplayMode::Auto;

        // When
        let tui = mode.next();
        let inline = tui.next();
        let inline_blank = inline.next();
        let auto = inline_blank.next();

        // Then
        assert_eq!(tui, DisplayMode::Tui);
        assert_eq!(inline, DisplayMode::Inline);
        assert_eq!(inline_blank, DisplayMode::InlineBlank);
        assert_eq!(auto, DisplayMode::Auto);
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
        let formatted = render_row_count(count);

        // Then
        assert_eq!(formatted, "\u{1b}[2m(2 rows)\u{1b}[0m");
    }

    #[test]
    fn rows_affected_is_dimmed() {
        // Given
        let count = 2;

        // When
        let formatted = render_rows_affected(count);

        // Then
        assert_eq!(formatted, "\u{1b}[2m(2 rows affected)\u{1b}[0m");
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
    fn normal_display_colors_header() {
        // Given
        let mut builder = Builder::new();
        builder.push_record(["id", "email"]);
        builder.push_record(["1", "user@example.com"]);

        // When
        let table = render_table(builder, 80);

        // Then
        assert!(table.contains("\u{1b}[33m"));
        assert!(table.contains("\u{1b}[1m"));
        assert!(table.contains("id"));
        assert!(table.contains("email"));
    }

    #[test]
    fn normal_display_dims_borders() {
        // Given
        let mut builder = Builder::new();
        builder.push_record(["id", "email"]);
        builder.push_record(["1", "user@example.com"]);

        // When
        let table = render_table(builder, 80);

        // Then
        assert!(table.contains("\u{1b}[90m"));
        assert!(table.contains("\u{1b}[2m"));
        assert!(table.contains("\u{1b}[22m"));
    }

    #[test]
    fn blank_display_omits_table_borders() {
        // Given
        let mut builder = Builder::new();
        builder.push_record(["id", "email"]);
        builder.push_record(["1", "user@example.com"]);

        // When
        let table = render_table_with_style(builder, 80, InlineTableStyle::Blank);

        // Then
        assert!(table.contains("id"));
        assert!(table.contains("email"));
        assert!(!table.contains('\u{2500}'));
        assert!(!table.contains('\u{2502}'));
    }

    #[test]
    fn expanded_display_colors_first_column() {
        // Given
        let fields = vec![
            ("id".to_owned(), "1".to_owned()),
            ("email".to_owned(), "user@example.com".to_owned()),
        ];

        // When
        let table = render_expanded_table(expanded_builder(fields), 80);

        // Then
        assert!(table.contains("\u{1b}[33m"));
        assert!(table.contains("\u{1b}[1m"));
        assert!(table.contains("id"));
        assert!(table.contains("email"));
        assert!(!table.contains("\u{1b}[33m\u{1b}[1muser@example.com"));
    }

    #[test]
    fn expanded_display_dims_borders() {
        // Given
        let fields = vec![("id".to_owned(), "1".to_owned())];

        // When
        let table = render_expanded_table(expanded_builder(fields), 80);

        // Then
        assert!(table.contains("\u{1b}[90m"));
        assert!(table.contains("\u{1b}[2m"));
        assert!(table.contains("\u{1b}[22m"));
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
        let width = dimensions_from_terminal_size(size).table_width();

        // Then
        assert_eq!(width, 79);
    }

    #[test]
    fn table_width_uses_fallback_when_terminal_size_is_unavailable() {
        // Given
        let size = Err(std::io::Error::other("no tty"));

        // When
        let dimensions = dimensions_from_terminal_size(size);

        // Then
        assert_eq!(dimensions.width, DEFAULT_TERMINAL_WIDTH);
        assert_eq!(dimensions.height, DEFAULT_TERMINAL_HEIGHT);
        assert_eq!(dimensions.table_width(), DEFAULT_TERMINAL_WIDTH - 1);
    }

    #[test]
    fn table_width_keeps_a_minimum_for_tiny_terminals() {
        // Given
        let size = Ok((4, 24));

        // When
        let width = dimensions_from_terminal_size(size).table_width();

        // Then
        assert_eq!(width, MIN_TABLE_WIDTH);
    }

    #[test]
    fn display_mode_uses_inline_for_non_interactive_stdout() {
        // Given
        let grid = test_grid(10, 100);
        let dimensions = TerminalDimensions {
            width: 80,
            height: 24,
        };

        // When
        let use_tui = should_use_tui(&grid, None, dimensions, false);

        // Then
        assert!(!use_tui);
    }

    #[test]
    fn display_mode_uses_tui_when_columns_are_too_narrow() {
        // Given
        let grid = test_grid(8, 2);
        let dimensions = TerminalDimensions {
            width: 80,
            height: 24,
        };

        // When
        let use_tui = should_use_tui(&grid, None, dimensions, true);

        // Then
        assert!(use_tui);
    }

    #[test]
    fn display_mode_uses_tui_when_rows_exceed_terminal_height() {
        // Given
        let grid = test_grid(2, 30);
        let dimensions = TerminalDimensions {
            width: 80,
            height: 24,
        };

        // When
        let use_tui = should_use_tui(&grid, None, dimensions, true);

        // Then
        assert!(use_tui);
    }

    #[test]
    fn display_mode_uses_tui_when_wrapped_output_exceeds_terminal_height() {
        // Given
        let grid = test_grid(2, 2);
        let dimensions = TerminalDimensions {
            width: 80,
            height: 4,
        };
        let inline = "header\nrow1\nrow2\nrow3\nrow4";

        // When
        let use_tui = should_use_tui(&grid, Some(inline), dimensions, true);

        // Then
        assert!(use_tui);
    }

    #[test]
    fn live_text_document_select_star_has_edit_metadata_when_configured() {
        // Given
        let Ok(database_url) = std::env::var("DBCRAB_LIVE_DATABASE_URL") else {
            return;
        };
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
        let (mut grid, pool) = runtime.block_on(async {
            let pool = PgPool::connect(&database_url)
                .await
                .expect("live database should connect");
            let rows = sqlx::query(AssertSqlSafe(
                "select * from text_document limit 1".to_owned(),
            ))
            .fetch_all(&pool)
            .await
            .expect("live query should return rows");
            (ResultGrid::from_rows(&rows), pool)
        });

        // When
        runtime.block_on(async {
            grid.load_update_metadata(&pool)
                .await
                .expect("live metadata should load");
        });

        // Then
        let content_column = grid
            .columns()
            .iter()
            .position(|column| column == "content")
            .expect("content column should be returned");
        assert!(grid.is_column_editable(content_column));
    }

    #[test]
    fn display_mode_keeps_small_results_inline() {
        // Given
        let grid = test_grid(2, 2);
        let dimensions = TerminalDimensions {
            width: 80,
            height: 24,
        };
        let inline = "header\nrow1\nrow2";

        // When
        let use_tui = should_use_tui(&grid, Some(inline), dimensions, true);

        // Then
        assert!(!use_tui);
    }

    fn test_grid(column_count: usize, row_count: usize) -> ResultGrid {
        ResultGrid::new(
            (1..=column_count)
                .map(|index| format!("column{index}"))
                .collect(),
            vec!["text".to_owned(); column_count],
            (1..=row_count)
                .map(|row| {
                    (1..=column_count)
                        .map(|column| format!("r{row}c{column}"))
                        .collect()
                })
                .collect(),
        )
    }
}
