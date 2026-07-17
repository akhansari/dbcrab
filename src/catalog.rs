use std::{
    io::{self, Write},
    sync::{Arc, RwLock},
};

use sqlx::{PgPool, Row};

use crate::errors::AppResult;

pub const METADATA_CONFIRM_RELATION_THRESHOLD: i64 = 1_000;

pub type SharedCatalog = Arc<RwLock<Catalog>>;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    schemas: Vec<String>,
    tables: Vec<TableInfo>,
    columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub data_type: String,
}

impl Catalog {
    pub async fn load(pool: &PgPool) -> AppResult<Self> {
        let relation_count = relation_count(pool).await?;
        let should_load_columns = if should_confirm_metadata(relation_count) {
            confirm_metadata_load(relation_count)?
        } else {
            true
        };

        Self::load_with_columns(pool, should_load_columns).await
    }

    pub async fn load_unattended(pool: &PgPool) -> AppResult<Self> {
        let relation_count = relation_count(pool).await?;
        Self::load_with_columns(pool, !should_confirm_metadata(relation_count)).await
    }

    async fn load_with_columns(pool: &PgPool, should_load_columns: bool) -> AppResult<Self> {
        let schemas = load_schemas(pool).await?;
        let tables = load_tables(pool).await?;
        let columns = if should_load_columns {
            load_columns(pool).await?
        } else {
            Vec::new()
        };

        Ok(Self {
            schemas,
            tables,
            columns,
        })
    }

    #[cfg(test)]
    pub fn from_parts(
        schemas: Vec<String>,
        tables: Vec<TableInfo>,
        columns: Vec<ColumnInfo>,
    ) -> Self {
        Self {
            schemas,
            tables,
            columns,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} schemas, {} relations, {} columns",
            self.schemas.len(),
            self.tables.len(),
            self.columns.len()
        )
    }

    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }

    pub fn tables(&self) -> &[TableInfo] {
        &self.tables
    }

    pub fn columns(&self) -> &[ColumnInfo] {
        &self.columns
    }

    pub fn schema_exists(&self, schema: &str) -> bool {
        self.schemas
            .iter()
            .any(|candidate| identifier_eq(candidate, schema))
    }

    pub fn tables_in_schema(&self, schema: &str) -> Vec<&TableInfo> {
        self.tables
            .iter()
            .filter(|table| identifier_eq(&table.schema, schema))
            .collect()
    }

    pub fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&TableInfo> {
        self.tables.iter().find(|table| {
            identifier_eq(&table.name, name)
                && schema.is_none_or(|schema| identifier_eq(&table.schema, schema))
        })
    }

    pub fn columns_for_table(&self, schema: Option<&str>, table: &str) -> Vec<&ColumnInfo> {
        self.columns
            .iter()
            .filter(|column| {
                identifier_eq(&column.table, table)
                    && schema.is_none_or(|schema| identifier_eq(&column.schema, schema))
            })
            .collect()
    }

    pub fn closest_relation(&self, name: &str) -> Option<String> {
        let relation_names = self.tables.iter().map(|table| table.name.as_str());
        closest_name(name, relation_names)
    }
}

pub fn shared_catalog(catalog: Catalog) -> SharedCatalog {
    Arc::new(RwLock::new(catalog))
}

pub fn should_confirm_metadata(relation_count: i64) -> bool {
    relation_count > METADATA_CONFIRM_RELATION_THRESHOLD
}

pub fn quote_identifier(identifier: &str) -> String {
    if can_use_unquoted_identifier(identifier) {
        identifier.to_owned()
    } else {
        format!("\"{}\"", identifier.replace('"', "\"\""))
    }
}

pub fn normalize_typed_identifier(identifier: &str) -> String {
    let trimmed = identifier.trim();
    if trimmed.starts_with('"') {
        unquote_partial_identifier(trimmed)
    } else {
        trimmed.to_ascii_lowercase()
    }
}

pub fn identifier_matches_prefix(identifier: &str, typed_prefix: &str) -> bool {
    if typed_prefix.starts_with('"') {
        identifier.starts_with(&unquote_partial_identifier(typed_prefix))
    } else {
        identifier
            .to_ascii_lowercase()
            .starts_with(&typed_prefix.to_ascii_lowercase())
    }
}

fn can_use_unquoted_identifier(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        && !crate::sql::is_sql_keyword(identifier)
}

fn unquote_partial_identifier(identifier: &str) -> String {
    let without_start = identifier.strip_prefix('"').unwrap_or(identifier);
    let without_end = without_start.strip_suffix('"').unwrap_or(without_start);
    without_end.replace("\"\"", "\"")
}

fn identifier_eq(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

async fn relation_count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        select count(*)::bigint as count
        from pg_catalog.pg_class c
        join pg_catalog.pg_namespace n on n.oid = c.relnamespace
        where c.relkind in ('r', 'p', 'v', 'm', 'f')
          and n.nspname <> 'pg_catalog'
          and n.nspname <> 'information_schema'
          and n.nspname !~ '^pg_toast'
          and n.nspname !~ '^pg_temp_'
        "#,
    )
    .fetch_one(pool)
    .await?;

    row.try_get("count")
}

async fn load_schemas(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        select nspname as schema
        from pg_catalog.pg_namespace
        where nspname <> 'pg_catalog'
          and nspname <> 'information_schema'
          and nspname !~ '^pg_toast'
          and nspname !~ '^pg_temp_'
        order by nspname
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(|row| row.try_get("schema")).collect()
}

async fn load_tables(pool: &PgPool) -> Result<Vec<TableInfo>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        select n.nspname as schema,
               c.relname as name,
               c.relkind::text as kind
        from pg_catalog.pg_class c
        join pg_catalog.pg_namespace n on n.oid = c.relnamespace
        where c.relkind in ('r', 'p', 'v', 'm', 'f')
          and n.nspname <> 'pg_catalog'
          and n.nspname <> 'information_schema'
          and n.nspname !~ '^pg_toast'
          and n.nspname !~ '^pg_temp_'
        order by n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(TableInfo {
                schema: row.try_get("schema")?,
                name: row.try_get("name")?,
                kind: row.try_get("kind")?,
            })
        })
        .collect()
}

async fn load_columns(pool: &PgPool) -> Result<Vec<ColumnInfo>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        select n.nspname as schema,
               c.relname as table,
               a.attname as name,
               pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type
        from pg_catalog.pg_attribute a
        join pg_catalog.pg_class c on c.oid = a.attrelid
        join pg_catalog.pg_namespace n on n.oid = c.relnamespace
        where c.relkind in ('r', 'p', 'v', 'm', 'f')
          and a.attnum > 0
          and not a.attisdropped
          and n.nspname <> 'pg_catalog'
          and n.nspname <> 'information_schema'
          and n.nspname !~ '^pg_toast'
          and n.nspname !~ '^pg_temp_'
        order by n.nspname, c.relname, a.attnum
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ColumnInfo {
                schema: row.try_get("schema")?,
                table: row.try_get("table")?,
                name: row.try_get("name")?,
                data_type: row.try_get("data_type")?,
            })
        })
        .collect()
}

fn confirm_metadata_load(relation_count: i64) -> io::Result<bool> {
    print!("Database has {relation_count} relations. Load all columns for completion? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub(crate) fn closest_name<'a>(
    target: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<String> {
    let target = target.to_ascii_lowercase();
    let mut best: Option<(&str, usize)> = None;
    let mut tied = false;

    for candidate in candidates {
        let distance = levenshtein(&target, &candidate.to_ascii_lowercase());
        match best {
            Some((best_candidate, best_distance)) if distance == best_distance => {
                tied |= !candidate.eq_ignore_ascii_case(best_candidate);
            }
            Some((_, best_distance)) if distance > best_distance => {}
            _ => {
                best = Some((candidate, distance));
                tied = false;
            }
        }
    }

    let (candidate, distance) = best?;
    let threshold = target.chars().count().div_ceil(3).clamp(1, 3);
    (!tied && distance <= threshold).then(|| candidate.to_owned())
}

fn levenshtein(left: &str, right: &str) -> usize {
    let mut costs: Vec<usize> = (0..=right.chars().count()).collect();

    for (left_index, left_char) in left.chars().enumerate() {
        let mut previous = costs[0];
        costs[0] = left_index + 1;

        for (right_index, right_char) in right.chars().enumerate() {
            let insertion = costs[right_index + 1] + 1;
            let deletion = costs[right_index] + 1;
            let substitution = previous + usize::from(left_char != right_char);
            previous = costs[right_index + 1];
            costs[right_index + 1] = insertion.min(deletion).min(substitution);
        }
    }

    costs[right.chars().count()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_confirm_metadata_above_threshold() {
        // Given
        let relation_count = METADATA_CONFIRM_RELATION_THRESHOLD + 1;

        // When
        let should_confirm = should_confirm_metadata(relation_count);

        // Then
        assert!(should_confirm);
    }

    #[test]
    fn should_not_confirm_metadata_at_threshold() {
        // Given
        let relation_count = METADATA_CONFIRM_RELATION_THRESHOLD;

        // When
        let should_confirm = should_confirm_metadata(relation_count);

        // Then
        assert!(!should_confirm);
    }

    #[test]
    fn quote_identifier_preserves_mixed_case_names() {
        // Given
        let identifier = "User Account";

        // When
        let quoted = quote_identifier(identifier);

        // Then
        assert_eq!(quoted, "\"User Account\"");
    }

    #[test]
    fn quote_identifier_leaves_plain_lowercase_names_unquoted() {
        // Given
        let identifier = "users";

        // When
        let quoted = quote_identifier(identifier);

        // Then
        assert_eq!(quoted, "users");
    }

    #[test]
    fn closest_relation_suggests_near_match() {
        // Given
        let catalog = Catalog::from_parts(
            vec!["public".into()],
            vec![TableInfo {
                schema: "public".into(),
                name: "customers".into(),
                kind: "r".into(),
            }],
            vec![],
        );

        // When
        let suggestion = catalog.closest_relation("customres");

        // Then
        assert_eq!(suggestion.as_deref(), Some("customers"));
    }

    #[test]
    fn closest_relation_rejects_weak_match_for_short_name() {
        // Given
        let catalog = Catalog::from_parts(
            vec!["public".into()],
            vec![TableInfo {
                schema: "public".into(),
                name: "users".into(),
                kind: "r".into(),
            }],
            vec![],
        );

        // When
        let suggestion = catalog.closest_relation("usr");

        // Then
        assert_eq!(suggestion, None);
    }

    #[test]
    fn closest_relation_rejects_equally_close_matches() {
        // Given
        let catalog = Catalog::from_parts(
            vec!["public".into()],
            vec![
                TableInfo {
                    schema: "public".into(),
                    name: "name".into(),
                    kind: "r".into(),
                },
                TableInfo {
                    schema: "public".into(),
                    name: "game".into(),
                    kind: "r".into(),
                },
            ],
            vec![],
        );

        // When
        let suggestion = catalog.closest_relation("fame");

        // Then
        assert_eq!(suggestion, None);
    }
}
