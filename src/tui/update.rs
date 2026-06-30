use std::collections::HashMap;

use sqlx::{AssertSqlSafe, Column, PgPool, Row, TypeInfo};

use crate::{
    catalog::quote_identifier,
    render::{CellValue, ResultGrid, render_cell_value},
};

use super::state::human_position;

pub(super) struct UpdateResult {
    pub(super) message: String,
    pub(super) updated: bool,
}

impl UpdateResult {
    fn unchanged(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            updated: false,
        }
    }

    fn updated(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            updated: true,
        }
    }
}

pub(super) async fn update_selected_row(
    grid: &mut ResultGrid,
    selected_row: usize,
    staged: &mut HashMap<(usize, usize), CellValue>,
    pool: Option<&PgPool>,
) -> UpdateResult {
    let mut changes = staged
        .iter()
        .filter(|((row, _), _)| *row == selected_row)
        .map(|((_, column), value)| (*column, value.clone()))
        .collect::<Vec<_>>();
    changes.sort_by_key(|(column, _)| *column);

    if changes.is_empty() {
        return UpdateResult::unchanged("no staged changes for selected row");
    }

    let Some(pool) = pool else {
        return UpdateResult::unchanged("update failed");
    };

    let statement = match build_update_statement(grid, selected_row, &changes) {
        Ok(statement) => statement,
        Err(message) => return UpdateResult::unchanged(format!("update failed: {message}")),
    };

    let mut query = sqlx::query(AssertSqlSafe(statement.sql));
    for value in statement.binds {
        query = query.bind(value);
    }

    match query.fetch_optional(pool).await {
        Ok(Some(row)) => {
            for (returned_index, column) in statement.returning_columns.iter().enumerate() {
                let type_name = row.columns()[returned_index].type_info().name();
                let value = render_cell_value(&row, returned_index, type_name);
                grid.set_cell_value(selected_row, *column, value);
                staged.remove(&(selected_row, *column));
            }
            UpdateResult::updated(format!(
                "updated row {}",
                human_position(selected_row, grid.row_count())
            ))
        }
        Ok(None) => UpdateResult::unchanged("update failed: row not found"),
        Err(err) => UpdateResult::unchanged(format!("update failed: {}", concise_sqlx_error(&err))),
    }
}

pub(super) struct UpdateStatement {
    pub(super) sql: String,
    pub(super) binds: Vec<String>,
    pub(super) returning_columns: Vec<usize>,
}

pub(super) fn build_update_statement(
    grid: &ResultGrid,
    row: usize,
    changes: &[(usize, CellValue)],
) -> Result<UpdateStatement, String> {
    let first_info = changes
        .iter()
        .find_map(|(column, _)| grid.column_update_info(*column))
        .ok_or_else(|| "selected row is not editable".to_owned())?;
    let relation_id = first_info.relation_id;

    if changes.iter().any(|(column, _)| {
        grid.column_update_info(*column)
            .is_none_or(|info| info.relation_id != relation_id || info.primary_key)
    }) {
        return Err("selected row is not editable".to_owned());
    }

    let mut changed_attributes = Vec::new();
    for (column, _) in changes {
        let info = grid
            .column_update_info(*column)
            .ok_or_else(|| "selected row is not editable".to_owned())?;
        if changed_attributes.contains(&info.attribute_no) {
            return Err("selected row is not editable".to_owned());
        }
        changed_attributes.push(info.attribute_no);
    }

    if changes.iter().any(|(column, value)| {
        matches!(value, CellValue::Null)
            && grid
                .column_update_info(*column)
                .is_some_and(|info| !info.nullable)
    }) {
        return Err("column is not nullable".to_owned());
    }

    let primary_key_columns = grid
        .primary_key_columns_for_relation(relation_id)
        .ok_or_else(|| "selected row is not editable".to_owned())?;
    let mut binds = Vec::new();
    let mut next_bind = 1;

    let set_clauses = changes
        .iter()
        .map(|(column, value)| {
            let info = grid
                .column_update_info(*column)
                .ok_or_else(|| "selected row is not editable".to_owned())?;
            Ok(match value {
                CellValue::Null => format!(
                    "{} = NULL::{}",
                    quote_identifier(&info.column_name),
                    info.data_type
                ),
                CellValue::Text(value) => {
                    let clause = format!(
                        "{} = ${next_bind}::text::{}",
                        quote_identifier(&info.column_name),
                        info.data_type
                    );
                    next_bind += 1;
                    binds.push(value.clone());
                    clause
                }
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let where_clauses = primary_key_columns
        .iter()
        .map(|column| {
            let info = grid
                .column_update_info(*column)
                .ok_or_else(|| "selected row is not editable".to_owned())?;
            let value = grid
                .cell_value(row, *column)
                .and_then(CellValue::as_text)
                .ok_or_else(|| "primary key value is missing".to_owned())?;
            let clause = format!(
                "{} = ${next_bind}::text::{}",
                quote_identifier(&info.column_name),
                info.data_type
            );
            next_bind += 1;
            binds.push(value.to_owned());
            Ok(clause)
        })
        .collect::<Result<Vec<_>, String>>()?;

    let returning_columns = changes
        .iter()
        .map(|(column, _)| *column)
        .collect::<Vec<_>>();
    let returning = returning_columns
        .iter()
        .map(|column| {
            grid.column_update_info(*column)
                .map(|info| quote_identifier(&info.column_name))
                .ok_or_else(|| "selected row is not editable".to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;

    let sql = format!(
        "update {}.{} set {} where {} returning {}",
        quote_identifier(&first_info.relation_schema),
        quote_identifier(&first_info.relation_name),
        set_clauses.join(", "),
        where_clauses.join(" and "),
        returning.join(", ")
    );

    Ok(UpdateStatement {
        sql,
        binds,
        returning_columns,
    })
}

fn concise_sqlx_error(err: &sqlx::Error) -> String {
    err.as_database_error()
        .map_or_else(|| err.to_string(), |err| err.message().to_owned())
}
