use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::Instant,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::TryStreamExt;
use kdl::{KdlDocument, KdlNode};
use sqlx::{AssertSqlSafe, Either, PgPool};

use crate::{
    config::NamedSqlConfig,
    errors::{AppError, AppResult},
    paths,
    render::ResultGrid,
    sql,
};

const PROJECT_CONFIG_FILE: &str = "dbcrab.kdl";
const SHARED_SCOPE: &str = "shared";

#[derive(Debug, Clone)]
pub struct NamedSqlContext {
    name: Option<String>,
    context_source: ContextSource,
    local_root: PathBuf,
    local_root_source: NamedSqlRootSource,
    shared_root: PathBuf,
    shared_root_source: NamedSqlRootSource,
    project_config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ContextSource {
    CommandLine,
    ProjectConfig(PathBuf),
    Default,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum NamedSqlRootSource {
    ProjectConfig(PathBuf),
    UserConfig,
    DefaultDataDirectory,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NamedSqlScope {
    Local,
    Shared,
}

#[derive(Debug, Clone)]
pub struct NamedSql {
    pub name: String,
    pub scope: NamedSqlScope,
    pub path: PathBuf,
    pub sql: String,
    pub analysis: SqlAnalysis,
}

#[derive(Debug, Clone)]
pub struct NamedSqlEntry {
    pub name: String,
    pub parameters: Vec<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SqlAnalysis {
    pub statements: Vec<String>,
    pub parameters: Vec<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NamedValue {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreparedNamedSql {
    pub statements: Vec<BoundStatement>,
}

#[derive(Debug, Clone)]
pub struct BoundStatement {
    pub source: String,
    pub sql: String,
    pub values: Vec<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct NamedStatementOutput {
    pub statement: String,
    pub elapsed_ms: u128,
    pub result: NamedStatementResult,
}

#[derive(Debug, Clone)]
pub enum NamedStatementResult {
    Rows(ResultGrid),
    RowsAffected(u64),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NamedExecutionCompletion {
    Committed,
    ReadOnly,
}

#[derive(Debug)]
pub struct NamedExecutionError {
    pub error: AppError,
    pub statement: Option<String>,
    pub rolled_back: bool,
}

#[derive(Debug)]
struct ProjectConfig {
    path: PathBuf,
    context: Option<String>,
    named_sql_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ParameterOccurrence {
    name: String,
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct SqlScan {
    statement_ranges: Vec<(usize, usize)>,
    parameters: Vec<ParameterOccurrence>,
    diagnostics: Vec<String>,
}

#[derive(Debug)]
enum ScanState {
    Normal,
    SingleQuoted { backslash_escapes: bool },
    DoubleQuoted,
    LineComment,
    BlockComment { depth: usize },
    DollarQuoted { tag: String },
}

impl NamedSqlContext {
    pub fn load(cli_context: Option<&str>, config: &NamedSqlConfig) -> AppResult<Self> {
        let cwd = env::current_dir()?;
        Self::load_from(&cwd, cli_context, config)
    }

    fn load_from(
        cwd: &Path,
        cli_context: Option<&str>,
        config: &NamedSqlConfig,
    ) -> AppResult<Self> {
        Self::load_from_with_optional_base(
            cwd,
            cli_context,
            config,
            paths::default_named_sql_base_path(),
        )
    }

    #[cfg(test)]
    pub(crate) fn load_from_with_base(
        cwd: &Path,
        cli_context: Option<&str>,
        config: &NamedSqlConfig,
        base: PathBuf,
    ) -> AppResult<Self> {
        Self::load_from_with_optional_base(cwd, cli_context, config, Some(base))
    }

    fn load_from_with_optional_base(
        cwd: &Path,
        cli_context: Option<&str>,
        config: &NamedSqlConfig,
        base: Option<PathBuf>,
    ) -> AppResult<Self> {
        let project = discover_project_config(cwd)?
            .map(|path| load_project_config(&path))
            .transpose()?;
        let project_context = project
            .as_ref()
            .and_then(|project| project.context.as_deref());

        if let Some(context) = cli_context {
            validate_context_name(context)?;
        }

        let name = cli_context.or(project_context).map(str::to_owned);
        let context_source = match cli_context {
            Some(_) => ContextSource::CommandLine,
            None => project
                .as_ref()
                .filter(|project| project.context.is_some())
                .map_or(ContextSource::Default, |project| {
                    ContextSource::ProjectConfig(project.path.clone())
                }),
        };
        let shared_root_source = if config.shared_path.is_some() {
            NamedSqlRootSource::UserConfig
        } else {
            NamedSqlRootSource::DefaultDataDirectory
        };
        let shared_root = config
            .shared_path
            .clone()
            .or_else(|| base.as_ref().map(|base| base.join(SHARED_SCOPE)))
            .ok_or_else(missing_data_directory)?;
        let (local_root, local_root_source) = match name.as_deref() {
            None => (shared_root.clone(), shared_root_source.clone()),
            Some(context) => project
                .as_ref()
                .filter(|project| project.context.as_deref() == Some(context))
                .and_then(|project| {
                    project.named_sql_path.clone().map(|root| {
                        (
                            root,
                            NamedSqlRootSource::ProjectConfig(project.path.clone()),
                        )
                    })
                })
                .or_else(|| {
                    base.as_ref()
                        .map(|base| (base.join(context), NamedSqlRootSource::DefaultDataDirectory))
                })
                .ok_or_else(missing_data_directory)?,
        };

        if name.is_some() && paths_overlap(&local_root, &shared_root)? {
            return Err(AppError::message(format!(
                "named SQL context path `{}` overlaps shared path `{}`",
                local_root.display(),
                shared_root.display()
            )));
        }

        Ok(Self {
            name,
            context_source,
            local_root,
            local_root_source,
            shared_root,
            shared_root_source,
            project_config_path: project.map(|project| project.path),
        })
    }

    pub fn history_context(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(SHARED_SCOPE)
    }

    pub fn context_source_label(&self) -> String {
        match &self.context_source {
            ContextSource::CommandLine => "--context".to_owned(),
            ContextSource::ProjectConfig(path) => path.display().to_string(),
            ContextSource::Default => "default".to_owned(),
        }
    }

    pub fn project_config_label(&self) -> String {
        self.project_config_path
            .as_ref()
            .map_or_else(|| "none".to_owned(), |path| path.display().to_string())
    }

    pub fn history_path(&self) -> Option<PathBuf> {
        paths::default_history_path(self.history_context())
    }

    pub fn root_source_label(&self, scope: NamedSqlScope) -> String {
        let source = match scope {
            NamedSqlScope::Local => &self.local_root_source,
            NamedSqlScope::Shared => &self.shared_root_source,
        };
        match source {
            NamedSqlRootSource::ProjectConfig(path) => path.display().to_string(),
            NamedSqlRootSource::UserConfig => "user config".to_owned(),
            NamedSqlRootSource::DefaultDataDirectory => "default data directory".to_owned(),
        }
    }

    pub fn scope_label(&self, scope: NamedSqlScope) -> &str {
        if self.name.is_none() || scope == NamedSqlScope::Shared {
            SHARED_SCOPE
        } else {
            self.display_name()
        }
    }

    pub fn root(&self, scope: NamedSqlScope) -> &Path {
        match scope {
            NamedSqlScope::Local => &self.local_root,
            NamedSqlScope::Shared => &self.shared_root,
        }
    }

    pub fn read(&self, name: &str) -> AppResult<NamedSql> {
        let (scope, logical_name) = self.resolve_name(name)?;
        let path = self.path_for(scope, logical_name);
        reject_directory_symlinks(self.root(scope), path.parent())?;
        let sql = fs::read_to_string(&path).map_err(|err| {
            AppError::message(format!(
                "failed to read named SQL `{name}` from `{}`: {err}",
                path.display()
            ))
        })?;

        Ok(NamedSql {
            name: self.qualified_name(scope, logical_name),
            scope,
            path,
            analysis: analyze_sql(&sql),
            sql,
        })
    }

    pub fn save(&self, name: &str, sql: &str) -> AppResult<NamedSql> {
        let (scope, logical_name, path) = self.prepare_save_target(name)?;
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            fs::write(&path, sql).map_err(|err| {
                AppError::message(format!(
                    "failed to write through named SQL symlink `{}`: {err}",
                    path.display()
                ))
            })?;
        } else {
            atomic_write(&path, sql.as_bytes())?;
        }

        Ok(NamedSql {
            name: self.qualified_name(scope, logical_name),
            scope,
            path,
            sql: sql.to_owned(),
            analysis: analyze_sql(sql),
        })
    }

    pub(crate) fn prepare_save_path(&self, name: &str) -> AppResult<PathBuf> {
        self.prepare_save_target(name).map(|(_, _, path)| path)
    }

    fn prepare_save_target<'a>(
        &self,
        name: &'a str,
    ) -> AppResult<(NamedSqlScope, &'a str, PathBuf)> {
        let (scope, logical_name) = self.resolve_name(name)?;
        let path = self.path_for(scope, logical_name);
        let parent = path.parent().ok_or_else(|| {
            AppError::message(format!("named SQL path `{}` has no parent", path.display()))
        })?;
        reject_directory_symlinks(self.root(scope), Some(parent))?;
        fs::create_dir_all(parent).map_err(|err| {
            AppError::message(format!(
                "failed to create named SQL directory `{}`: {err}",
                parent.display()
            ))
        })?;
        Ok((scope, logical_name, path))
    }

    pub fn delete(&self, name: &str) -> AppResult<PathBuf> {
        let (scope, logical_name) = self.resolve_name(name)?;
        let path = self.path_for(scope, logical_name);
        reject_directory_symlinks(self.root(scope), path.parent())?;
        fs::remove_file(&path).map_err(|err| {
            AppError::message(format!(
                "failed to delete named SQL `{name}` from `{}`: {err}",
                path.display()
            ))
        })?;
        prune_empty_parents(path.parent(), self.root(scope));
        Ok(path)
    }

    pub fn list(&self, shared: bool, all: bool) -> AppResult<Vec<NamedSqlEntry>> {
        let mut entries = Vec::new();
        if !shared {
            entries.extend(self.list_scope(NamedSqlScope::Local)?);
        }
        if shared || (all && self.name.is_some()) {
            entries.extend(self.list_scope(NamedSqlScope::Shared)?);
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    pub fn completion_names(&self) -> AppResult<Vec<String>> {
        let mut names = self.names_in_scope(NamedSqlScope::Local)?;
        if self.name.is_some() {
            names.extend(self.names_in_scope(NamedSqlScope::Shared)?);
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn prepare(&self, name: &str, values: &[NamedValue]) -> AppResult<PreparedNamedSql> {
        let named_sql = self.read(name)?;
        if !named_sql.analysis.diagnostics.is_empty() {
            return Err(AppError::message(format!(
                "named SQL `{}` is invalid:\n{}",
                named_sql.name,
                named_sql.analysis.diagnostics.join("\n")
            )));
        }

        let mut supplied = HashMap::new();
        for value in values {
            validate_parameter_name(&value.name)?;
            if supplied.insert(value.name.as_str(), &value.value).is_some() {
                return Err(AppError::message(format!(
                    "duplicate named SQL parameter `{}`",
                    value.name
                )));
            }
        }
        let expected = named_sql
            .analysis
            .parameters
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let missing = named_sql
            .analysis
            .parameters
            .iter()
            .filter(|name| !supplied.contains_key(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let mut unexpected = supplied
            .keys()
            .filter(|name| !expected.contains(**name))
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        unexpected.sort();
        if !missing.is_empty() || !unexpected.is_empty() {
            let mut errors = Vec::new();
            if !missing.is_empty() {
                errors.push(format!("missing parameters: {}", missing.join(", ")));
            }
            if !unexpected.is_empty() {
                errors.push(format!("unexpected parameters: {}", unexpected.join(", ")));
            }
            return Err(AppError::message(errors.join("; ")));
        }

        let statements = named_sql
            .analysis
            .statements
            .iter()
            .map(|statement| rewrite_statement(statement, &supplied))
            .collect::<AppResult<Vec<_>>>()?;
        Ok(PreparedNamedSql { statements })
    }

    fn list_scope(&self, scope: NamedSqlScope) -> AppResult<Vec<NamedSqlEntry>> {
        let root = self.root(scope);
        if !root.exists() {
            return Ok(Vec::new());
        }
        reject_directory_symlinks(root, Some(root))?;
        if !root.is_dir() {
            return Err(AppError::message(format!(
                "named SQL root `{}` is not a directory",
                root.display()
            )));
        }

        let mut files = Vec::new();
        collect_sql_files(root, root, &mut files)?;
        Ok(files
            .into_iter()
            .map(|(path, relative)| {
                let logical = logical_name_from_path(&relative);
                let (display_name, mut diagnostics) = match logical {
                    Ok(name) => (self.qualified_name(scope, &name), Vec::new()),
                    Err(err) => (relative.to_string_lossy().into_owned(), vec![err]),
                };
                let analysis = fs::read_to_string(&path).map_or_else(
                    |err| SqlAnalysis {
                        statements: Vec::new(),
                        parameters: Vec::new(),
                        diagnostics: vec![format!("cannot read file: {err}")],
                    },
                    |sql| analyze_sql(&sql),
                );
                diagnostics.extend(analysis.diagnostics);
                NamedSqlEntry {
                    name: display_name,
                    parameters: analysis.parameters,
                    diagnostics,
                }
            })
            .collect())
    }

    fn names_in_scope(&self, scope: NamedSqlScope) -> AppResult<Vec<String>> {
        let root = self.root(scope);
        if !root.exists() {
            return Ok(Vec::new());
        }
        reject_directory_symlinks(root, Some(root))?;
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        collect_sql_files(root, root, &mut files)?;
        Ok(files
            .into_iter()
            .filter_map(|(_, relative)| logical_name_from_path(&relative).ok())
            .map(|name| self.qualified_name(scope, &name))
            .collect())
    }

    fn resolve_name<'a>(&self, name: &'a str) -> AppResult<(NamedSqlScope, &'a str)> {
        if let Some(name) = name.strip_prefix("shared/") {
            validate_logical_name(name)?;
            return Ok((NamedSqlScope::Shared, name));
        }
        validate_logical_name(name)?;
        Ok((NamedSqlScope::Local, name))
    }

    fn path_for(&self, scope: NamedSqlScope, name: &str) -> PathBuf {
        let mut path = self.root(scope).to_owned();
        let mut segments = name.split('/').peekable();
        while let Some(segment) = segments.next() {
            if segments.peek().is_some() {
                path.push(segment);
            } else {
                path.push(format!("{segment}.sql"));
            }
        }
        path
    }

    fn qualified_name(&self, scope: NamedSqlScope, name: &str) -> String {
        if scope == NamedSqlScope::Shared && self.name.is_some() {
            format!("shared/{name}")
        } else {
            name.to_owned()
        }
    }
}

fn missing_data_directory() -> AppError {
    AppError::message(
        "cannot determine the named SQL data directory; set XDG_DATA_HOME, HOME, or explicit named SQL paths",
    )
}

pub async fn execute_prepared(
    pool: &PgPool,
    prepared: &PreparedNamedSql,
    read_only: bool,
    statement_timeout: Option<&str>,
    mut on_output: impl FnMut(NamedStatementOutput),
) -> Result<NamedExecutionCompletion, NamedExecutionError> {
    if read_only
        && let Some(statement) = prepared
            .statements
            .iter()
            .find(|statement| !sql::is_read_only_statement(&statement.source))
    {
        return Err(NamedExecutionError {
            error: AppError::message(
                "read-only agent mode rejected a mutating named SQL statement; pass --allow-write to run it",
            ),
            statement: Some(statement.source.clone()),
            rolled_back: false,
        });
    }

    let mut tx = pool.begin().await.map_err(|error| NamedExecutionError {
        error: error.into(),
        statement: None,
        rolled_back: false,
    })?;
    if read_only
        && let Err(error) = sqlx::query("set transaction read only")
            .execute(&mut *tx)
            .await
    {
        let rolled_back = tx.rollback().await.is_ok();
        return Err(NamedExecutionError {
            error: error.into(),
            statement: None,
            rolled_back,
        });
    }
    if let Some(timeout) = statement_timeout
        && let Err(error) =
            sqlx::query("select pg_catalog.set_config('statement_timeout', $1, true)")
                .bind(timeout)
                .execute(&mut *tx)
                .await
    {
        let rolled_back = tx.rollback().await.is_ok();
        return Err(NamedExecutionError {
            error: error.into(),
            statement: None,
            rolled_back,
        });
    }

    for statement in &prepared.statements {
        let started = Instant::now();
        let mut query = sqlx::query(AssertSqlSafe(statement.sql.clone()));
        for value in &statement.values {
            query = query.bind(value.as_deref());
        }
        let result = async {
            let mut rows = Vec::new();
            let mut rows_affected = 0;
            #[allow(deprecated)]
            let mut results = query.fetch_many(&mut *tx);
            while let Some(result) = results.try_next().await? {
                match result {
                    Either::Left(result) => rows_affected += result.rows_affected(),
                    Either::Right(row) => rows.push(row),
                }
            }
            Ok::<_, sqlx::Error>(
                if !rows.is_empty()
                    || (rows_affected == 0 && sql::likely_returns_rows(&statement.source))
                {
                    NamedStatementResult::Rows(ResultGrid::from_rows(&rows))
                } else {
                    NamedStatementResult::RowsAffected(rows_affected)
                },
            )
        }
        .await;

        match result {
            Ok(result) => on_output(NamedStatementOutput {
                statement: statement.source.clone(),
                elapsed_ms: started.elapsed().as_millis(),
                result,
            }),
            Err(error) => {
                let rolled_back = tx.rollback().await.is_ok();
                return Err(NamedExecutionError {
                    error: error.into(),
                    statement: Some(statement.sql.clone()),
                    rolled_back,
                });
            }
        }
    }

    if read_only {
        tx.rollback().await.map_err(|error| NamedExecutionError {
            error: error.into(),
            statement: None,
            rolled_back: false,
        })?;
        Ok(NamedExecutionCompletion::ReadOnly)
    } else {
        tx.commit().await.map_err(|error| NamedExecutionError {
            error: error.into(),
            statement: None,
            rolled_back: false,
        })?;
        Ok(NamedExecutionCompletion::Committed)
    }
}

pub fn parse_named_value(raw: &str, cooked: &str) -> AppResult<NamedValue> {
    let (name, value) = cooked.split_once('=').ok_or_else(|| {
        AppError::message(format!(
            "named SQL parameter `{cooked}` must use name=value syntax"
        ))
    })?;
    validate_parameter_name(name)?;

    let raw_value = raw.split_once('=').map_or("", |(_, value)| value);
    let quoted = raw_value.starts_with(['\'', '"'])
        || (raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']));
    Ok(NamedValue {
        name: name.to_owned(),
        value: if !quoted && value == "null" {
            None
        } else {
            Some(value.to_owned())
        },
    })
}

pub fn analyze_sql(input: &str) -> SqlAnalysis {
    let scan = scan_sql(input);
    let mut diagnostics = scan.diagnostics;
    let statements = scan
        .statement_ranges
        .iter()
        .filter_map(|(start, end)| {
            let statement = input[*start..*end].trim();
            (!statement.is_empty() && sql::first_keyword(statement).is_some())
                .then(|| statement.to_owned())
        })
        .collect::<Vec<_>>();
    if statements.is_empty() {
        diagnostics.push("named SQL contains no executable statements".to_owned());
    }
    for (index, statement) in statements.iter().enumerate() {
        if is_transaction_boundary(statement) {
            diagnostics.push(format!(
                "statement {} contains transaction control; named SQL transactions are managed by DBCrab",
                index + 1
            ));
        }
    }

    let mut seen = HashSet::new();
    let parameters = scan
        .parameters
        .into_iter()
        .filter_map(|parameter| {
            seen.insert(parameter.name.clone())
                .then_some(parameter.name)
        })
        .collect();
    SqlAnalysis {
        statements,
        parameters,
        diagnostics,
    }
}

fn rewrite_statement(
    statement: &str,
    supplied: &HashMap<&str, &Option<String>>,
) -> AppResult<BoundStatement> {
    let scan = scan_sql(statement);
    if !scan.diagnostics.is_empty() {
        return Err(AppError::message(scan.diagnostics.join("\n")));
    }

    let mut indexes = HashMap::<&str, usize>::new();
    let mut values = Vec::new();
    let mut rewritten = String::with_capacity(statement.len());
    let mut cursor = 0;
    for parameter in &scan.parameters {
        rewritten.push_str(&statement[cursor..parameter.start]);
        let index = if let Some(index) = indexes.get(parameter.name.as_str()) {
            *index
        } else {
            let index = indexes.len() + 1;
            indexes.insert(parameter.name.as_str(), index);
            values.push(
                supplied
                    .get(parameter.name.as_str())
                    .map(|value| (**value).clone())
                    .ok_or_else(|| {
                        AppError::message(format!("missing parameter `{}`", parameter.name))
                    })?,
            );
            index
        };
        rewritten.push('$');
        rewritten.push_str(&index.to_string());
        cursor = parameter.end;
    }
    rewritten.push_str(&statement[cursor..]);

    Ok(BoundStatement {
        source: statement.to_owned(),
        sql: rewritten,
        values,
    })
}

fn scan_sql(input: &str) -> SqlScan {
    let mut statement_ranges = Vec::new();
    let mut parameters = Vec::new();
    let mut diagnostics = Vec::new();
    let mut state = ScanState::Normal;
    let mut start = 0;
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match &mut state {
            ScanState::Normal => match ch {
                ';' => {
                    statement_ranges.push((start, idx + 1));
                    start = idx + 1;
                }
                '\'' => {
                    state = ScanState::SingleQuoted {
                        backslash_escapes: is_escape_string_prefix(input, idx),
                    };
                }
                '"' => state = ScanState::DoubleQuoted,
                '-' if peek_char(&mut chars) == Some('-') => {
                    chars.next();
                    state = ScanState::LineComment;
                }
                '/' if peek_char(&mut chars) == Some('*') => {
                    chars.next();
                    state = ScanState::BlockComment { depth: 1 };
                }
                '$' => {
                    if dollar_token_boundary(input, idx)
                        && peek_char(&mut chars).is_some_and(|next| next.is_ascii_digit())
                    {
                        while chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()) {
                            chars.next();
                        }
                        diagnostics.push(format!(
                            "native positional parameter at byte {} is not supported; use a named `:parameter`",
                            idx + 1
                        ));
                    } else if dollar_token_boundary(input, idx)
                        && let Some(tag) = dollar_quote_tag(input, idx)
                    {
                        for _ in 0..tag.chars().count().saturating_sub(1) {
                            chars.next();
                        }
                        state = ScanState::DollarQuoted { tag };
                    }
                }
                ':' => match peek_char(&mut chars) {
                    Some(':') | Some('=') => {
                        chars.next();
                    }
                    Some(next) if next.is_ascii_alphabetic() || next == '_' => {
                        let mut end = idx + 1;
                        let mut name = String::new();
                        let mut valid = next.is_ascii_lowercase() || next == '_';
                        while let Some((parameter_idx, parameter_ch)) = chars.peek().copied() {
                            if parameter_ch.is_ascii_alphanumeric() || parameter_ch == '_' {
                                chars.next();
                                name.push(parameter_ch);
                                valid &= !parameter_ch.is_ascii_uppercase();
                                end = parameter_idx + parameter_ch.len_utf8();
                            } else {
                                break;
                            }
                        }
                        if valid {
                            parameters.push(ParameterOccurrence {
                                name,
                                start: idx,
                                end,
                            });
                        } else {
                            diagnostics.push(format!(
                                "parameter at byte {} must use a lowercase name",
                                idx + 1
                            ));
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            ScanState::SingleQuoted { backslash_escapes } => match ch {
                '\\' if *backslash_escapes => {
                    chars.next();
                }
                '\'' if peek_char(&mut chars) == Some('\'') => {
                    chars.next();
                }
                '\'' => state = ScanState::Normal,
                _ => {}
            },
            ScanState::DoubleQuoted => {
                if ch == '"' {
                    if peek_char(&mut chars) == Some('"') {
                        chars.next();
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::LineComment => {
                if ch == '\n' {
                    state = ScanState::Normal;
                }
            }
            ScanState::BlockComment { depth } => match ch {
                '/' if peek_char(&mut chars) == Some('*') => {
                    chars.next();
                    *depth += 1;
                }
                '*' if peek_char(&mut chars) == Some('/') => {
                    chars.next();
                    *depth -= 1;
                    if *depth == 0 {
                        state = ScanState::Normal;
                    }
                }
                _ => {}
            },
            ScanState::DollarQuoted { tag } => {
                if input[idx..].starts_with(tag.as_str()) {
                    for _ in 0..tag.chars().count().saturating_sub(1) {
                        chars.next();
                    }
                    state = ScanState::Normal;
                }
            }
        }
    }

    if start < input.len() {
        statement_ranges.push((start, input.len()));
    }
    match state {
        ScanState::Normal | ScanState::LineComment => {}
        ScanState::SingleQuoted { .. } => {
            diagnostics.push("unterminated single-quoted string".to_owned());
        }
        ScanState::DoubleQuoted => diagnostics.push("unterminated quoted identifier".to_owned()),
        ScanState::BlockComment { .. } => diagnostics.push("unterminated block comment".to_owned()),
        ScanState::DollarQuoted { tag } => {
            diagnostics.push(format!("unterminated dollar quote `{tag}`"));
        }
    }

    SqlScan {
        statement_ranges,
        parameters,
        diagnostics,
    }
}

fn is_transaction_boundary(statement: &str) -> bool {
    let keywords = leading_keywords(statement, 2);
    match keywords.as_slice() {
        [first, ..]
            if matches!(
                first.as_str(),
                "abort" | "begin" | "commit" | "end" | "release" | "rollback" | "savepoint"
            ) =>
        {
            true
        }
        [first, second]
            if matches!(first.as_str(), "prepare" | "set" | "start") && second == "transaction" =>
        {
            true
        }
        _ => false,
    }
}

fn leading_keywords(input: &str, limit: usize) -> Vec<String> {
    let mut keywords = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '-' && peek_char(&mut chars) == Some('-') {
            chars.next();
            for (_, comment_ch) in chars.by_ref() {
                if comment_ch == '\n' {
                    break;
                }
            }
            continue;
        }
        if ch == '/' && peek_char(&mut chars) == Some('*') {
            chars.next();
            let mut depth = 1;
            while let Some((_, comment_ch)) = chars.next() {
                match comment_ch {
                    '/' if peek_char(&mut chars) == Some('*') => {
                        chars.next();
                        depth += 1;
                    }
                    '*' if peek_char(&mut chars) == Some('/') => {
                        chars.next();
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        if ch.is_ascii_alphabetic() {
            let mut word = String::from(ch.to_ascii_lowercase());
            while let Some((_, next)) = chars.peek().copied() {
                if !next.is_ascii_alphabetic() {
                    break;
                }
                chars.next();
                word.push(next.to_ascii_lowercase());
            }
            keywords.push(word);
            if keywords.len() == limit {
                break;
            }
            continue;
        }
        break;
    }
    keywords
}

fn validate_context_name(name: &str) -> AppResult<()> {
    validate_segment(name, "context")?;
    if name == SHARED_SCOPE {
        return Err(AppError::message(
            "`shared` is reserved; omit --context to use the shared context",
        ));
    }
    Ok(())
}

fn validate_logical_name(name: &str) -> AppResult<()> {
    if name.is_empty() {
        return Err(AppError::message("named SQL name cannot be empty"));
    }
    if name.ends_with(".sql") {
        return Err(AppError::message(
            "named SQL names omit the `.sql` extension",
        ));
    }
    for (index, segment) in name.split('/').enumerate() {
        validate_segment(segment, "named SQL path")?;
        if index == 0 && segment == SHARED_SCOPE {
            return Err(AppError::message(
                "`shared` is reserved as the shared named SQL prefix",
            ));
        }
    }
    Ok(())
}

fn validate_segment(segment: &str, label: &str) -> AppResult<()> {
    if segment.is_empty() || matches!(segment, "." | "..") {
        return Err(AppError::message(format!(
            "invalid {label} segment `{segment}`"
        )));
    }
    if segment.starts_with('-') {
        return Err(AppError::message(format!(
            "{label} segment `{segment}` cannot start with `-`"
        )));
    }
    if !segment
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(AppError::message(format!(
            "{label} `{segment}` must use lowercase ASCII letters, digits, `.`, `_`, or `-`"
        )));
    }
    Ok(())
}

fn validate_parameter_name(name: &str) -> AppResult<()> {
    let mut chars = name.chars();
    if !chars
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
        || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(AppError::message(format!(
            "parameter name `{name}` must start with a lowercase letter or `_` and contain only lowercase letters, digits, or `_`"
        )));
    }
    Ok(())
}

fn discover_project_config(cwd: &Path) -> AppResult<Option<PathBuf>> {
    let local = cwd.join(PROJECT_CONFIG_FILE);
    if local.is_file() {
        return Ok(Some(local));
    }

    let git_root = cwd
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists());
    Ok(git_root
        .map(|root| root.join(PROJECT_CONFIG_FILE))
        .filter(|path| path.is_file()))
}

fn load_project_config(path: &Path) -> AppResult<ProjectConfig> {
    let text = fs::read_to_string(path).map_err(|err| {
        AppError::message(format!(
            "failed to read project config `{}`: {err}",
            path.display()
        ))
    })?;
    let document = text.parse::<KdlDocument>().map_err(|err| {
        AppError::message(format!(
            "invalid project config `{}`: KDL parse error: {err}",
            path.display()
        ))
    })?;
    let mut context = None;
    let mut named_sql_path = None;
    for node in document.nodes() {
        match node.name().value() {
            "context" => {
                if context.is_some() {
                    return Err(project_node_error(path, node, "duplicate `context` node"));
                }
                let value = project_string_value(path, node, "context")?;
                validate_context_name(value)?;
                context = Some(value.to_owned());
            }
            "named-sql-path" => {
                if named_sql_path.is_some() {
                    return Err(project_node_error(
                        path,
                        node,
                        "duplicate `named-sql-path` node",
                    ));
                }
                let value = project_string_value(path, node, "named-sql-path")?;
                let value = paths::expand_home(Path::new(value));
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                named_sql_path = Some(if value.is_absolute() {
                    value
                } else {
                    parent.join(value)
                });
            }
            name => {
                return Err(project_node_error(
                    path,
                    node,
                    format!("unknown project setting `{name}`"),
                ));
            }
        }
    }
    if named_sql_path.is_some() && context.is_none() {
        return Err(AppError::message(format!(
            "project config `{}` requires `context` when `named-sql-path` is set",
            path.display()
        )));
    }
    Ok(ProjectConfig {
        path: path.to_owned(),
        context,
        named_sql_path,
    })
}

fn project_string_value<'a>(path: &Path, node: &'a KdlNode, name: &str) -> AppResult<&'a str> {
    if node.ty().is_some() || node.children().is_some() || node.entries().len() != 1 {
        return Err(project_node_error(
            path,
            node,
            format!("`{name}` requires exactly one string value"),
        ));
    }
    let entry = &node.entries()[0];
    if entry.name().is_some() || entry.ty().is_some() {
        return Err(project_node_error(
            path,
            node,
            format!("`{name}` requires exactly one string value"),
        ));
    }
    entry
        .value()
        .as_string()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            project_node_error(
                path,
                node,
                format!("`{name}` requires exactly one non-empty string value"),
            )
        })
}

fn project_node_error(path: &Path, node: &KdlNode, message: impl std::fmt::Display) -> AppError {
    AppError::message(format!(
        "invalid project config `{}` at byte {}: {message}",
        path.display(),
        node.span().offset()
    ))
}

fn collect_sql_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> AppResult<()> {
    let entries = fs::read_dir(directory).map_err(|err| {
        AppError::message(format!(
            "failed to list named SQL directory `{}`: {err}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_sql_files(root, &path, files)?;
        } else if (file_type.is_file() || file_type.is_symlink())
            && path.extension().is_some_and(|extension| extension == "sql")
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| AppError::message(err.to_string()))?
                .to_owned();
            files.push((path, relative));
        }
    }
    Ok(())
}

fn reject_directory_symlinks(root: &Path, directory: Option<&Path>) -> AppResult<()> {
    let Some(directory) = directory else {
        return Ok(());
    };
    let relative = directory.strip_prefix(root).map_err(|_| {
        AppError::message(format!(
            "named SQL path `{}` escapes root `{}`",
            directory.display(),
            root.display()
        ))
    })?;
    let mut current = root.to_owned();
    for component in std::iter::once(Component::CurDir).chain(relative.components()) {
        if component != Component::CurDir {
            current.push(component.as_os_str());
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::message(format!(
                    "named SQL directory `{}` is a symlink; only file symlinks are supported",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn logical_name_from_path(path: &Path) -> Result<String, String> {
    let mut path = path.to_owned();
    path.set_extension("");
    let segments = path
        .components()
        .map(|component| match component {
            Component::Normal(segment) => segment
                .to_str()
                .ok_or_else(|| "path is not valid UTF-8".to_owned()),
            _ => Err("path contains an invalid component".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let name = segments.join("/");
    validate_logical_name(&name).map_err(|err| err.to_string())?;
    Ok(name)
}

fn atomic_write(path: &Path, contents: &[u8]) -> AppResult<()> {
    if cfg!(windows) && path.exists() {
        return fs::write(path, contents).map_err(|err| {
            AppError::message(format!(
                "failed to overwrite named SQL `{}`: {err}",
                path.display()
            ))
        });
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("named-sql");
    let temporary =
        path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), stamp));
    let result = (|| -> AppResult<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|err| {
        AppError::message(format!(
            "failed to save named SQL `{}`: {err}",
            path.display()
        ))
    })
}

fn prune_empty_parents(mut directory: Option<&Path>, root: &Path) {
    while let Some(path) = directory {
        if path == root || fs::remove_dir(path).is_err() {
            break;
        }
        directory = path.parent();
    }
}

fn paths_overlap(left: &Path, right: &Path) -> AppResult<bool> {
    let left = canonical_or_absolute(left)?;
    let right = canonical_or_absolute(right)?;
    Ok(left.starts_with(&right) || right.starts_with(&left))
}

fn canonical_or_absolute(path: &Path) -> AppResult<PathBuf> {
    let absolute = if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&env::current_dir()?.join(path))
    };
    if absolute.exists() {
        return fs::canonicalize(absolute).map_err(AppError::from);
    }

    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            AppError::message(format!(
                "cannot resolve named SQL path `{}`",
                absolute.display()
            ))
        })?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| {
            AppError::message(format!(
                "cannot resolve named SQL path `{}`",
                absolute.display()
            ))
        })?;
    }
    let mut resolved = fs::canonicalize(existing)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn peek_char(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<char> {
    chars.peek().map(|(_, ch)| *ch)
}

fn is_escape_string_prefix(input: &str, quote_start: usize) -> bool {
    let mut preceding = input[..quote_start].chars().rev();
    preceding.next().is_some_and(|ch| matches!(ch, 'e' | 'E'))
        && preceding
            .next()
            .is_none_or(|ch| !is_identifier_continuation(ch))
}

fn dollar_token_boundary(input: &str, start: usize) -> bool {
    input[..start]
        .chars()
        .next_back()
        .is_none_or(|ch| !is_identifier_continuation(ch))
}

fn is_identifier_continuation(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '$')
}

fn dollar_quote_tag(input: &str, start: usize) -> Option<String> {
    let after_dollar = &input[start + 1..];
    let end = after_dollar.find('$')?;
    let tag = &after_dollar[..end];
    if tag.is_empty()
        || (tag
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
            && tag.chars().all(|ch| ch.is_alphanumeric() || ch == '_'))
    {
        Some(format!("${tag}$"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let unique = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("dbcrab-{name}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_context(root: &Path, name: Option<&str>) -> NamedSqlContext {
        NamedSqlContext {
            name: name.map(str::to_owned),
            context_source: name.map_or(ContextSource::Default, |_| ContextSource::CommandLine),
            local_root: name.map_or_else(|| root.join("shared"), |name| root.join(name)),
            local_root_source: NamedSqlRootSource::DefaultDataDirectory,
            shared_root: root.join("shared"),
            shared_root_source: NamedSqlRootSource::DefaultDataDirectory,
            project_config_path: None,
        }
    }

    #[test]
    fn analysis_finds_named_parameters_only_in_sql_code() {
        // Given
        let sql = "select :id::uuid, ':ignored', \"also:ignored\"; -- :comment\nselect :id, :name; select $$:body$$";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.parameters, ["id", "name"]);
        assert_eq!(analysis.statements.len(), 3);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn analysis_reports_uppercase_parameter_names() {
        // Given
        let sql = "select :UserId";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.diagnostics.len(), 1);
        assert!(analysis.diagnostics[0].contains("lowercase"));
    }

    #[test]
    fn analysis_rejects_uppercase_parameter_suffixes_without_partial_rewrite() {
        // Given
        let sql = "select :userId";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert!(analysis.parameters.is_empty());
        assert!(analysis.diagnostics[0].contains("lowercase"));
    }

    #[test]
    fn analysis_rejects_native_positional_parameters() {
        // Given
        let sql = "select $1::integer, :value::integer";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.parameters, ["value"]);
        assert!(analysis.diagnostics[0].contains("positional parameter"));
    }

    #[test]
    fn ordinary_string_backslash_does_not_escape_closing_quote() {
        // Given
        let sql = "select 'a\\'; select :id";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.statements.len(), 2);
        assert_eq!(analysis.parameters, ["id"]);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn escape_string_backslash_escapes_quote() {
        // Given
        let sql = "select E'a\\'; still one string'; select :id";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.statements.len(), 2);
        assert_eq!(analysis.parameters, ["id"]);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn dollar_quote_requires_identifier_boundary() {
        // Given
        let sql = "select foo$tag$bar, :id";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.parameters, ["id"]);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn unicode_dollar_quote_hides_parameter_markers() {
        // Given
        let sql = "select $täg$:ignored$täg$, :id";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.parameters, ["id"]);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn analysis_reports_transaction_boundaries() {
        // Given
        let sql = "select 1; commit";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert_eq!(analysis.statements.len(), 2);
        assert!(analysis.diagnostics[0].contains("transaction control"));
    }

    #[test]
    fn analysis_allows_regular_prepare_statements() {
        // Given
        let sql = "prepare q as select 'transaction'";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn analysis_reports_savepoint_control() {
        // Given
        let sql = "savepoint before_update";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert!(analysis.diagnostics[0].contains("transaction control"));
    }

    #[test]
    fn analysis_reports_unterminated_sql_constructs() {
        // Given
        let sql = "select 'unfinished";

        // When
        let analysis = analyze_sql(sql);

        // Then
        assert!(
            analysis
                .diagnostics
                .contains(&"unterminated single-quoted string".to_owned())
        );
    }

    #[test]
    fn prepare_rewrites_each_statements_parameters_independently() {
        // Given
        let directory = TestDirectory::new("prepare");
        let context = test_context(directory.path(), Some("app"));
        fs::create_dir_all(context.root(NamedSqlScope::Local))
            .expect("context root should be created");
        fs::write(
            context.root(NamedSqlScope::Local).join("report.sql"),
            "select :id::uuid, :name; select :name",
        )
        .expect("named SQL fixture should be written");
        let values = [
            NamedValue {
                name: "name".to_owned(),
                value: Some("Ada".to_owned()),
            },
            NamedValue {
                name: "id".to_owned(),
                value: Some("123".to_owned()),
            },
        ];

        // When
        let prepared = context
            .prepare("report", &values)
            .expect("named SQL should prepare");

        // Then
        assert_eq!(prepared.statements[0].sql, "select $1::uuid, $2;");
        assert_eq!(
            prepared.statements[0].values,
            [Some("123".to_owned()), Some("Ada".to_owned())]
        );
        assert_eq!(prepared.statements[1].sql, "select $1");
        assert_eq!(prepared.statements[1].values, [Some("Ada".to_owned())]);
    }

    #[test]
    fn prepare_rejects_unexpected_parameters() {
        // Given
        let directory = TestDirectory::new("unexpected-parameter");
        let context = test_context(directory.path(), None);
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::write(
            context.root(NamedSqlScope::Shared).join("health.sql"),
            "select 1",
        )
        .expect("named SQL fixture should be written");
        let values = [NamedValue {
            name: "extra".to_owned(),
            value: Some("value".to_owned()),
        }];

        // When
        let result = context.prepare("health", &values);

        // Then
        assert_eq!(
            result
                .expect_err("unexpected parameter should fail")
                .to_string(),
            "unexpected parameters: extra"
        );
    }

    #[test]
    fn save_creates_nested_sql_file() {
        // Given
        let directory = TestDirectory::new("nested-save");
        let context = test_context(directory.path(), Some("app"));

        // When
        let saved = context
            .save("reports/monthly", "select 1")
            .expect("named SQL should save");

        // Then
        assert_eq!(saved.path, directory.path().join("app/reports/monthly.sql"));
        assert_eq!(
            fs::read_to_string(saved.path).expect("saved SQL should be readable"),
            "select 1"
        );
    }

    #[test]
    fn prepare_save_path_creates_parent_without_creating_file() {
        // Given
        let directory = TestDirectory::new("prepare-save-path");
        let context = test_context(directory.path(), Some("app"));

        // When
        let path = context
            .prepare_save_path("reports/monthly")
            .expect("save path should prepare");

        // Then
        assert_eq!(path, directory.path().join("app/reports/monthly.sql"));
        assert!(path.parent().is_some_and(Path::is_dir));
        assert!(!path.exists());
    }

    #[test]
    fn delete_prunes_empty_nested_directories() {
        // Given
        let directory = TestDirectory::new("nested-delete");
        let context = test_context(directory.path(), Some("app"));
        let nested = context
            .root(NamedSqlScope::Local)
            .join("reports/monthly.sql");
        fs::create_dir_all(nested.parent().expect("fixture path should have a parent"))
            .expect("nested directory should be created");
        fs::write(&nested, "select 1").expect("named SQL fixture should be written");

        // When
        context
            .delete("reports/monthly")
            .expect("named SQL should delete");

        // Then
        assert!(!nested.exists());
        assert!(!context.root(NamedSqlScope::Local).join("reports").exists());
        assert!(context.root(NamedSqlScope::Local).exists());
    }

    #[test]
    fn list_reports_invalid_file_names() {
        // Given
        let directory = TestDirectory::new("invalid-list-entry");
        let context = test_context(directory.path(), None);
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::write(
            context.root(NamedSqlScope::Shared).join("Valid.sql"),
            "select 1",
        )
        .expect("invalid fixture should be written");

        // When
        let entries = context.list(false, false).expect("named SQL should list");

        // Then
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Valid.sql");
        assert!(entries[0].diagnostics[0].contains("lowercase"));
    }

    #[cfg(unix)]
    #[test]
    fn save_writes_through_file_symlink() {
        use std::os::unix::fs::symlink;

        // Given
        let directory = TestDirectory::new("symlink-save");
        let context = test_context(directory.path(), None);
        let target = directory.path().join("target.sql");
        let link = context.root(NamedSqlScope::Shared).join("linked.sql");
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::write(&target, "select 1").expect("target should be written");
        symlink(&target, &link).expect("file symlink should be created");

        // When
        context
            .save("linked", "select 2")
            .expect("symlink should save");

        // Then
        assert_eq!(
            fs::read_to_string(target).expect("target should be readable"),
            "select 2"
        );
        assert!(
            link.symlink_metadata()
                .expect("link should exist")
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_unlinks_without_deleting_symlink_target() {
        use std::os::unix::fs::symlink;

        // Given
        let directory = TestDirectory::new("symlink-delete");
        let context = test_context(directory.path(), None);
        let target = directory.path().join("target.sql");
        let link = context.root(NamedSqlScope::Shared).join("linked.sql");
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::write(&target, "select 1").expect("target should be written");
        symlink(&target, &link).expect("file symlink should be created");

        // When
        context.delete("linked").expect("symlink should delete");

        // Then
        assert!(!link.exists());
        assert_eq!(
            fs::read_to_string(target).expect("target should remain"),
            "select 1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_rejects_symlinked_directories_below_scope_root() {
        use std::os::unix::fs::symlink;

        // Given
        let directory = TestDirectory::new("directory-symlink");
        let context = test_context(directory.path(), None);
        let external = directory.path().join("external");
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::create_dir_all(&external).expect("external directory should be created");
        fs::write(external.join("query.sql"), "select 1").expect("external SQL should be written");
        symlink(
            &external,
            context.root(NamedSqlScope::Shared).join("linked"),
        )
        .expect("directory symlink should be created");

        // When
        let result = context.read("linked/query");

        // Then
        assert!(
            result
                .expect_err("directory symlink should be rejected")
                .to_string()
                .contains("only file symlinks")
        );
    }

    #[test]
    fn named_context_requires_explicit_shared_prefix() {
        // Given
        let directory = TestDirectory::new("shared-prefix");
        let context = test_context(directory.path(), Some("app"));
        fs::create_dir_all(context.root(NamedSqlScope::Local))
            .expect("context root should be created");
        fs::create_dir_all(context.root(NamedSqlScope::Shared))
            .expect("shared root should be created");
        fs::write(
            context.root(NamedSqlScope::Local).join("check.sql"),
            "select 'local'",
        )
        .expect("local fixture should be written");
        fs::write(
            context.root(NamedSqlScope::Shared).join("check.sql"),
            "select 'shared'",
        )
        .expect("shared fixture should be written");

        // When
        let local = context.read("check").expect("local SQL should resolve");
        let shared = context
            .read("shared/check")
            .expect("shared SQL should resolve");

        // Then
        assert_eq!(local.sql, "select 'local'");
        assert_eq!(shared.sql, "select 'shared'");
    }

    #[test]
    fn unquoted_null_is_parsed_as_sql_null() {
        // Given
        let raw = "value=null";

        // When
        let value = parse_named_value(raw, raw).expect("parameter should parse");

        // Then
        assert_eq!(value.value, None);
    }

    #[test]
    fn quoted_null_is_parsed_as_text() {
        // Given
        let raw = "value=\"null\"";

        // When
        let value = parse_named_value(raw, "value=null").expect("parameter should parse");

        // Then
        assert_eq!(value.value, Some("null".to_owned()));
    }

    #[test]
    fn current_directory_project_config_wins_over_git_root() {
        // Given
        let directory = TestDirectory::new("project-precedence");
        let git_root = directory.path().join("repo");
        let cwd = git_root.join("services/api");
        fs::create_dir_all(git_root.join(".git")).expect("git marker should be created");
        fs::create_dir_all(&cwd).expect("working directory should be created");
        fs::write(git_root.join(PROJECT_CONFIG_FILE), "context \"root\"")
            .expect("root config should be written");
        fs::write(
            cwd.join(PROJECT_CONFIG_FILE),
            "context \"api\"\nnamed-sql-path \"./queries\"",
        )
        .expect("current config should be written");

        // When
        let context = NamedSqlContext::load_from_with_base(
            &cwd,
            None,
            &NamedSqlConfig::default(),
            directory.path().join("data"),
        )
        .expect("context should load");

        // Then
        assert_eq!(context.display_name(), "api");
        assert_eq!(context.local_root, cwd.join("queries"));
        assert_eq!(
            context.context_source_label(),
            cwd.join(PROJECT_CONFIG_FILE).display().to_string()
        );
    }

    #[test]
    fn git_root_project_config_is_used_as_fallback() {
        // Given
        let directory = TestDirectory::new("git-root-config");
        let git_root = directory.path().join("repo");
        let cwd = git_root.join("services/api");
        fs::create_dir_all(git_root.join(".git")).expect("git marker should be created");
        fs::create_dir_all(&cwd).expect("working directory should be created");
        fs::write(
            git_root.join(PROJECT_CONFIG_FILE),
            "context \"root\"\nnamed-sql-path \"./queries\"",
        )
        .expect("root config should be written");

        // When
        let context = NamedSqlContext::load_from_with_base(
            &cwd,
            None,
            &NamedSqlConfig::default(),
            directory.path().join("data"),
        )
        .expect("context should load");

        // Then
        assert_eq!(context.display_name(), "root");
        assert_eq!(context.local_root, git_root.join("queries"));
    }

    #[test]
    fn cli_context_override_ignores_different_project_path() {
        // Given
        let directory = TestDirectory::new("cli-context");
        fs::write(
            directory.path().join(PROJECT_CONFIG_FILE),
            "context \"project\"\nnamed-sql-path \"./queries\"",
        )
        .expect("project config should be written");
        let base = directory.path().join("data");

        // When
        let context = NamedSqlContext::load_from_with_base(
            directory.path(),
            Some("other"),
            &NamedSqlConfig::default(),
            base.clone(),
        )
        .expect("context should load");

        // Then
        assert_eq!(context.display_name(), "other");
        assert_eq!(context.local_root, base.join("other"));
    }

    #[test]
    fn matching_cli_context_uses_project_path() {
        // Given
        let directory = TestDirectory::new("matching-cli-context");
        fs::write(
            directory.path().join(PROJECT_CONFIG_FILE),
            "context \"billing\"\nnamed-sql-path \"./queries\"",
        )
        .expect("project config should be written");

        // When
        let context = NamedSqlContext::load_from_with_base(
            directory.path(),
            Some("billing"),
            &NamedSqlConfig::default(),
            directory.path().join("data"),
        )
        .expect("context should load");

        // Then
        assert_eq!(context.local_root, directory.path().join("queries"));
        assert_eq!(context.context_source_label(), "--context");
        assert_eq!(
            context.project_config_label(),
            directory
                .path()
                .join(PROJECT_CONFIG_FILE)
                .display()
                .to_string()
        );
        assert_eq!(
            context.root_source_label(NamedSqlScope::Local),
            directory
                .path()
                .join(PROJECT_CONFIG_FILE)
                .display()
                .to_string()
        );
    }

    #[test]
    fn same_cli_context_outside_project_uses_default_path() {
        // Given
        let directory = TestDirectory::new("same-context-other-project");
        let first_project = directory.path().join("first");
        let other_project = directory.path().join("other");
        fs::create_dir_all(&first_project).expect("first project should be created");
        fs::create_dir_all(&other_project).expect("other project should be created");
        fs::write(
            first_project.join(PROJECT_CONFIG_FILE),
            "context \"billing\"\nnamed-sql-path \"./queries\"",
        )
        .expect("first project config should be written");
        let base = directory.path().join("data");

        // When
        let context = NamedSqlContext::load_from_with_base(
            &other_project,
            Some("billing"),
            &NamedSqlConfig::default(),
            base.clone(),
        )
        .expect("context should load");

        // Then
        assert_eq!(context.local_root, base.join("billing"));
        assert_eq!(
            context.root_source_label(NamedSqlScope::Local),
            "default data directory"
        );
    }

    #[test]
    fn context_rejects_overlapping_local_and_shared_roots() {
        // Given
        let directory = TestDirectory::new("overlapping-roots");
        fs::write(
            directory.path().join(PROJECT_CONFIG_FILE),
            "context \"app\"\nnamed-sql-path \"./sql\"",
        )
        .expect("project config should be written");
        let config = NamedSqlConfig {
            shared_path: Some(directory.path().join("sql/shared")),
        };

        // When
        let result = NamedSqlContext::load_from_with_base(
            directory.path(),
            None,
            &config,
            directory.path().join("data"),
        );

        // Then
        assert!(
            result
                .expect_err("overlapping roots should fail")
                .to_string()
                .contains("overlaps")
        );
    }

    #[tokio::test]
    async fn read_only_execution_rejects_mutating_statement_before_connecting() {
        // Given
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dbcrab")
            .expect("lazy test pool should be created");
        let prepared = PreparedNamedSql {
            statements: vec![BoundStatement {
                source: "delete from users".to_owned(),
                sql: "delete from users".to_owned(),
                values: Vec::new(),
            }],
        };

        // When
        let result = execute_prepared(&pool, &prepared, true, Some("1s"), |_| {}).await;

        // Then
        let failure = result.expect_err("mutating statement should be rejected");
        assert!(!failure.rolled_back);
        assert!(failure.error.to_string().contains("--allow-write"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_execution_binds_named_values_and_commits_all_statements() {
        // Given
        let pool = live_pool().await;
        let prepared = PreparedNamedSql {
            statements: vec![
                BoundStatement {
                    source: "create temporary table named_values(value uuid)".to_owned(),
                    sql: "create temporary table named_values(value uuid)".to_owned(),
                    values: Vec::new(),
                },
                BoundStatement {
                    source: "insert into named_values values (:id::uuid)".to_owned(),
                    sql: "insert into named_values values ($1::uuid)".to_owned(),
                    values: vec![Some("8b7347d4-7a2d-4be0-86ad-e4cb035afc4c".to_owned())],
                },
                BoundStatement {
                    source: "select value::text from named_values".to_owned(),
                    sql: "select value::text from named_values".to_owned(),
                    values: Vec::new(),
                },
            ],
        };
        let mut outputs = Vec::new();

        // When
        let result = execute_prepared(&pool, &prepared, false, Some("10s"), |output| {
            outputs.push(output)
        })
        .await;

        // Then
        result.expect("named SQL should commit");
        assert_eq!(outputs.len(), 3);
        assert!(matches!(outputs[2].result, NamedStatementResult::Rows(_)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_execution_rolls_back_prior_statements_after_failure() {
        // Given
        let pool = live_pool().await;
        sqlx::query("drop table if exists dbcrab_named_sql_rollback_test")
            .execute(&pool)
            .await
            .expect("old rollback fixture should be removed");
        let prepared = PreparedNamedSql {
            statements: vec![
                BoundStatement {
                    source: "create table dbcrab_named_sql_rollback_test(id integer)".to_owned(),
                    sql: "create table dbcrab_named_sql_rollback_test(id integer)".to_owned(),
                    values: Vec::new(),
                },
                BoundStatement {
                    source: "select * from relation_that_does_not_exist".to_owned(),
                    sql: "select * from relation_that_does_not_exist".to_owned(),
                    values: Vec::new(),
                },
            ],
        };

        // When
        let result = execute_prepared(&pool, &prepared, false, Some("10s"), |_| {}).await;

        // Then
        let failure = result.expect_err("later failure should roll back");
        assert!(failure.rolled_back);
        let exists: bool =
            sqlx::query_scalar("select to_regclass('dbcrab_named_sql_rollback_test') is not null")
                .fetch_one(&pool)
                .await
                .expect("fixture existence should load");
        assert!(!exists);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_read_only_execution_rolls_back_session_settings() {
        // Given
        let pool = live_pool().await;
        let before: String = sqlx::query_scalar("select current_setting('application_name')")
            .fetch_one(&pool)
            .await
            .expect("initial application name should load");
        let prepared = PreparedNamedSql {
            statements: vec![BoundStatement {
                source: "select set_config('application_name', 'dbcrab-named-test', false)"
                    .to_owned(),
                sql: "select set_config('application_name', 'dbcrab-named-test', false)".to_owned(),
                values: Vec::new(),
            }],
        };

        // When
        let completion = execute_prepared(&pool, &prepared, true, Some("10s"), |_| {})
            .await
            .expect("read-only named SQL should complete");

        // Then
        assert_eq!(completion, NamedExecutionCompletion::ReadOnly);
        let after: String = sqlx::query_scalar("select current_setting('application_name')")
            .fetch_one(&pool)
            .await
            .expect("final application name should load");
        assert_eq!(after, before);
    }

    async fn live_pool() -> PgPool {
        let url = env::var("DBCRAB_TEST_DATABASE_URL")
            .expect("DBCRAB_TEST_DATABASE_URL is required for ignored live tests");
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("live test database should connect")
    }
}
