use std::{
    collections::HashSet,
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use clap::{Arg, ArgAction, ArgGroup, Command};
use nu_ansi_term::{Color, Style};
use reedline::{
    Completer, CompletionResult, Highlighter, Span, StyledText, Suggestion, ValidationResult,
    Validator,
};
use sqlx::{PgPool, Row};

use crate::{
    catalog::{Catalog, SharedCatalog, quote_identifier},
    config::ConfigSource,
    errors::{AppError, AppResult},
    named_sql::{
        NamedSql, NamedSqlContext, NamedSqlScope, NamedValue, PreparedNamedSql, parse_named_value,
    },
    render::{CellValue, ResultGrid},
    transfer::{self, ExportOptions, ImportOptions, TransferSummary},
};

const SYSTEM_FLAG: &[&str] = &["-x", "--system"];
const DESCRIBE_FLAGS: &[&str] = &[
    "-f",
    "--function",
    "-s",
    "--schema",
    "-t",
    "--table",
    "-v",
    "--view",
    "-T",
    "--type",
    "-r",
    "--relation",
    "-x",
    "--system",
];
const SOURCE_FLAGS: &[&str] = &["-f", "--function", "-v", "--view", "-x", "--system"];
const IMPORT_FLAGS: &[&str] = &["--name", "--input", "--format", "--no-header"];
const EXPORT_TABLE_FLAGS: &[&str] = &["--name", "--output", "--format", "--no-header", "--force"];
const EXPORT_QUERY_FLAGS: &[&str] = &["--sql", "--output", "--format", "--no-header", "--force"];
const NAMED_LIST_FLAGS: &[&str] = &["--shared", "--all"];
const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "help",
        usage: "help [command]",
        description: "Show command help",
        flags: "-h --help",
        examples: "help\nhelp describe",
    },
    CommandInfo {
        name: "connection",
        usage: "connection",
        description: "Show safe connection details",
        flags: "-h --help",
        examples: "connection",
    },
    CommandInfo {
        name: "session",
        usage: "session",
        description: "Show DBCrab session details",
        flags: "-h --help",
        examples: "session",
    },
    CommandInfo {
        name: "refresh",
        usage: "refresh",
        description: "Refresh autocomplete metadata",
        flags: "-h --help",
        examples: "refresh",
    },
    CommandInfo {
        name: "schemas",
        usage: "schemas [filter] [-x]",
        description: "List schemas",
        flags: "-x --system, -h --help",
        examples: "schemas\nschemas auth",
    },
    CommandInfo {
        name: "databases",
        usage: "databases [filter]",
        description: "List databases",
        flags: "-h --help",
        examples: "databases\ndatabases prod",
    },
    CommandInfo {
        name: "roles",
        usage: "roles [filter]",
        description: "List roles",
        flags: "-h --help",
        examples: "roles\nroles app",
    },
    CommandInfo {
        name: "extensions",
        usage: "extensions [filter] [-x]",
        description: "List installed extensions",
        flags: "-x --system, -h --help",
        examples: "extensions\nextensions postgis",
    },
    CommandInfo {
        name: "tables",
        usage: "tables [filter] [-x]",
        description: "List tables, partitioned tables, and foreign tables",
        flags: "-x --system, -h --help",
        examples: "tables\ntables user\ntables pg_catalog -x",
    },
    CommandInfo {
        name: "views",
        usage: "views [filter] [-x]",
        description: "List views and materialized views",
        flags: "-x --system, -h --help",
        examples: "views\nviews active",
    },
    CommandInfo {
        name: "functions",
        usage: "functions [filter] [-x]",
        description: "List functions and procedures",
        flags: "-x --system, -h --help",
        examples: "functions\nfunctions login",
    },
    CommandInfo {
        name: "types",
        usage: "types [filter] [-x]",
        description: "List PostgreSQL data types",
        flags: "-x --system, -h --help",
        examples: "types\ntypes status",
    },
    CommandInfo {
        name: "privileges",
        usage: "privileges [filter] [-x]",
        description: "List explicit object privileges",
        flags: "-x --system, -h --help",
        examples: "privileges\nprivileges users",
    },
    CommandInfo {
        name: "describe",
        usage: "describe <name> [kind flag] [-x]",
        description: "Inspect a database object",
        flags: "-f --function, -s --schema, -t --table, -v --view, -T --type, -r --relation, -x --system, -h --help",
        examples: "describe users\ndescribe login -f\ndescribe public.users -t",
    },
    CommandInfo {
        name: "source",
        usage: "source <function-or-view> [-f|-v] [-x]",
        description: "Show a function/procedure or view definition",
        flags: "-f --function, -v --view, -x --system, -h --help",
        examples: "source login\nsource auth.login(text, text)\nsource active_users -v",
    },
    CommandInfo {
        name: "import",
        usage: "import table --name <table> --input <path> [options]",
        description: "Import a local CSV file into a table",
        flags: "--name, --input, --format csv, --no-header, -h --help",
        examples: "import table --name users --input ./users.csv\nimport table --name users --input ./users.data --format csv",
    },
    CommandInfo {
        name: "export",
        usage: "export table|query [source] --output <path> [options]",
        description: "Export a relation or read-only query to a local CSV file",
        flags: "--name, --sql, --output, --format csv, --no-header, --force, -h --help",
        examples: "export table --name users --output ./users.csv\nexport query --sql \"select * from users where active\" --output ./active.csv",
    },
    CommandInfo {
        name: "run",
        usage: "run <name> [name=value ...]",
        description: "Run named SQL",
        flags: "-h --help",
        examples: "run users/by-id id=42\nrun shared/health-check",
    },
    CommandInfo {
        name: "named",
        usage: "named run|save|info|list|status|delete ...",
        description: "Manage and run named SQL",
        flags: "list: --shared --all, -h --help",
        examples: "named status\nnamed list\nnamed info users/by-id\nnamed save health\nnamed save health select 1",
    },
    CommandInfo {
        name: "quit",
        usage: "quit",
        description: "Exit DBCrab",
        flags: "-h --help",
        examples: "quit",
    },
];

#[derive(Debug, Clone)]
pub struct MetaOutput {
    pub sections: Vec<MetaSection>,
}

#[derive(Debug, Clone)]
pub struct MetaSection {
    pub title: String,
    pub grid: ResultGrid,
}

#[derive(Debug)]
pub enum CommandOutcome {
    None,
    Exit,
    Output(MetaOutput),
    Run(PreparedNamedSql),
    RunFailed(AppError),
}

pub struct CommandValidator;

impl Validator for CommandValidator {
    fn validate(&self, _line: &str) -> ValidationResult {
        ValidationResult::Complete
    }
}

#[derive(Clone)]
pub struct CommandCompleter {
    catalog: SharedCatalog,
    named_sql: NamedSqlContext,
}

impl CommandCompleter {
    pub fn new(catalog: SharedCatalog, named_sql: NamedSqlContext) -> Self {
        Self { catalog, named_sql }
    }
}

impl Completer for CommandCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        CompletionResult::fresh(command_suggestions_with_named(
            line,
            pos,
            &self.catalog,
            Some(&self.named_sql),
        ))
    }
}

pub struct CommandHighlighter;

impl Highlighter for CommandHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        highlight_command(line)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Schemas,
    Databases,
    Roles,
    Extensions,
    Tables,
    Views,
    Functions,
    Types,
    Privileges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectKindFilter {
    Any,
    Function,
    Schema,
    Table,
    View,
    Type,
    Relation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKindFilter {
    Any,
    Function,
    View,
}

#[derive(Debug, Clone)]
enum ParsedCommand {
    None,
    Help(Option<String>),
    Connection,
    Session,
    Refresh,
    List {
        kind: ListKind,
        filter: String,
        system: bool,
    },
    Describe {
        target: String,
        kind: ObjectKindFilter,
        system: bool,
    },
    Source {
        target: String,
        kind: SourceKindFilter,
        system: bool,
    },
    ImportTable {
        target: String,
        options: ImportOptions,
    },
    ExportTable {
        target: String,
        options: ExportOptions,
    },
    ExportQuery {
        query: String,
        options: ExportOptions,
    },
    NamedRun {
        name: String,
        values: Vec<NamedValue>,
    },
    NamedSave {
        name: String,
        sql: Option<String>,
    },
    NamedInfo(String),
    NamedList {
        shared: bool,
        all: bool,
    },
    NamedStatus,
    NamedDelete(String),
    Quit,
}

#[derive(Debug, Clone)]
struct CommandInfo {
    name: &'static str,
    usage: &'static str,
    description: &'static str,
    flags: &'static str,
    examples: &'static str,
}

#[derive(Debug, Clone)]
struct Token {
    raw: String,
    cooked: String,
}

#[derive(Debug, Clone)]
struct ObjectTarget {
    schema: Option<String>,
    name: String,
    signature: Option<String>,
    search: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    category: String,
    kind: String,
    schema: String,
    name: String,
    detail: String,
    oid: i64,
}

pub async fn execute(
    input: &str,
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
    named_sql: &NamedSqlContext,
    config_source: &ConfigSource,
) -> AppResult<CommandOutcome> {
    execute_with_metadata_mode(
        input,
        pool,
        completion_catalog,
        Some(named_sql),
        Some(config_source),
        MetadataMode::Interactive,
    )
    .await
}

pub async fn execute_unattended(
    input: &str,
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
    named_sql: Option<&NamedSqlContext>,
    config_source: Option<&ConfigSource>,
    allow_write: bool,
) -> AppResult<CommandOutcome> {
    execute_with_metadata_mode(
        input,
        pool,
        completion_catalog,
        named_sql,
        config_source,
        MetadataMode::Unattended { allow_write },
    )
    .await
}

#[derive(Debug, Clone, Copy)]
enum MetadataMode {
    Interactive,
    Unattended { allow_write: bool },
}

async fn execute_with_metadata_mode(
    input: &str,
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
    named_sql: Option<&NamedSqlContext>,
    config_source: Option<&ConfigSource>,
    metadata_mode: MetadataMode,
) -> AppResult<CommandOutcome> {
    match parse_command(input)? {
        ParsedCommand::None => Ok(CommandOutcome::None),
        ParsedCommand::Help(command) => Ok(output(help_output(command.as_deref()))),
        ParsedCommand::Connection => Ok(output(connection_output(pool).await?)),
        ParsedCommand::Session => {
            let named_sql = require_named_sql_context(named_sql)?;
            let config_source = require_config_source(config_source)?;
            Ok(output(vec![session_section(named_sql, config_source)]))
        }
        ParsedCommand::Refresh => refresh_output(pool, completion_catalog, metadata_mode).await,
        ParsedCommand::List {
            kind,
            filter,
            system,
        } => Ok(output(list_output(pool, kind, &filter, system).await?)),
        ParsedCommand::Describe {
            target,
            kind,
            system,
        } => Ok(output(describe_output(pool, &target, kind, system).await?)),
        ParsedCommand::Source {
            target,
            kind,
            system,
        } => Ok(output(source_output(pool, &target, kind, system).await?)),
        ParsedCommand::ImportTable { target, options } => {
            if matches!(
                metadata_mode,
                MetadataMode::Unattended { allow_write: false }
            ) {
                return Err(AppError::message(
                    "non-interactive import requires the top-level --allow-write flag",
                ));
            }
            Ok(output(vec![transfer_section(
                transfer::import_table(pool, &target, options).await?,
            )]))
        }
        ParsedCommand::ExportTable { target, options } => Ok(output(vec![transfer_section(
            transfer::export_table(pool, &target, options).await?,
        )])),
        ParsedCommand::ExportQuery { query, options } => Ok(output(vec![transfer_section(
            transfer::export_query(pool, &query, options).await?,
        )])),
        ParsedCommand::NamedRun { name, values } => {
            let named_sql = require_named_sql_context(named_sql)?;
            Ok(match named_sql.prepare(&name, &values) {
                Ok(prepared) => CommandOutcome::Run(prepared),
                Err(error) => CommandOutcome::RunFailed(error),
            })
        }
        ParsedCommand::NamedSave { name, sql } => {
            let named_sql = require_named_sql_context(named_sql)?;
            match sql {
                Some(sql) => {
                    require_unattended_write(metadata_mode, "named save")?;
                    Ok(output(named_save_output(named_sql.save(&name, &sql)?)))
                }
                None if matches!(metadata_mode, MetadataMode::Unattended { .. }) => {
                    Err(AppError::message(
                        "named save without SQL is only available in the interactive REPL",
                    ))
                }
                None => Ok(output(edit_named_sql(named_sql, &name)?)),
            }
        }
        ParsedCommand::NamedInfo(name) => {
            let named_sql = require_named_sql_context(named_sql)?;
            Ok(output(named_info_output(named_sql, named_sql.read(&name)?)))
        }
        ParsedCommand::NamedList { shared, all } => {
            let named_sql = require_named_sql_context(named_sql)?;
            Ok(output(named_list_output(named_sql, shared, all)?))
        }
        ParsedCommand::NamedStatus => {
            let named_sql = require_named_sql_context(named_sql)?;
            Ok(output(named_status_output(named_sql)))
        }
        ParsedCommand::NamedDelete(name) => {
            let named_sql = require_named_sql_context(named_sql)?;
            require_unattended_write(metadata_mode, "named delete")?;
            if matches!(metadata_mode, MetadataMode::Interactive) && !confirm_named_delete(&name)? {
                return Ok(output(vec![section(
                    "Named SQL",
                    ResultGrid::from_records(["status", "name"], [["cancelled", name.as_str()]]),
                )]));
            }
            let path = named_sql.delete(&name)?;
            Ok(output(vec![section(
                "Named SQL",
                ResultGrid::from_records(
                    ["status", "name", "path"],
                    [["deleted".to_owned(), name, path.display().to_string()]],
                ),
            )]))
        }
        ParsedCommand::Quit => Ok(CommandOutcome::Exit),
    }
}

pub fn command_uses_runtime_config(input: &str) -> bool {
    matches!(
        input.split_whitespace().next(),
        Some("session" | "run" | "named")
    )
}

fn require_named_sql_context(context: Option<&NamedSqlContext>) -> AppResult<&NamedSqlContext> {
    context.ok_or_else(|| AppError::message("named SQL context was not loaded"))
}

fn require_config_source(source: Option<&ConfigSource>) -> AppResult<&ConfigSource> {
    source.ok_or_else(|| AppError::message("runtime configuration was not loaded"))
}

fn output(sections: Vec<MetaSection>) -> CommandOutcome {
    CommandOutcome::Output(MetaOutput { sections })
}

fn named_save_output(named_sql: NamedSql) -> Vec<MetaSection> {
    let validity = validation_summary(&named_sql.analysis.diagnostics);
    let mut sections = vec![section(
        "Named SQL",
        ResultGrid::from_records(
            ["status", "name", "path", "validity"],
            [[
                "saved".to_owned(),
                named_sql.name,
                named_sql.path.display().to_string(),
                validity,
            ]],
        ),
    )];
    if !named_sql.analysis.diagnostics.is_empty() {
        sections.push(section(
            "Validation warnings",
            ResultGrid::from_records(
                ["warning"],
                named_sql
                    .analysis
                    .diagnostics
                    .into_iter()
                    .map(|warning| [warning]),
            ),
        ));
    }
    sections
}

fn edit_named_sql(context: &NamedSqlContext, name: &str) -> AppResult<Vec<MetaSection>> {
    let path = context.prepare_save_path(name)?;
    let editor = configured_editor()?;
    run_editor(&editor, &path)?;

    if path.try_exists().map_err(|err| {
        AppError::message(format!(
            "failed to inspect named SQL path `{}` after editing: {err}",
            path.display()
        ))
    })? {
        return Ok(named_save_output(context.read(name)?));
    }

    Ok(vec![section(
        "Named SQL",
        ResultGrid::from_records(
            ["status", "name", "path"],
            [[
                "cancelled".to_owned(),
                name.to_owned(),
                path.display().to_string(),
            ]],
        ),
    )])
}

#[derive(Debug, Eq, PartialEq)]
struct ConfiguredEditor {
    variable: &'static str,
    command: String,
}

fn configured_editor() -> AppResult<ConfiguredEditor> {
    let editor = environment_value("EDITOR")?;
    let visual = if editor
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        None
    } else {
        environment_value("VISUAL")?
    };
    select_editor(editor, visual)
}

fn environment_value(name: &'static str) -> AppResult<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            Err(AppError::message(format!("${name} is not valid UTF-8")))
        }
    }
}

fn select_editor(editor: Option<String>, visual: Option<String>) -> AppResult<ConfiguredEditor> {
    editor
        .filter(|value| !value.trim().is_empty())
        .map(|command| ConfiguredEditor {
            variable: "EDITOR",
            command,
        })
        .or_else(|| {
            visual
                .filter(|value| !value.trim().is_empty())
                .map(|command| ConfiguredEditor {
                    variable: "VISUAL",
                    command,
                })
        })
        .ok_or_else(|| {
            AppError::message("named save without SQL requires $EDITOR or $VISUAL to be set")
        })
}

fn run_editor(editor: &ConfiguredEditor, path: &Path) -> AppResult<()> {
    let tokens = tokenize(&editor.command)
        .map_err(|err| AppError::message(format!("failed to parse ${}: {err}", editor.variable)))?;
    let (program, arguments) = tokens.split_first().ok_or_else(|| {
        AppError::message(format!(
            "${} does not contain an editor command",
            editor.variable
        ))
    })?;
    let status = process::Command::new(&program.cooked)
        .args(arguments.iter().map(|argument| &argument.cooked))
        .arg(path)
        .status()
        .map_err(|err| {
            AppError::message(format!(
                "failed to launch editor `{}` from ${}: {err}",
                program.cooked, editor.variable
            ))
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(AppError::message(format!(
            "editor `{}` exited with {status}",
            program.cooked
        )))
    }
}

fn named_info_output(context: &NamedSqlContext, named_sql: NamedSql) -> Vec<MetaSection> {
    let details = [
        ["name".to_owned(), named_sql.name],
        [
            "scope".to_owned(),
            context.scope_label(named_sql.scope).to_owned(),
        ],
        ["path".to_owned(), named_sql.path.display().to_string()],
        [
            "parameters".to_owned(),
            parameter_summary(&named_sql.analysis.parameters),
        ],
        [
            "statements".to_owned(),
            named_sql.analysis.statements.len().to_string(),
        ],
        [
            "validity".to_owned(),
            validation_summary(&named_sql.analysis.diagnostics),
        ],
    ];
    let mut sections = vec![section(
        "Named SQL info",
        ResultGrid::from_records(["field", "value"], details),
    )];
    if !named_sql.analysis.diagnostics.is_empty() {
        sections.push(section(
            "Validation",
            ResultGrid::from_records(
                ["diagnostic"],
                named_sql
                    .analysis
                    .diagnostics
                    .iter()
                    .map(|diagnostic| [diagnostic]),
            ),
        ));
    }
    sections.push(section(
        "SQL",
        ResultGrid::from_records(["sql"], [[named_sql.sql]]),
    ));
    sections
}

fn named_list_output(
    context: &NamedSqlContext,
    shared: bool,
    all: bool,
) -> AppResult<Vec<MetaSection>> {
    let entries = context.list(shared, all)?;
    Ok(vec![section(
        format!("Named SQL ({})", context.display_name()),
        ResultGrid::from_records(
            ["name", "parameters", "validity"],
            entries.into_iter().map(|entry| {
                [
                    entry.name,
                    parameter_summary(&entry.parameters),
                    validation_summary(&entry.diagnostics),
                ]
            }),
        ),
    )])
}

fn named_status_output(context: &NamedSqlContext) -> Vec<MetaSection> {
    let details = [
        [
            "active context".to_owned(),
            context.display_name().to_owned(),
        ],
        [
            "active root".to_owned(),
            context.root(NamedSqlScope::Local).display().to_string(),
        ],
        [
            "active root source".to_owned(),
            context.root_source_label(NamedSqlScope::Local),
        ],
        [
            "shared root".to_owned(),
            context.root(NamedSqlScope::Shared).display().to_string(),
        ],
        [
            "shared root source".to_owned(),
            context.root_source_label(NamedSqlScope::Shared),
        ],
    ];
    vec![section(
        "Named SQL status",
        ResultGrid::from_records(["field", "value"], details),
    )]
}

fn parameter_summary(parameters: &[String]) -> String {
    if parameters.is_empty() {
        "-".to_owned()
    } else {
        parameters.join(", ")
    }
}

fn validation_summary(diagnostics: &[String]) -> String {
    if diagnostics.is_empty() {
        "valid".to_owned()
    } else {
        format!("invalid: {}", diagnostics.join("; "))
    }
}

fn require_unattended_write(mode: MetadataMode, command: &str) -> AppResult<()> {
    if matches!(mode, MetadataMode::Unattended { allow_write: false }) {
        Err(AppError::message(format!(
            "non-interactive {command} requires the top-level --allow-write flag"
        )))
    } else {
        Ok(())
    }
}

fn confirm_named_delete(name: &str) -> AppResult<bool> {
    print!("Delete named SQL `{name}`? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn parse_command(input: &str) -> AppResult<ParsedCommand> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(ParsedCommand::None);
    }
    if is_named_save_command(input)
        && !input
            .split_whitespace()
            .nth(2)
            .is_some_and(|word| matches!(word, "-h" | "--help"))
    {
        return parse_named_save(input);
    }
    if input.contains(';')
        && !is_export_query_command(input)
        && !is_named_save_command(input)
        && !is_named_run_command(input)
    {
        return Err(AppError::message(
            "Commands do not use semicolons. Try the command again without `;`.",
        ));
    }

    let tokens = tokenize(input).map_err(AppError::message)?;
    if tokens.is_empty() {
        return Ok(ParsedCommand::None);
    }

    if has_help_token(&tokens) {
        return Ok(ParsedCommand::Help(Some(tokens[0].cooked.clone())));
    }

    let matches = command_spec()
        .try_get_matches_from(tokens.iter().map(|token| token.cooked.clone()))
        .map_err(|err| AppError::message(command_parse_error(&err.to_string())))?;

    let Some((name, matches)) = matches.subcommand() else {
        return Ok(ParsedCommand::None);
    };

    match name {
        "help" => Ok(ParsedCommand::Help(command_help_target(&tokens))),
        "connection" => Ok(ParsedCommand::Connection),
        "session" => Ok(ParsedCommand::Session),
        "refresh" => Ok(ParsedCommand::Refresh),
        "schemas" => Ok(ParsedCommand::List {
            kind: ListKind::Schemas,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "databases" => Ok(ParsedCommand::List {
            kind: ListKind::Databases,
            filter: cooked_positionals(&tokens, &[]),
            system: false,
        }),
        "roles" => Ok(ParsedCommand::List {
            kind: ListKind::Roles,
            filter: cooked_positionals(&tokens, &[]),
            system: false,
        }),
        "extensions" => Ok(ParsedCommand::List {
            kind: ListKind::Extensions,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "tables" => Ok(ParsedCommand::List {
            kind: ListKind::Tables,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "views" => Ok(ParsedCommand::List {
            kind: ListKind::Views,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "functions" => Ok(ParsedCommand::List {
            kind: ListKind::Functions,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "types" => Ok(ParsedCommand::List {
            kind: ListKind::Types,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "privileges" => Ok(ParsedCommand::List {
            kind: ListKind::Privileges,
            filter: cooked_positionals(&tokens, SYSTEM_FLAG),
            system: matches.get_flag("system"),
        }),
        "describe" => Ok(ParsedCommand::Describe {
            target: raw_positionals(&tokens, DESCRIBE_FLAGS),
            kind: describe_kind(matches),
            system: matches.get_flag("system"),
        }),
        "source" => Ok(ParsedCommand::Source {
            target: raw_positionals(&tokens, SOURCE_FLAGS),
            kind: source_kind(matches),
            system: matches.get_flag("system"),
        }),
        "import" => parse_import_command(matches),
        "export" => parse_export_command(matches),
        "run" => parse_named_run(&tokens, 1),
        "named" => parse_named_command(matches, &tokens),
        "quit" => Ok(ParsedCommand::Quit),
        _ => unreachable!("clap only returns configured commands"),
    }
}

fn command_spec() -> Command {
    Command::new("commands")
        .no_binary_name(true)
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .subcommand(Command::new("help").arg(words_arg("command", false)))
        .subcommand(Command::new("connection"))
        .subcommand(Command::new("session"))
        .subcommand(Command::new("refresh"))
        .subcommand(list_command("schemas"))
        .subcommand(Command::new("databases").arg(words_arg("filter", false)))
        .subcommand(Command::new("roles").arg(words_arg("filter", false)))
        .subcommand(list_command("extensions"))
        .subcommand(list_command("tables"))
        .subcommand(list_command("views"))
        .subcommand(list_command("functions"))
        .subcommand(list_command("types"))
        .subcommand(list_command("privileges"))
        .subcommand(
            Command::new("describe")
                .arg(words_arg("name", true))
                .arg(kind_flag("function", 'f', "function"))
                .arg(kind_flag("schema", 's', "schema"))
                .arg(kind_flag("table", 't', "table"))
                .arg(kind_flag("view", 'v', "view"))
                .arg(kind_flag("type", 'T', "type"))
                .arg(kind_flag("relation", 'r', "relation"))
                .arg(system_arg())
                .group(
                    ArgGroup::new("kind")
                        .args(["function", "schema", "table", "view", "type", "relation"])
                        .multiple(false),
                ),
        )
        .subcommand(
            Command::new("source")
                .arg(words_arg("name", true))
                .arg(kind_flag("function", 'f', "function"))
                .arg(kind_flag("view", 'v', "view"))
                .arg(system_arg())
                .group(
                    ArgGroup::new("kind")
                        .args(["function", "view"])
                        .multiple(false),
                ),
        )
        .subcommand(import_command())
        .subcommand(export_command())
        .subcommand(named_run_command("run"))
        .subcommand(named_command())
        .subcommand(Command::new("quit"))
}

fn named_command() -> Command {
    Command::new("named")
        .subcommand_required(true)
        .subcommand(named_run_command("run"))
        .subcommand(
            Command::new("save")
                .arg(Arg::new("name").required(true))
                .arg(Arg::new("sql").num_args(1..)),
        )
        .subcommand(Command::new("info").arg(Arg::new("name").required(true)))
        .subcommand(Command::new("status"))
        .subcommand(
            Command::new("list")
                .arg(Arg::new("shared").long("shared").action(ArgAction::SetTrue))
                .arg(Arg::new("all").long("all").action(ArgAction::SetTrue))
                .group(
                    ArgGroup::new("scope")
                        .args(["shared", "all"])
                        .multiple(false),
                ),
        )
        .subcommand(Command::new("delete").arg(Arg::new("name").required(true)))
}

fn named_run_command(name: &'static str) -> Command {
    Command::new(name)
        .arg(Arg::new("name").required(true))
        .arg(Arg::new("params").num_args(0..).allow_hyphen_values(true))
}

fn import_command() -> Command {
    Command::new("import").subcommand_required(true).subcommand(
        Command::new("table")
            .arg(required_value_arg("name", "name", "TABLE"))
            .arg(required_value_arg("input", "input", "PATH"))
            .arg(csv_format_arg())
            .arg(no_header_arg()),
    )
}

fn export_command() -> Command {
    Command::new("export")
        .subcommand_required(true)
        .subcommand(
            Command::new("table")
                .arg(required_value_arg("name", "name", "RELATION"))
                .arg(export_output_arg())
                .arg(csv_format_arg())
                .arg(no_header_arg())
                .arg(force_arg()),
        )
        .subcommand(
            Command::new("query")
                .arg(required_value_arg("sql", "sql", "SQL"))
                .arg(export_output_arg())
                .arg(csv_format_arg())
                .arg(no_header_arg())
                .arg(force_arg()),
        )
}

fn required_value_arg(id: &'static str, long: &'static str, value_name: &'static str) -> Arg {
    Arg::new(id)
        .long(long)
        .value_name(value_name)
        .required(true)
}

fn export_output_arg() -> Arg {
    required_value_arg("output", "output", "PATH")
}

fn csv_format_arg() -> Arg {
    Arg::new("format")
        .long("format")
        .value_name("FORMAT")
        .value_parser(["csv"])
}

fn no_header_arg() -> Arg {
    Arg::new("no-header")
        .long("no-header")
        .action(ArgAction::SetTrue)
}

fn force_arg() -> Arg {
    Arg::new("force").long("force").action(ArgAction::SetTrue)
}

fn parse_import_command(matches: &clap::ArgMatches) -> AppResult<ParsedCommand> {
    let Some(("table", matches)) = matches.subcommand() else {
        return Err(AppError::message("import requires the table subcommand"));
    };
    Ok(ParsedCommand::ImportTable {
        target: required_value(matches, "name")?,
        options: ImportOptions {
            input: PathBuf::from(required_value(matches, "input")?),
            header: !matches.get_flag("no-header"),
            format_explicit: matches.get_one::<String>("format").is_some(),
        },
    })
}

fn parse_export_command(matches: &clap::ArgMatches) -> AppResult<ParsedCommand> {
    let Some((kind, matches)) = matches.subcommand() else {
        return Err(AppError::message(
            "export requires the table or query subcommand",
        ));
    };
    let options = ExportOptions {
        output: PathBuf::from(required_value(matches, "output")?),
        header: !matches.get_flag("no-header"),
        force: matches.get_flag("force"),
        format_explicit: matches.get_one::<String>("format").is_some(),
    };
    match kind {
        "table" => Ok(ParsedCommand::ExportTable {
            target: required_value(matches, "name")?,
            options,
        }),
        "query" => Ok(ParsedCommand::ExportQuery {
            query: required_value(matches, "sql")?,
            options,
        }),
        _ => Err(AppError::message("unknown export subcommand")),
    }
}

fn parse_named_command(matches: &clap::ArgMatches, tokens: &[Token]) -> AppResult<ParsedCommand> {
    let Some((subcommand, matches)) = matches.subcommand() else {
        return Err(AppError::message("named requires a subcommand"));
    };
    match subcommand {
        "run" => parse_named_run(tokens, 2),
        "save" => Ok(ParsedCommand::NamedSave {
            name: required_positional(matches, "name")?,
            sql: matches
                .get_many::<String>("sql")
                .map(|values| values.cloned().collect::<Vec<_>>().join(" ")),
        }),
        "info" => Ok(ParsedCommand::NamedInfo(required_positional(
            matches, "name",
        )?)),
        "list" => Ok(ParsedCommand::NamedList {
            shared: matches.get_flag("shared"),
            all: matches.get_flag("all"),
        }),
        "status" => Ok(ParsedCommand::NamedStatus),
        "delete" => Ok(ParsedCommand::NamedDelete(required_positional(
            matches, "name",
        )?)),
        _ => Err(AppError::message("unknown named subcommand")),
    }
}

fn parse_named_run(tokens: &[Token], name_index: usize) -> AppResult<ParsedCommand> {
    let name = tokens
        .get(name_index)
        .map(|token| token.cooked.clone())
        .ok_or_else(|| AppError::message("run requires a named SQL name"))?;
    let values = tokens
        .iter()
        .skip(name_index + 1)
        .map(|token| parse_named_value(&token.raw, &token.cooked))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(ParsedCommand::NamedRun { name, values })
}

fn parse_named_save(input: &str) -> AppResult<ParsedCommand> {
    let after_named = input
        .strip_prefix("named")
        .and_then(|input| input.strip_prefix(char::is_whitespace))
        .map(str::trim_start)
        .ok_or_else(|| AppError::message("invalid named save command"))?;
    let after_save = after_named
        .strip_prefix("save")
        .and_then(|input| input.strip_prefix(char::is_whitespace))
        .map(str::trim_start)
        .ok_or_else(|| AppError::message("named save requires a name"))?;
    let (name, sql) = after_save.find(char::is_whitespace).map_or_else(
        || (after_save, None),
        |name_end| {
            let sql = after_save[name_end..].trim_start();
            (
                &after_save[..name_end],
                (!sql.is_empty()).then(|| sql.to_owned()),
            )
        },
    );
    Ok(ParsedCommand::NamedSave {
        name: name.to_owned(),
        sql,
    })
}

fn required_positional(matches: &clap::ArgMatches, id: &str) -> AppResult<String> {
    matches
        .get_one::<String>(id)
        .cloned()
        .ok_or_else(|| AppError::message(format!("missing required {id}")))
}

fn required_value(matches: &clap::ArgMatches, id: &str) -> AppResult<String> {
    matches
        .get_one::<String>(id)
        .cloned()
        .ok_or_else(|| AppError::message(format!("missing required --{id}")))
}

fn is_export_query_command(input: &str) -> bool {
    let mut words = input.split_whitespace();
    words.next() == Some("export") && words.next() == Some("query")
}

fn is_named_save_command(input: &str) -> bool {
    let mut words = input.split_whitespace();
    words.next() == Some("named") && words.next() == Some("save")
}

fn is_named_run_command(input: &str) -> bool {
    let mut words = input.split_whitespace();
    match words.next() {
        Some("run") => true,
        Some("named") => words.next() == Some("run"),
        _ => false,
    }
}

fn list_command(name: &'static str) -> Command {
    Command::new(name)
        .arg(words_arg("filter", false))
        .arg(system_arg())
}

fn words_arg(name: &'static str, required: bool) -> Arg {
    let mut arg = Arg::new(name).num_args(if required { 1.. } else { 0.. });
    if required {
        arg = arg.required(true);
    }
    arg
}

fn system_arg() -> Arg {
    Arg::new("system")
        .short('x')
        .long("system")
        .action(ArgAction::SetTrue)
}

fn kind_flag(id: &'static str, short: char, long: &'static str) -> Arg {
    Arg::new(id)
        .short(short)
        .long(long)
        .action(ArgAction::SetTrue)
}

fn describe_kind(matches: &clap::ArgMatches) -> ObjectKindFilter {
    if matches.get_flag("function") {
        ObjectKindFilter::Function
    } else if matches.get_flag("schema") {
        ObjectKindFilter::Schema
    } else if matches.get_flag("table") {
        ObjectKindFilter::Table
    } else if matches.get_flag("view") {
        ObjectKindFilter::View
    } else if matches.get_flag("type") {
        ObjectKindFilter::Type
    } else if matches.get_flag("relation") {
        ObjectKindFilter::Relation
    } else {
        ObjectKindFilter::Any
    }
}

fn source_kind(matches: &clap::ArgMatches) -> SourceKindFilter {
    if matches.get_flag("function") {
        SourceKindFilter::Function
    } else if matches.get_flag("view") {
        SourceKindFilter::View
    } else {
        SourceKindFilter::Any
    }
}

fn is_help_token(token: &Token) -> bool {
    matches!(token.cooked.as_str(), "-h" | "--help")
}

fn has_help_token(tokens: &[Token]) -> bool {
    tokens
        .iter()
        .skip(1)
        .take_while(|token| token.cooked != "--")
        .any(is_help_token)
}

fn command_help_target(tokens: &[Token]) -> Option<String> {
    tokens
        .iter()
        .skip(1)
        .find(|token| !is_help_token(token))
        .map(|token| token.cooked.clone())
}

fn raw_positionals(tokens: &[Token], flags: &[&str]) -> String {
    positionals(tokens, flags, |token| token.raw.clone())
}

fn cooked_positionals(tokens: &[Token], flags: &[&str]) -> String {
    positionals(tokens, flags, |token| token.cooked.clone())
}

fn positionals(tokens: &[Token], flags: &[&str], value: impl Fn(&Token) -> String) -> String {
    let mut after_double_dash = false;
    tokens
        .iter()
        .skip(1)
        .filter_map(|token| {
            if !after_double_dash && token.cooked == "--" {
                after_double_dash = true;
                return None;
            }
            if !after_double_dash && flags.contains(&token.cooked.as_str()) {
                return None;
            }
            Some(value(token))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_parse_error(error: &str) -> String {
    format!("{error}\n\nTry:\n  help")
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((start, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        let mut cooked = String::new();
        let mut end = start;

        while let Some((idx, ch)) = chars.peek().copied() {
            if ch.is_whitespace() {
                break;
            }

            match ch {
                '\'' | '"' => {
                    let quote = ch;
                    chars.next();
                    end = idx + ch.len_utf8();
                    let mut closed = false;

                    while let Some((quoted_idx, quoted_ch)) = chars.next() {
                        end = quoted_idx + quoted_ch.len_utf8();
                        if quoted_ch == quote {
                            closed = true;
                            break;
                        }
                        if quoted_ch == '\\' {
                            if let Some((escaped_idx, escaped_ch)) = chars.next() {
                                end = escaped_idx + escaped_ch.len_utf8();
                                cooked.push(escaped_ch);
                            } else {
                                return Err("unterminated escape in quoted string".to_owned());
                            }
                        } else {
                            cooked.push(quoted_ch);
                        }
                    }

                    if !closed {
                        return Err("unterminated quoted string".to_owned());
                    }
                }
                '\\' => {
                    chars.next();
                    if let Some((escaped_idx, escaped_ch)) = chars.next() {
                        end = escaped_idx + escaped_ch.len_utf8();
                        cooked.push(escaped_ch);
                    } else {
                        return Err("unterminated escape".to_owned());
                    }
                }
                _ => {
                    chars.next();
                    end = idx + ch.len_utf8();
                    cooked.push(ch);
                }
            }
        }

        tokens.push(Token {
            raw: input[start..end].to_owned(),
            cooked,
        });
    }

    Ok(tokens)
}

async fn refresh_output(
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
    metadata_mode: MetadataMode,
) -> AppResult<CommandOutcome> {
    let catalog = match metadata_mode {
        MetadataMode::Interactive => Catalog::load(pool).await?,
        MetadataMode::Unattended { .. } => Catalog::load_unattended(pool).await?,
    };
    let summary = catalog.summary();
    *completion_catalog
        .write()
        .expect("completion catalog is not poisoned") = catalog;

    Ok(output(vec![section(
        "Refresh",
        ResultGrid::from_records(["status", "metadata"], [["refreshed".to_owned(), summary]]),
    )]))
}

async fn connection_output(pool: &PgPool) -> AppResult<Vec<MetaSection>> {
    let rows = sqlx::query(
        r#"
        select current_database() as database,
               current_user as "user",
               coalesce(inet_server_addr()::text, 'local socket') as host,
               coalesce(inet_server_port()::text, '') as port,
               current_setting('server_version') as server,
               pg_backend_pid()::text as pid,
               case when ssl then 'on' else 'off' end as ssl,
               current_schema() as schema,
               current_setting('search_path') as path
        from pg_stat_ssl
        where pid = pg_backend_pid()
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(vec![section("Connection", ResultGrid::from_rows(&rows))])
}

fn session_section(named_sql: &NamedSqlContext, config_source: &ConfigSource) -> MetaSection {
    let history = named_sql
        .history_path()
        .map_or_else(|| "disabled".to_owned(), |path| path.display().to_string());
    section(
        "DBCrab session",
        ResultGrid::from_records(
            ["field", "value"],
            [
                ["context".to_owned(), named_sql.display_name().to_owned()],
                [
                    "context source".to_owned(),
                    named_sql.context_source_label(),
                ],
                ["history".to_owned(), history],
                ["user config".to_owned(), config_source.label()],
                [
                    "project config".to_owned(),
                    named_sql.project_config_label(),
                ],
            ],
        ),
    )
}

async fn list_output(
    pool: &PgPool,
    kind: ListKind,
    filter: &str,
    system: bool,
) -> AppResult<Vec<MetaSection>> {
    match kind {
        ListKind::Schemas => query_list(pool, "Schemas", SCHEMAS_SQL, system, filter).await,
        ListKind::Databases => query_list(pool, "Databases", DATABASES_SQL, false, filter).await,
        ListKind::Roles => query_list(pool, "Roles", ROLES_SQL, false, filter).await,
        ListKind::Extensions => {
            query_list(pool, "Extensions", EXTENSIONS_SQL, system, filter).await
        }
        ListKind::Tables => query_list(pool, "Tables", TABLES_SQL, system, filter).await,
        ListKind::Views => query_list(pool, "Views", VIEWS_SQL, system, filter).await,
        ListKind::Functions => query_list(pool, "Functions", FUNCTIONS_SQL, system, filter).await,
        ListKind::Types => query_list(pool, "Types", TYPES_SQL, system, filter).await,
        ListKind::Privileges => {
            query_list(pool, "Privileges", PRIVILEGES_SQL, system, filter).await
        }
    }
}

async fn query_list(
    pool: &PgPool,
    title: &str,
    sql: &'static str,
    system: bool,
    filter: &str,
) -> AppResult<Vec<MetaSection>> {
    let rows = sqlx::query(sql)
        .bind(system)
        .bind(filter)
        .fetch_all(pool)
        .await?;

    Ok(vec![section(title, ResultGrid::from_rows(&rows))])
}

async fn describe_output(
    pool: &PgPool,
    target: &str,
    kind: ObjectKindFilter,
    system: bool,
) -> AppResult<Vec<MetaSection>> {
    let parsed = parse_object_target(target);
    let exact_candidates =
        candidates(pool, &parsed, CandidateMode::Describe(kind), system, true).await?;

    match exact_candidates.as_slice() {
        [candidate] => describe_candidate(pool, candidate).await,
        [] => {
            let fuzzy =
                candidates(pool, &parsed, CandidateMode::Describe(kind), system, false).await?;
            Ok(vec![section("Candidates", candidates_grid(&fuzzy, target))])
        }
        _ => Ok(vec![section(
            "Candidates",
            candidates_grid(&exact_candidates, target),
        )]),
    }
}

async fn source_output(
    pool: &PgPool,
    target: &str,
    kind: SourceKindFilter,
    system: bool,
) -> AppResult<Vec<MetaSection>> {
    let parsed = parse_object_target(target);
    let exact_candidates =
        candidates(pool, &parsed, CandidateMode::Source(kind), system, true).await?;

    match exact_candidates.as_slice() {
        [candidate] if candidate.category == "function" => {
            source_function(pool, candidate.oid).await
        }
        [candidate] if candidate.category == "relation" => source_view(pool, candidate.oid).await,
        [candidate] => Ok(vec![section(
            "Source",
            ResultGrid::from_records(
                ["message"],
                [[format!(
                    "source is not available for {} {}.{}",
                    candidate.kind, candidate.schema, candidate.name
                )]],
            ),
        )]),
        [] => {
            let fuzzy =
                candidates(pool, &parsed, CandidateMode::Source(kind), system, false).await?;
            Ok(vec![section("Candidates", candidates_grid(&fuzzy, target))])
        }
        _ => Ok(vec![section(
            "Candidates",
            candidates_grid(&exact_candidates, target),
        )]),
    }
}

async fn describe_candidate(pool: &PgPool, candidate: &Candidate) -> AppResult<Vec<MetaSection>> {
    match candidate.category.as_str() {
        "relation" => describe_relation(pool, candidate.oid).await,
        "function" => describe_function(pool, candidate.oid).await,
        "schema" => describe_schema(pool, &candidate.name).await,
        "type" => describe_type(pool, candidate.oid).await,
        _ => Ok(vec![section(
            "Describe",
            ResultGrid::from_records(["message"], [["unsupported object kind"]]),
        )]),
    }
}

async fn describe_relation(pool: &PgPool, oid: i64) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section("Relation", oid_query(pool, RELATION_DETAIL_SQL, oid).await?),
        section("Columns", oid_query(pool, RELATION_COLUMNS_SQL, oid).await?),
        section("Indexes", oid_query(pool, RELATION_INDEXES_SQL, oid).await?),
        section(
            "Constraints",
            oid_query(pool, RELATION_CONSTRAINTS_SQL, oid).await?,
        ),
        section(
            "Privileges",
            oid_query(pool, RELATION_PRIVILEGES_SQL, oid).await?,
        ),
    ])
}

async fn describe_function(pool: &PgPool, oid: i64) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section("Function", oid_query(pool, FUNCTION_DETAIL_SQL, oid).await?),
        section(
            "Arguments",
            oid_query(pool, FUNCTION_ARGUMENTS_SQL, oid).await?,
        ),
        section(
            "Privileges",
            oid_query(pool, FUNCTION_PRIVILEGES_SQL, oid).await?,
        ),
    ])
}

async fn describe_schema(pool: &PgPool, schema: &str) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section(
            "Schema",
            schema_query(pool, SCHEMA_DETAIL_SQL, schema).await?,
        ),
        section(
            "Objects",
            schema_query(pool, SCHEMA_OBJECTS_SQL, schema).await?,
        ),
        section(
            "Privileges",
            schema_query(pool, SCHEMA_PRIVILEGES_SQL, schema).await?,
        ),
    ])
}

async fn describe_type(pool: &PgPool, oid: i64) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section("Type", oid_query(pool, TYPE_DETAIL_SQL, oid).await?),
        section("Enum Values", oid_query(pool, TYPE_ENUM_SQL, oid).await?),
        section(
            "Composite Fields",
            oid_query(pool, TYPE_COMPOSITE_SQL, oid).await?,
        ),
        section("Domain", oid_query(pool, TYPE_DOMAIN_SQL, oid).await?),
        section(
            "Domain Constraints",
            oid_query(pool, TYPE_DOMAIN_CONSTRAINTS_SQL, oid).await?,
        ),
        section("Range", oid_query(pool, TYPE_RANGE_SQL, oid).await?),
        section(
            "Privileges",
            oid_query(pool, TYPE_PRIVILEGES_SQL, oid).await?,
        ),
    ])
}

async fn source_function(pool: &PgPool, oid: i64) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section("Function", oid_query(pool, FUNCTION_DETAIL_SQL, oid).await?),
        section("Source", oid_query(pool, FUNCTION_SOURCE_SQL, oid).await?),
    ])
}

async fn source_view(pool: &PgPool, oid: i64) -> AppResult<Vec<MetaSection>> {
    Ok(vec![
        section("View", oid_query(pool, RELATION_DETAIL_SQL, oid).await?),
        section("Source", oid_query(pool, VIEW_SOURCE_SQL, oid).await?),
    ])
}

async fn oid_query(pool: &PgPool, sql: &'static str, oid: i64) -> AppResult<ResultGrid> {
    let rows = sqlx::query(sql).bind(oid).fetch_all(pool).await?;
    Ok(ResultGrid::from_rows(&rows))
}

async fn schema_query(pool: &PgPool, sql: &'static str, schema: &str) -> AppResult<ResultGrid> {
    let rows = sqlx::query(sql).bind(schema).fetch_all(pool).await?;
    Ok(ResultGrid::from_rows(&rows))
}

fn section(title: impl Into<String>, grid: ResultGrid) -> MetaSection {
    MetaSection {
        title: title.into(),
        grid,
    }
}

fn transfer_section(summary: TransferSummary) -> MetaSection {
    let rows = vec![vec![
        CellValue::Text(summary.operation.to_string()),
        CellValue::Text(summary.source),
        CellValue::Text(summary.path.display().to_string()),
        CellValue::Text("csv".to_owned()),
        CellValue::Text(summary.bytes.to_string()),
        summary
            .rows
            .map_or(CellValue::Null, |rows| CellValue::Text(rows.to_string())),
        CellValue::Text(summary.elapsed.as_millis().to_string()),
    ]];
    section(
        "Transfer",
        ResultGrid::from_cells(
            vec![
                "operation".to_owned(),
                "source".to_owned(),
                "path".to_owned(),
                "format".to_owned(),
                "bytes".to_owned(),
                "rows".to_owned(),
                "elapsed_ms".to_owned(),
            ],
            vec!["text".to_owned(); 7],
            vec![None; 7],
            rows,
        ),
    )
}

#[derive(Debug, Clone, Copy)]
enum CandidateMode {
    Describe(ObjectKindFilter),
    Source(SourceKindFilter),
}

async fn candidates(
    pool: &PgPool,
    target: &ObjectTarget,
    mode: CandidateMode,
    system: bool,
    exact: bool,
) -> AppResult<Vec<Candidate>> {
    let rows = sqlx::query(CANDIDATES_SQL)
        .bind(system)
        .bind(target.schema.as_deref().unwrap_or(""))
        .bind(&target.name)
        .bind(target.signature.as_deref().unwrap_or(""))
        .bind(exact)
        .bind(&target.search)
        .bind(include_relations(mode))
        .bind(include_functions(mode))
        .bind(include_schemas(mode))
        .bind(include_types(mode))
        .bind(include_tables(mode))
        .bind(include_views(mode))
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(Candidate {
                category: row.try_get("category")?,
                kind: row.try_get("kind")?,
                schema: row.try_get("schema")?,
                name: row.try_get("name")?,
                detail: row.try_get("detail")?,
                oid: row.try_get("oid")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

fn include_relations(mode: CandidateMode) -> bool {
    match mode {
        CandidateMode::Describe(kind) => matches!(
            kind,
            ObjectKindFilter::Any
                | ObjectKindFilter::Relation
                | ObjectKindFilter::Table
                | ObjectKindFilter::View
        ),
        CandidateMode::Source(kind) => {
            matches!(kind, SourceKindFilter::Any | SourceKindFilter::View)
        }
    }
}

fn include_functions(mode: CandidateMode) -> bool {
    match mode {
        CandidateMode::Describe(kind) => {
            matches!(kind, ObjectKindFilter::Any | ObjectKindFilter::Function)
        }
        CandidateMode::Source(kind) => {
            matches!(kind, SourceKindFilter::Any | SourceKindFilter::Function)
        }
    }
}

fn include_schemas(mode: CandidateMode) -> bool {
    matches!(
        mode,
        CandidateMode::Describe(ObjectKindFilter::Any | ObjectKindFilter::Schema)
    )
}

fn include_types(mode: CandidateMode) -> bool {
    matches!(
        mode,
        CandidateMode::Describe(ObjectKindFilter::Any | ObjectKindFilter::Type)
    )
}

fn include_tables(mode: CandidateMode) -> bool {
    matches!(mode, CandidateMode::Describe(ObjectKindFilter::Table))
}

fn include_views(mode: CandidateMode) -> bool {
    matches!(
        mode,
        CandidateMode::Describe(ObjectKindFilter::View)
            | CandidateMode::Source(SourceKindFilter::Any | SourceKindFilter::View)
    )
}

fn candidates_grid(candidates: &[Candidate], target: &str) -> ResultGrid {
    if candidates.is_empty() {
        return ResultGrid::from_records(["message"], [[format!("No object matched `{target}`")]]);
    }

    ResultGrid::from_records(
        ["kind", "schema", "name", "detail"],
        candidates.iter().map(|candidate| {
            [
                candidate.kind.clone(),
                candidate.schema.clone(),
                candidate.name.clone(),
                candidate.detail.clone(),
            ]
        }),
    )
}

fn parse_object_target(raw: &str) -> ObjectTarget {
    let trimmed = raw.trim();
    let (name_part, signature) = split_signature(trimmed);
    let parts = parse_identifier_path(name_part).unwrap_or_default();

    match parts.as_slice() {
        [name] => ObjectTarget {
            schema: None,
            name: name.clone(),
            signature,
            search: trimmed.to_owned(),
        },
        [schema, name] => ObjectTarget {
            schema: Some(schema.clone()),
            name: name.clone(),
            signature,
            search: trimmed.to_owned(),
        },
        _ => ObjectTarget {
            schema: None,
            name: trimmed.to_ascii_lowercase(),
            signature,
            search: trimmed.to_owned(),
        },
    }
}

fn split_signature(input: &str) -> (&str, Option<String>) {
    let Some(open) = find_unquoted(input, '(') else {
        return (input, None);
    };
    if !input.ends_with(')') {
        return (input, None);
    }

    let signature = input[open + 1..input.len() - 1].trim().to_owned();
    (&input[..open], Some(signature))
}

fn parse_identifier_path(input: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut rest = input.trim();

    while !rest.is_empty() {
        let (part, remaining) = parse_identifier(rest)?;
        parts.push(part);
        rest = remaining.trim_start();
        if rest.is_empty() {
            break;
        }
        rest = rest.strip_prefix('.')?.trim_start();
    }

    (!parts.is_empty()).then_some(parts)
}

fn parse_identifier(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    if let Some(mut rest) = input.strip_prefix('"') {
        let mut value = String::new();
        loop {
            let index = rest.find('"')?;
            value.push_str(&rest[..index]);
            rest = &rest[index + 1..];
            if rest.starts_with('"') {
                value.push('"');
                rest = &rest[1..];
            } else {
                return Some((value, rest));
            }
        }
    }

    let end = input
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_')
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())?;
    Some((input[..end].to_ascii_lowercase(), &input[end..]))
}

fn find_unquoted(input: &str, needle: char) -> Option<usize> {
    let mut in_double_quote = false;
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if in_double_quote {
            if ch == '"' {
                if matches!(chars.peek(), Some((_, '"'))) {
                    chars.next();
                } else {
                    in_double_quote = false;
                }
            }
            continue;
        }

        match ch {
            '"' => in_double_quote = true,
            ch if ch == needle => return Some(idx),
            _ => {}
        }
    }

    None
}

fn help_output(command: Option<&str>) -> Vec<MetaSection> {
    match command {
        Some(command) => {
            let rows = COMMANDS
                .iter()
                .filter(|info| info.name == command)
                .map(command_info_row)
                .collect::<Vec<_>>();

            let grid = if rows.is_empty() {
                ResultGrid::from_records(["message"], [[format!("Unknown command `{command}`")]])
            } else {
                ResultGrid::from_records(help_columns(), rows)
            };

            vec![section("Help", grid)]
        }
        None => vec![section(
            "Help",
            ResultGrid::from_records(help_columns(), COMMANDS.iter().map(command_info_row)),
        )],
    }
}

fn help_columns() -> [&'static str; 5] {
    ["command", "usage", "description", "flags", "examples"]
}

fn command_info_row(info: &CommandInfo) -> [String; 5] {
    [
        info.name.to_owned(),
        info.usage.to_owned(),
        info.description.to_owned(),
        info.flags.to_owned(),
        info.examples.to_owned(),
    ]
}

#[cfg(test)]
fn command_suggestions(line: &str, pos: usize, catalog: &SharedCatalog) -> Vec<Suggestion> {
    command_suggestions_with_named(line, pos, catalog, None)
}

fn command_suggestions_with_named(
    line: &str,
    pos: usize,
    catalog: &SharedCatalog,
    named_sql: Option<&NamedSqlContext>,
) -> Vec<Suggestion> {
    let pos = pos.min(line.len());
    let span = command_token_span(line, pos);
    let prefix = &line[span.start..span.end];
    let command = line.split_whitespace().next().unwrap_or_default();
    let words_before = line[..span.start].split_whitespace().collect::<Vec<_>>();
    let subcommand = words_before.get(1).copied();
    let previous = words_before.last().copied();

    let candidates = if span.start == 0 {
        COMMANDS
            .iter()
            .filter(|info| crate::fuzzy::find(info.name, prefix).is_some())
            .map(|info| command_suggestion(info.name, info.description, span, true))
            .collect::<Vec<_>>()
    } else if matches!(command, "import" | "export") && words_before.len() == 1 {
        transfer_subcommand_suggestions(command, prefix, span)
    } else if command == "named" && words_before.len() == 1 {
        named_subcommand_suggestions(prefix, span)
    } else if prefix.starts_with('-') {
        flag_suggestions(command, subcommand, prefix, span)
    } else if (command == "run" && words_before.len() == 1)
        || (command == "named"
            && words_before.len() == 2
            && matches!(subcommand, Some("run" | "save" | "info" | "delete")))
    {
        named_sql.map_or_else(Vec::new, |named_sql| {
            named_sql_name_suggestions(named_sql, prefix, span)
        })
    } else if (matches!(command, "import" | "export") && previous == Some("--name"))
        || matches!(command, "describe" | "source" | "tables" | "views")
    {
        object_suggestions(prefix, span, catalog)
    } else {
        Vec::new()
    };

    let mut matches = dedupe_suggestions(candidates)
        .into_iter()
        .filter_map(|mut suggestion| {
            let matched = crate::fuzzy::find(&suggestion.value, prefix)?;
            suggestion.match_indices = Some(matched.indices);
            Some((matched.rank, suggestion))
        })
        .collect::<Vec<_>>();
    if !prefix.is_empty() {
        matches.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left.value.len().cmp(&right.value.len()))
                .then_with(|| left.value.cmp(&right.value))
        });
    }
    matches
        .into_iter()
        .map(|(_, suggestion)| suggestion)
        .collect()
}

fn named_subcommand_suggestions(prefix: &str, span: Span) -> Vec<Suggestion> {
    [
        ("run", "Run named SQL"),
        ("save", "Save named SQL"),
        ("info", "Inspect named SQL"),
        ("list", "List named SQL"),
        ("status", "Show named SQL storage"),
        ("delete", "Delete named SQL"),
    ]
    .into_iter()
    .filter(|(name, _)| crate::fuzzy::find(name, prefix).is_some())
    .map(|(name, description)| command_suggestion(name, description, span, true))
    .collect()
}

fn named_sql_name_suggestions(
    named_sql: &NamedSqlContext,
    prefix: &str,
    span: Span,
) -> Vec<Suggestion> {
    named_sql
        .completion_names()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| crate::fuzzy::find(name, prefix).is_some())
        .map(|name| command_suggestion(&name, "named SQL", span, true))
        .collect()
}

fn transfer_subcommand_suggestions(command: &str, prefix: &str, span: Span) -> Vec<Suggestion> {
    let subcommands: &[(&str, &str)] = match command {
        "import" => &[("table", "Import CSV rows into a table")],
        "export" => &[
            ("table", "Export a relation to CSV"),
            ("query", "Export a read-only query to CSV"),
        ],
        _ => &[],
    };
    subcommands
        .iter()
        .filter(|(name, _)| crate::fuzzy::find(name, prefix).is_some())
        .map(|(name, description)| command_suggestion(name, description, span, true))
        .collect()
}

fn command_token_span(line: &str, pos: usize) -> Span {
    let start = line[..pos]
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    Span::new(start, pos)
}

fn command_suggestion(
    value: &str,
    description: &str,
    span: Span,
    append_whitespace: bool,
) -> Suggestion {
    Suggestion {
        value: value.to_owned(),
        display_override: None,
        description: Some(description.to_owned()),
        style: Some(Style::new().fg(Color::Purple)),
        extra: None,
        span,
        append_whitespace,
        match_indices: None,
    }
}

fn flag_suggestions(
    command: &str,
    subcommand: Option<&str>,
    prefix: &str,
    span: Span,
) -> Vec<Suggestion> {
    let flags = match (command, subcommand) {
        ("describe", _) => DESCRIBE_FLAGS,
        ("source", _) => SOURCE_FLAGS,
        ("import", Some("table")) => IMPORT_FLAGS,
        ("export", Some("table")) => EXPORT_TABLE_FLAGS,
        ("export", Some("query")) => EXPORT_QUERY_FLAGS,
        ("named", Some("list")) => NAMED_LIST_FLAGS,
        (
            "schemas" | "extensions" | "tables" | "views" | "functions" | "types" | "privileges",
            _,
        ) => SYSTEM_FLAG,
        _ => &[],
    };

    flags
        .iter()
        .copied()
        .filter(|flag| crate::fuzzy::find(flag, prefix).is_some())
        .map(|flag| Suggestion {
            value: flag.to_owned(),
            display_override: None,
            description: Some("flag".to_owned()),
            style: Some(Style::new().fg(Color::Cyan)),
            extra: None,
            span,
            append_whitespace: true,
            match_indices: None,
        })
        .collect()
}

fn object_suggestions(prefix: &str, span: Span, catalog: &SharedCatalog) -> Vec<Suggestion> {
    let catalog = catalog.read().expect("completion catalog is not poisoned");
    let schemas = catalog
        .schemas()
        .iter()
        .filter(|schema| crate::fuzzy::find(schema, prefix).is_some())
        .map(|schema| Suggestion {
            value: format!("{}.", quote_identifier(schema)),
            display_override: None,
            description: Some("schema".to_owned()),
            style: Some(Style::new().fg(Color::Cyan)),
            extra: None,
            span,
            append_whitespace: false,
            match_indices: None,
        });
    let tables = catalog
        .tables()
        .iter()
        .filter(|table| crate::fuzzy::find(&table.name, prefix).is_some())
        .map(|table| Suggestion {
            value: quote_identifier(&table.name),
            display_override: None,
            description: Some(format!("relation {}.{}", table.schema, table.kind)),
            style: Some(Style::new().fg(Color::Green)),
            extra: None,
            span,
            append_whitespace: true,
            match_indices: None,
        });

    schemas.chain(tables).collect()
}

fn dedupe_suggestions(suggestions: Vec<Suggestion>) -> Vec<Suggestion> {
    let mut seen = HashSet::new();
    suggestions
        .into_iter()
        .filter(|suggestion| seen.insert(suggestion.value.clone()))
        .collect()
}

fn highlight_command(line: &str) -> StyledText {
    let mut styled = StyledText::new();
    let mut cursor = 0;

    for (idx, token) in line.split_whitespace().enumerate() {
        let start = line[cursor..]
            .find(token)
            .map_or(cursor, |offset| cursor + offset);
        if start > cursor {
            styled.push((Style::new(), line[cursor..start].to_owned()));
        }
        let style = if idx == 0 {
            Style::new().bold().fg(Color::Purple)
        } else if token.starts_with('-') {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new()
        };
        styled.push((style, token.to_owned()));
        cursor = start + token.len();
    }

    if cursor < line.len() {
        styled.push((Style::new(), line[cursor..].to_owned()));
    }

    styled
}

const SCHEMAS_SQL: &str = r#"
select n.nspname as schema,
       pg_catalog.pg_get_userbyid(n.nspowner) as owner
from pg_catalog.pg_namespace n
where ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
))
  and ($2 = '' or n.nspname ilike '%' || $2 || '%')
order by case when array_position(current_schemas(false), n.nspname) is null then 1 else 0 end,
         array_position(current_schemas(false), n.nspname),
         n.nspname
"#;

const DATABASES_SQL: &str = r#"
select d.datname as database,
       pg_catalog.pg_get_userbyid(d.datdba) as owner,
       pg_catalog.pg_encoding_to_char(d.encoding) as encoding,
       case
           when d.datallowconn and pg_catalog.has_database_privilege(d.oid, 'CONNECT')
           then pg_catalog.pg_size_pretty(pg_catalog.pg_database_size(d.datname))
           else ''
       end as size
from pg_catalog.pg_database d
where ($1::bool is not null)
  and ($2 = '' or d.datname ilike '%' || $2 || '%')
order by d.datname
"#;

const ROLES_SQL: &str = r#"
select r.rolname as role,
       concat_ws(', ',
           case when r.rolsuper then 'superuser' end,
           case when r.rolinherit then 'inherit' end,
           case when r.rolcreaterole then 'create role' end,
           case when r.rolcreatedb then 'create db' end,
           case when r.rolcanlogin then 'login' end,
           case when r.rolreplication then 'replication' end,
           case when r.rolbypassrls then 'bypass rls' end
       ) as attributes
from pg_catalog.pg_roles r
where ($1::bool is not null)
  and ($2 = '' or r.rolname ilike '%' || $2 || '%')
order by r.rolname
"#;

const EXTENSIONS_SQL: &str = r#"
select e.extname as extension,
       e.extversion as version,
       n.nspname as schema,
       coalesce(pg_catalog.obj_description(e.oid, 'pg_extension'), '') as description
from pg_catalog.pg_extension e
join pg_catalog.pg_namespace n on n.oid = e.extnamespace
where ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
))
  and ($2 = '' or e.extname ilike '%' || $2 || '%' or n.nspname || '.' || e.extname ilike '%' || $2 || '%')
order by n.nspname, e.extname
"#;

const TABLES_SQL: &str = r#"
select n.nspname as schema,
       c.relname as name,
       case c.relkind when 'r' then 'table' when 'p' then 'partitioned table' when 'f' then 'foreign table' end as kind,
       pg_catalog.pg_get_userbyid(c.relowner) as owner,
       case when c.reltuples >= 0 then c.reltuples::bigint::text else '' end as estimated_rows,
       pg_catalog.pg_size_pretty(pg_catalog.pg_total_relation_size(c.oid)) as size
from pg_catalog.pg_class c
join pg_catalog.pg_namespace n on n.oid = c.relnamespace
where c.relkind in ('r', 'p', 'f')
  and ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
  ))
  and ($2 = '' or c.relname ilike '%' || $2 || '%' or n.nspname || '.' || c.relname ilike '%' || $2 || '%')
order by case when array_position(current_schemas(false), n.nspname) is null then 1 else 0 end,
         array_position(current_schemas(false), n.nspname),
         n.nspname,
         c.relname
"#;

const VIEWS_SQL: &str = r#"
select n.nspname as schema,
       c.relname as name,
       case c.relkind when 'v' then 'view' when 'm' then 'materialized view' end as kind,
       pg_catalog.pg_get_userbyid(c.relowner) as owner
from pg_catalog.pg_class c
join pg_catalog.pg_namespace n on n.oid = c.relnamespace
where c.relkind in ('v', 'm')
  and ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
  ))
  and ($2 = '' or c.relname ilike '%' || $2 || '%' or n.nspname || '.' || c.relname ilike '%' || $2 || '%')
order by case when array_position(current_schemas(false), n.nspname) is null then 1 else 0 end,
         array_position(current_schemas(false), n.nspname),
         n.nspname,
         c.relname
"#;

const FUNCTIONS_SQL: &str = r#"
select n.nspname as schema,
       p.proname as name,
       pg_catalog.pg_get_function_identity_arguments(p.oid) as arguments,
       pg_catalog.pg_get_function_result(p.oid) as returns,
       l.lanname as language,
       case p.prokind when 'f' then 'function' when 'p' then 'procedure' when 'a' then 'aggregate' when 'w' then 'window function' end as kind
from pg_catalog.pg_proc p
join pg_catalog.pg_namespace n on n.oid = p.pronamespace
join pg_catalog.pg_language l on l.oid = p.prolang
where p.prokind in ('f', 'p')
  and ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
  ))
  and ($2 = '' or p.proname ilike '%' || $2 || '%' or n.nspname || '.' || p.proname ilike '%' || $2 || '%')
order by case when array_position(current_schemas(false), n.nspname) is null then 1 else 0 end,
         array_position(current_schemas(false), n.nspname),
         n.nspname,
         p.proname,
         pg_catalog.pg_get_function_identity_arguments(p.oid)
"#;

const TYPES_SQL: &str = r#"
select n.nspname as schema,
       t.typname as name,
       case t.typtype when 'b' then 'base' when 'c' then 'composite' when 'd' then 'domain' when 'e' then 'enum' when 'm' then 'multirange' when 'r' then 'range' end as kind,
       pg_catalog.pg_get_userbyid(t.typowner) as owner
from pg_catalog.pg_type t
join pg_catalog.pg_namespace n on n.oid = t.typnamespace
left join pg_catalog.pg_class tc on tc.oid = t.typrelid
where t.typtype in ('b', 'c', 'd', 'e', 'm', 'r')
  and not exists (select 1 from pg_catalog.pg_type element where element.typarray = t.oid)
  and (t.typtype <> 'c' or tc.relkind = 'c')
  and ($1 or (
    n.nspname <> 'pg_catalog'
    and n.nspname <> 'information_schema'
    and n.nspname !~ '^pg_toast'
    and n.nspname !~ '^pg_temp_'
  ))
  and ($2 = '' or t.typname ilike '%' || $2 || '%' or n.nspname || '.' || t.typname ilike '%' || $2 || '%')
order by case when array_position(current_schemas(false), n.nspname) is null then 1 else 0 end,
         array_position(current_schemas(false), n.nspname),
         n.nspname,
         t.typname
"#;

const PRIVILEGES_SQL: &str = r#"
with privileges as (
    select n.nspname as schema,
           c.relname as object,
           case c.relkind when 'r' then 'table' when 'p' then 'partitioned table' when 'v' then 'view' when 'm' then 'materialized view' when 'S' then 'sequence' when 'f' then 'foreign table' end as kind,
           pg_catalog.pg_get_userbyid(c.relowner) as owner,
           acl.grantee,
           acl.grantor,
           acl.privilege_type,
           acl.is_grantable
    from pg_catalog.pg_class c
    join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    join lateral pg_catalog.aclexplode(c.relacl) acl on true
    where c.relkind in ('r', 'p', 'v', 'm', 'S', 'f')
    union all
    select n.nspname as schema,
           p.proname as object,
           case p.prokind when 'f' then 'function' when 'p' then 'procedure' end as kind,
           pg_catalog.pg_get_userbyid(p.proowner) as owner,
           acl.grantee,
           acl.grantor,
           acl.privilege_type,
           acl.is_grantable
    from pg_catalog.pg_proc p
    join pg_catalog.pg_namespace n on n.oid = p.pronamespace
    join lateral pg_catalog.aclexplode(p.proacl) acl on true
    where p.prokind in ('f', 'p')
    union all
    select n.nspname as schema,
           t.typname as object,
           'type' as kind,
           pg_catalog.pg_get_userbyid(t.typowner) as owner,
           acl.grantee,
           acl.grantor,
           acl.privilege_type,
           acl.is_grantable
    from pg_catalog.pg_type t
    join pg_catalog.pg_namespace n on n.oid = t.typnamespace
    join lateral pg_catalog.aclexplode(t.typacl) acl on true
    union all
    select n.nspname as schema,
           n.nspname as object,
           'schema' as kind,
           pg_catalog.pg_get_userbyid(n.nspowner) as owner,
           acl.grantee,
           acl.grantor,
           acl.privilege_type,
           acl.is_grantable
    from pg_catalog.pg_namespace n
    join lateral pg_catalog.aclexplode(n.nspacl) acl on true
)
select p.schema,
       p.object,
       p.kind,
       p.owner,
       case when p.grantee = 0 then 'PUBLIC' else grantee.rolname end as grantee,
       string_agg(p.privilege_type, ', ' order by p.privilege_type) as privileges,
       case when bool_or(p.is_grantable) then 'yes' else 'no' end as grant_option
from privileges p
left join pg_catalog.pg_roles grantee on grantee.oid = p.grantee
where ($1 or (
    p.schema <> 'pg_catalog'
    and p.schema <> 'information_schema'
    and p.schema !~ '^pg_toast'
    and p.schema !~ '^pg_temp_'
))
  and ($2 = ''
    or p.object ilike '%' || $2 || '%'
    or p.schema || '.' || p.object ilike '%' || $2 || '%'
    or (case when p.grantee = 0 then 'PUBLIC' else grantee.rolname::text end) ilike '%' || $2 || '%')
group by p.schema, p.object, p.kind, p.owner, p.grantee, grantee.rolname
order by p.schema, p.object, p.kind, grantee
"#;

const CANDIDATES_SQL: &str = r#"
with input as (
    select $1::bool as include_system,
           $2::text as wanted_schema,
           $3::text as wanted_name,
           $4::text as wanted_signature,
           $5::bool as exact,
           $6::text as search,
           $7::bool as include_relations,
           $8::bool as include_functions,
           $9::bool as include_schemas,
           $10::bool as include_types,
           $11::bool as only_tables,
           $12::bool as only_views
)
select * from (
    select 'relation' as category,
           case c.relkind when 'r' then 'table' when 'p' then 'partitioned table' when 'v' then 'view' when 'm' then 'materialized view' when 'S' then 'sequence' when 'f' then 'foreign table' when 'i' then 'index' end as kind,
           n.nspname as schema,
           c.relname as name,
           '' as detail,
           c.oid::int8 as oid
    from pg_catalog.pg_class c
    join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    cross join input i
    where i.include_relations
      and c.relkind in ('r', 'p', 'v', 'm', 'S', 'f', 'i')
      and (not i.only_tables or c.relkind in ('r', 'p', 'f'))
      and (not i.only_views or c.relkind in ('v', 'm'))
      and (i.include_system or (
        n.nspname <> 'pg_catalog'
        and n.nspname <> 'information_schema'
        and n.nspname !~ '^pg_toast'
        and n.nspname !~ '^pg_temp_'
      ))
      and ((i.exact and (i.wanted_schema = '' or n.nspname = i.wanted_schema) and c.relname = i.wanted_name)
        or (not i.exact and (c.relname ilike '%' || i.search || '%' or n.nspname || '.' || c.relname ilike '%' || i.search || '%')))
    union all
    select 'function' as category,
           case p.prokind when 'f' then 'function' when 'p' then 'procedure' end as kind,
           n.nspname as schema,
           p.proname as name,
           pg_catalog.pg_get_function_identity_arguments(p.oid) || ' returns ' || coalesce(pg_catalog.pg_get_function_result(p.oid), '') as detail,
           p.oid::int8 as oid
    from pg_catalog.pg_proc p
    join pg_catalog.pg_namespace n on n.oid = p.pronamespace
    cross join input i
    where i.include_functions
      and p.prokind in ('f', 'p')
      and (i.include_system or (
        n.nspname <> 'pg_catalog'
        and n.nspname <> 'information_schema'
        and n.nspname !~ '^pg_toast'
        and n.nspname !~ '^pg_temp_'
      ))
      and ((i.exact and (i.wanted_schema = '' or n.nspname = i.wanted_schema) and p.proname = i.wanted_name
            and (i.wanted_signature = '' or lower(pg_catalog.oidvectortypes(p.proargtypes)) = lower(i.wanted_signature)))
        or (not i.exact and (p.proname ilike '%' || i.search || '%' or n.nspname || '.' || p.proname ilike '%' || i.search || '%')))
    union all
    select 'schema' as category,
           'schema' as kind,
           '' as schema,
           n.nspname as name,
           '' as detail,
           n.oid::int8 as oid
    from pg_catalog.pg_namespace n
    cross join input i
    where i.include_schemas
      and (i.include_system or (
        n.nspname <> 'pg_catalog'
        and n.nspname <> 'information_schema'
        and n.nspname !~ '^pg_toast'
        and n.nspname !~ '^pg_temp_'
      ))
      and ((i.exact and i.wanted_schema = '' and n.nspname = i.wanted_name)
        or (not i.exact and n.nspname ilike '%' || i.search || '%'))
    union all
    select 'type' as category,
           case t.typtype when 'b' then 'base type' when 'c' then 'composite type' when 'd' then 'domain' when 'e' then 'enum' when 'm' then 'multirange' when 'r' then 'range' end as kind,
           n.nspname as schema,
           t.typname as name,
           '' as detail,
           t.oid::int8 as oid
    from pg_catalog.pg_type t
    join pg_catalog.pg_namespace n on n.oid = t.typnamespace
    left join pg_catalog.pg_class tc on tc.oid = t.typrelid
    cross join input i
    where i.include_types
      and t.typtype in ('b', 'c', 'd', 'e', 'm', 'r')
      and not exists (select 1 from pg_catalog.pg_type element where element.typarray = t.oid)
      and (t.typtype <> 'c' or tc.relkind = 'c')
      and (i.include_system or (
        n.nspname <> 'pg_catalog'
        and n.nspname <> 'information_schema'
        and n.nspname !~ '^pg_toast'
        and n.nspname !~ '^pg_temp_'
      ))
      and ((i.exact and (i.wanted_schema = '' or n.nspname = i.wanted_schema) and t.typname = i.wanted_name)
        or (not i.exact and (t.typname ilike '%' || i.search || '%' or n.nspname || '.' || t.typname ilike '%' || i.search || '%')))
) candidates
order by case when schema = any(current_schemas(false)) then 0 else 1 end,
         schema,
         name,
         kind,
         detail
"#;

const RELATION_DETAIL_SQL: &str = r#"
select n.nspname as schema,
       c.relname as name,
       case c.relkind when 'r' then 'table' when 'p' then 'partitioned table' when 'v' then 'view' when 'm' then 'materialized view' when 'S' then 'sequence' when 'f' then 'foreign table' when 'i' then 'index' end as kind,
       pg_catalog.pg_get_userbyid(c.relowner) as owner,
       case when c.reltuples >= 0 then c.reltuples::bigint::text else '' end as estimated_rows,
       pg_catalog.pg_size_pretty(pg_catalog.pg_total_relation_size(c.oid)) as size,
       coalesce(pg_catalog.obj_description(c.oid, 'pg_class'), '') as comment
from pg_catalog.pg_class c
join pg_catalog.pg_namespace n on n.oid = c.relnamespace
where c.oid = $1::oid
"#;

const RELATION_COLUMNS_SQL: &str = r#"
select a.attname as name,
       pg_catalog.format_type(a.atttypid, a.atttypmod) as type,
       case when a.attnotnull then 'no' else 'yes' end as nullable,
       coalesce(pg_catalog.pg_get_expr(d.adbin, d.adrelid), '') as default,
       case a.attidentity when 'a' then 'always' when 'd' then 'by default' else '' end as identity,
       case a.attgenerated when 's' then 'stored' else '' end as generated,
       coalesce(pg_catalog.col_description(a.attrelid, a.attnum), '') as comment
from pg_catalog.pg_attribute a
left join pg_catalog.pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum
where a.attrelid = $1::oid
  and a.attnum > 0
  and not a.attisdropped
order by a.attnum
"#;

const RELATION_INDEXES_SQL: &str = r#"
select ic.relname as name,
       pg_catalog.pg_get_indexdef(i.indexrelid) as definition,
       case when i.indisunique then 'yes' else 'no' end as unique,
       case when i.indisprimary then 'yes' else 'no' end as primary
from pg_catalog.pg_index i
join pg_catalog.pg_class ic on ic.oid = i.indexrelid
where i.indrelid = $1::oid
order by i.indisprimary desc, i.indisunique desc, ic.relname
"#;

const RELATION_CONSTRAINTS_SQL: &str = r#"
select con.conname as name,
       case con.contype when 'c' then 'check' when 'f' then 'foreign key' when 'p' then 'primary key' when 'u' then 'unique' when 'x' then 'exclusion' else con.contype::text end as type,
       pg_catalog.pg_get_constraintdef(con.oid, true) as definition
from pg_catalog.pg_constraint con
where con.conrelid = $1::oid
order by con.contype, con.conname
"#;

const RELATION_PRIVILEGES_SQL: &str = r#"
select case when acl.grantee = 0 then 'PUBLIC' else grantee.rolname end as grantee,
       string_agg(acl.privilege_type, ', ' order by acl.privilege_type) as privileges,
       pg_catalog.pg_get_userbyid(acl.grantor) as grantor,
       case when bool_or(acl.is_grantable) then 'yes' else 'no' end as grant_option
from pg_catalog.pg_class c
join lateral pg_catalog.aclexplode(c.relacl) acl on true
left join pg_catalog.pg_roles grantee on grantee.oid = acl.grantee
where c.oid = $1::oid
group by acl.grantee, grantee.rolname, acl.grantor
order by grantee
"#;

const FUNCTION_DETAIL_SQL: &str = r#"
select n.nspname as schema,
       p.proname as name,
       case p.prokind when 'f' then 'function' when 'p' then 'procedure' end as kind,
       pg_catalog.pg_get_function_identity_arguments(p.oid) as arguments,
       pg_catalog.pg_get_function_result(p.oid) as returns,
       l.lanname as language,
       case p.provolatile when 'i' then 'immutable' when 's' then 'stable' when 'v' then 'volatile' end as volatility,
       case p.proparallel when 's' then 'safe' when 'r' then 'restricted' when 'u' then 'unsafe' end as parallel,
       case when p.prosecdef then 'definer' else 'invoker' end as security,
       pg_catalog.pg_get_userbyid(p.proowner) as owner,
       coalesce(pg_catalog.obj_description(p.oid, 'pg_proc'), '') as comment
from pg_catalog.pg_proc p
join pg_catalog.pg_namespace n on n.oid = p.pronamespace
join pg_catalog.pg_language l on l.oid = p.prolang
where p.oid = $1::oid
"#;

const FUNCTION_ARGUMENTS_SQL: &str = r#"
select pg_catalog.pg_get_function_arguments(p.oid) as arguments
from pg_catalog.pg_proc p
where p.oid = $1::oid
"#;

const FUNCTION_PRIVILEGES_SQL: &str = r#"
select case when acl.grantee = 0 then 'PUBLIC' else grantee.rolname end as grantee,
       string_agg(acl.privilege_type, ', ' order by acl.privilege_type) as privileges,
       pg_catalog.pg_get_userbyid(acl.grantor) as grantor,
       case when bool_or(acl.is_grantable) then 'yes' else 'no' end as grant_option
from pg_catalog.pg_proc p
join lateral pg_catalog.aclexplode(p.proacl) acl on true
left join pg_catalog.pg_roles grantee on grantee.oid = acl.grantee
where p.oid = $1::oid
group by acl.grantee, grantee.rolname, acl.grantor
order by grantee
"#;

const FUNCTION_SOURCE_SQL: &str = r#"
select pg_catalog.pg_get_functiondef(p.oid) as definition
from pg_catalog.pg_proc p
where p.oid = $1::oid
"#;

const VIEW_SOURCE_SQL: &str = r#"
select pg_catalog.pg_get_viewdef(c.oid, true) as definition
from pg_catalog.pg_class c
where c.oid = $1::oid
  and c.relkind in ('v', 'm')
"#;

const SCHEMA_DETAIL_SQL: &str = r#"
select n.nspname as schema,
       pg_catalog.pg_get_userbyid(n.nspowner) as owner,
       coalesce(pg_catalog.obj_description(n.oid, 'pg_namespace'), '') as comment
from pg_catalog.pg_namespace n
where n.nspname = $1
"#;

const SCHEMA_OBJECTS_SQL: &str = r#"
select kind, count(*)::text as count
from (
    select case c.relkind when 'r' then 'tables' when 'p' then 'tables' when 'f' then 'tables' when 'v' then 'views' when 'm' then 'views' when 'S' then 'sequences' else 'relations' end as kind
    from pg_catalog.pg_class c
    join pg_catalog.pg_namespace n on n.oid = c.relnamespace
    where n.nspname = $1
      and c.relkind in ('r', 'p', 'f', 'v', 'm', 'S')
    union all
    select 'functions'
    from pg_catalog.pg_proc p
    join pg_catalog.pg_namespace n on n.oid = p.pronamespace
    where n.nspname = $1
      and p.prokind in ('f', 'p')
    union all
    select 'types'
    from pg_catalog.pg_type t
    join pg_catalog.pg_namespace n on n.oid = t.typnamespace
    left join pg_catalog.pg_class tc on tc.oid = t.typrelid
    where n.nspname = $1
      and t.typtype in ('b', 'c', 'd', 'e', 'm', 'r')
      and not exists (select 1 from pg_catalog.pg_type element where element.typarray = t.oid)
      and (t.typtype <> 'c' or tc.relkind = 'c')
) objects
group by kind
order by kind
"#;

const SCHEMA_PRIVILEGES_SQL: &str = r#"
select case when acl.grantee = 0 then 'PUBLIC' else grantee.rolname end as grantee,
       string_agg(acl.privilege_type, ', ' order by acl.privilege_type) as privileges,
       pg_catalog.pg_get_userbyid(acl.grantor) as grantor,
       case when bool_or(acl.is_grantable) then 'yes' else 'no' end as grant_option
from pg_catalog.pg_namespace n
join lateral pg_catalog.aclexplode(n.nspacl) acl on true
left join pg_catalog.pg_roles grantee on grantee.oid = acl.grantee
where n.nspname = $1
group by acl.grantee, grantee.rolname, acl.grantor
order by grantee
"#;

const TYPE_DETAIL_SQL: &str = r#"
select n.nspname as schema,
       t.typname as name,
       case t.typtype when 'b' then 'base' when 'c' then 'composite' when 'd' then 'domain' when 'e' then 'enum' when 'm' then 'multirange' when 'r' then 'range' end as kind,
       pg_catalog.pg_get_userbyid(t.typowner) as owner,
       coalesce(pg_catalog.obj_description(t.oid, 'pg_type'), '') as comment
from pg_catalog.pg_type t
join pg_catalog.pg_namespace n on n.oid = t.typnamespace
where t.oid = $1::oid
"#;

const TYPE_ENUM_SQL: &str = r#"
select e.enumsortorder::text as position,
       e.enumlabel as value
from pg_catalog.pg_enum e
where e.enumtypid = $1::oid
order by e.enumsortorder
"#;

const TYPE_COMPOSITE_SQL: &str = r#"
select a.attnum::text as position,
       a.attname as name,
       pg_catalog.format_type(a.atttypid, a.atttypmod) as type,
       coalesce(pg_catalog.col_description(a.attrelid, a.attnum), '') as comment
from pg_catalog.pg_type t
join pg_catalog.pg_class c on c.oid = t.typrelid
join pg_catalog.pg_attribute a on a.attrelid = c.oid
where t.oid = $1::oid
  and a.attnum > 0
  and not a.attisdropped
order by a.attnum
"#;

const TYPE_DOMAIN_SQL: &str = r#"
select pg_catalog.format_type(t.typbasetype, t.typtypmod) as base_type,
       case when t.typnotnull then 'no' else 'yes' end as nullable,
       coalesce(t.typdefault, '') as default,
       coalesce(coll.collname, '') as collation
from pg_catalog.pg_type t
left join pg_catalog.pg_collation coll on coll.oid = t.typcollation and t.typcollation <> 0
where t.oid = $1::oid
  and t.typtype = 'd'
"#;

const TYPE_DOMAIN_CONSTRAINTS_SQL: &str = r#"
select con.conname as name,
       pg_catalog.pg_get_constraintdef(con.oid, true) as check
from pg_catalog.pg_constraint con
where con.contypid = $1::oid
order by con.conname
"#;

const TYPE_RANGE_SQL: &str = r#"
select pg_catalog.format_type(r.rngsubtype, null) as subtype,
       coalesce(coll.collname, '') as collation,
       coalesce(canonical.proname, '') as canonical,
       coalesce(diff.proname, '') as subtype_diff
from pg_catalog.pg_range r
left join pg_catalog.pg_collation coll on coll.oid = r.rngcollation and r.rngcollation <> 0
left join pg_catalog.pg_proc canonical on canonical.oid = r.rngcanonical
left join pg_catalog.pg_proc diff on diff.oid = r.rngsubdiff
where r.rngtypid = $1::oid
"#;

const TYPE_PRIVILEGES_SQL: &str = r#"
select case when acl.grantee = 0 then 'PUBLIC' else grantee.rolname end as grantee,
       string_agg(acl.privilege_type, ', ' order by acl.privilege_type) as privileges,
       pg_catalog.pg_get_userbyid(acl.grantor) as grantor,
       case when bool_or(acl.is_grantable) then 'yes' else 'no' end as grant_option
from pg_catalog.pg_type t
join lateral pg_catalog.aclexplode(t.typacl) acl on true
left join pg_catalog.pg_roles grantee on grantee.oid = acl.grantee
where t.oid = $1::oid
group by acl.grantee, grantee.rolname, acl.grantor
order by grantee
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::shared_catalog;

    #[test]
    fn named_status_reports_only_named_sql_storage() {
        // Given
        let cwd = env::temp_dir().join(format!("dbcrab-meta-status-{}", process::id()));
        let base = env::temp_dir().join(format!("dbcrab-meta-data-{}", process::id()));
        let context = NamedSqlContext::load_from_with_base(
            &cwd,
            Some("billing"),
            &crate::config::NamedSqlConfig::default(),
            base.clone(),
        )
        .expect("context should load");

        // When
        let sections = named_status_output(&context);

        // Then
        assert_eq!(sections[0].title, "Named SQL status");
        assert_eq!(
            sections[0].grid,
            ResultGrid::from_records(
                ["field", "value"],
                [
                    ["active context".to_owned(), "billing".to_owned()],
                    [
                        "active root".to_owned(),
                        base.join("billing").display().to_string(),
                    ],
                    [
                        "active root source".to_owned(),
                        "default data directory".to_owned(),
                    ],
                    [
                        "shared root".to_owned(),
                        base.join("shared").display().to_string(),
                    ],
                    [
                        "shared root source".to_owned(),
                        "default data directory".to_owned(),
                    ],
                ],
            )
        );
    }

    #[test]
    fn session_reports_context_history_and_config_sources() {
        // Given
        let cwd = env::temp_dir().join(format!("dbcrab-meta-session-{}", process::id()));
        let base = env::temp_dir().join(format!("dbcrab-meta-data-{}", process::id()));
        let context = NamedSqlContext::load_from_with_base(
            &cwd,
            Some("billing"),
            &crate::config::NamedSqlConfig::default(),
            base,
        )
        .expect("context should load");
        let config_source = ConfigSource::File(PathBuf::from("/etc/dbcrab/config.kdl"));
        let history = context
            .history_path()
            .map_or_else(|| "disabled".to_owned(), |path| path.display().to_string());

        // When
        let session = session_section(&context, &config_source);

        // Then
        assert_eq!(session.title, "DBCrab session");
        assert_eq!(
            session.grid,
            ResultGrid::from_records(
                ["field", "value"],
                [
                    ["context".to_owned(), "billing".to_owned()],
                    ["context source".to_owned(), "--context".to_owned()],
                    ["history".to_owned(), history],
                    [
                        "user config".to_owned(),
                        "/etc/dbcrab/config.kdl".to_owned(),
                    ],
                    ["project config".to_owned(), "none".to_owned()],
                ],
            )
        );
    }

    #[test]
    fn parser_accepts_import_table_options() {
        // Given
        let input = "import table --name public.users --input ./users.csv --no-header";

        // When
        let parsed = parse_command(input).expect("import should parse");

        // Then
        match parsed {
            ParsedCommand::ImportTable { target, options } => {
                assert_eq!(target, "public.users");
                assert_eq!(options.input, PathBuf::from("./users.csv"));
                assert!(!options.header);
                assert!(!options.format_explicit);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parser_accepts_export_query_with_trailing_semicolon() {
        // Given
        let input =
            "export query --sql \"select ';' as separator;\" --output ./separators.csv --force";

        // When
        let parsed = parse_command(input).expect("export should parse");

        // Then
        match parsed {
            ParsedCommand::ExportQuery { query, options } => {
                assert_eq!(query, "select ';' as separator;");
                assert_eq!(options.output, PathBuf::from("./separators.csv"));
                assert!(options.header);
                assert!(options.force);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parser_preserves_named_save_sql_verbatim() {
        // Given
        let input = "named save reports/monthly select ':unfinished;";

        // When
        let parsed = parse_command(input).expect("named save should parse invalid SQL as a draft");

        // Then
        match parsed {
            ParsedCommand::NamedSave { name, sql } => {
                assert_eq!(name, "reports/monthly");
                assert_eq!(sql.as_deref(), Some("select ':unfinished;"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parser_accepts_named_save_without_sql() {
        // Given
        let input = "named save reports/monthly";

        // When
        let parsed = parse_command(input).expect("named save should parse without SQL");

        // Then
        assert!(matches!(
            parsed,
            ParsedCommand::NamedSave { name, sql: None } if name == "reports/monthly"
        ));
    }

    #[test]
    fn editor_selection_falls_back_to_visual() {
        // Given
        let editor = Some("   ".to_owned());
        let visual = Some("code --wait".to_owned());

        // When
        let configured = select_editor(editor, visual).expect("VISUAL should be selected");

        // Then
        assert_eq!(
            configured,
            ConfiguredEditor {
                variable: "VISUAL",
                command: "code --wait".to_owned(),
            }
        );
    }

    #[test]
    fn editor_selection_prefers_editor() {
        // Given
        let editor = Some("nvim".to_owned());
        let visual = Some("code --wait".to_owned());

        // When
        let configured = select_editor(editor, visual).expect("EDITOR should be selected");

        // Then
        assert_eq!(
            configured,
            ConfiguredEditor {
                variable: "EDITOR",
                command: "nvim".to_owned(),
            }
        );
    }

    #[test]
    fn editor_selection_requires_a_configured_command() {
        // Given
        let editor = None;
        let visual = None;

        // When
        let error = select_editor(editor, visual).expect_err("missing editor should fail");

        // Then
        assert_eq!(
            error.to_string(),
            "named save without SQL requires $EDITOR or $VISUAL to be set"
        );
    }

    #[cfg(unix)]
    #[test]
    fn editor_receives_configured_arguments_and_path() {
        // Given
        let editor = ConfiguredEditor {
            variable: "EDITOR",
            command: r#"sh -c 'test "$1" = "/tmp/dbcrab editor path.sql"' dbcrab-test"#.to_owned(),
        };
        let path = Path::new("/tmp/dbcrab editor path.sql");

        // When
        let result = run_editor(&editor, path);

        // Then
        result.expect("editor should receive the path as its final argument");
    }

    #[cfg(unix)]
    #[test]
    fn editor_nonzero_exit_is_an_error() {
        // Given
        let editor = ConfiguredEditor {
            variable: "EDITOR",
            command: "sh -c 'exit 7' dbcrab-test".to_owned(),
        };

        // When
        let error = run_editor(&editor, Path::new("query.sql"))
            .expect_err("non-zero editor exit should fail");

        // Then
        assert!(error.to_string().contains("exit status: 7"));
    }

    #[test]
    fn parser_accepts_short_named_run_alias() {
        // Given
        let input = "run users/by-id id=42 note=\"hello world\" empty=null";

        // When
        let parsed = parse_command(input).expect("named run should parse");

        // Then
        match parsed {
            ParsedCommand::NamedRun { name, values } => {
                assert_eq!(name, "users/by-id");
                assert_eq!(values[0].value, Some("42".to_owned()));
                assert_eq!(values[1].value, Some("hello world".to_owned()));
                assert_eq!(values[2].value, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parser_accepts_named_list_all() {
        // Given
        let input = "named list --all";

        // When
        let parsed = parse_command(input).expect("named list should parse");

        // Then
        assert!(matches!(
            parsed,
            ParsedCommand::NamedList {
                shared: false,
                all: true
            }
        ));
    }

    #[test]
    fn parser_accepts_named_status() {
        // Given
        let input = "named status";

        // When
        let parsed = parse_command(input).expect("named status should parse");

        // Then
        assert!(matches!(parsed, ParsedCommand::NamedStatus));
    }

    #[test]
    fn parser_accepts_session() {
        // Given
        let input = "session";

        // When
        let parsed = parse_command(input).expect("session should parse");

        // Then
        assert!(matches!(parsed, ParsedCommand::Session));
    }

    #[test]
    fn named_command_requires_runtime_configuration() {
        // Given
        let input = "named list";

        // When
        let uses_runtime_config = command_uses_runtime_config(input);

        // Then
        assert!(uses_runtime_config);
    }

    #[test]
    fn session_command_requires_runtime_configuration() {
        // Given
        let input = "session";

        // When
        let uses_runtime_config = command_uses_runtime_config(input);

        // Then
        assert!(uses_runtime_config);
    }

    #[test]
    fn connection_command_does_not_require_runtime_configuration() {
        // Given
        let input = "connection";

        // When
        let uses_runtime_config = command_uses_runtime_config(input);

        // Then
        assert!(!uses_runtime_config);
    }

    #[test]
    fn ordinary_command_does_not_require_runtime_configuration() {
        // Given
        let input = "tables";

        // When
        let uses_runtime_config = command_uses_runtime_config(input);

        // Then
        assert!(!uses_runtime_config);
    }

    #[test]
    fn parser_rejects_non_csv_format() {
        // Given
        let input = "export table --name users --output users.tsv --format tsv";

        // When
        let error = parse_command(input).expect_err("TSV should be rejected");

        // Then
        assert!(error.to_string().contains("invalid value 'tsv'"));
    }

    #[test]
    fn completer_suggests_export_subcommands() -> AppResult<()> {
        // Given
        let catalog = shared_catalog(Catalog::default());
        let base = env::temp_dir().join(format!("dbcrab-meta-completion-{}", process::id()));
        let context = NamedSqlContext::load_from_with_base(
            &base,
            None,
            &crate::config::NamedSqlConfig::default(),
            base.join("data"),
        )?;
        let mut completer = CommandCompleter::new(catalog, context);
        let line = "export q";

        // When
        let (result, ranges) = completer.complete_with_base_ranges(line, line.len());

        // Then
        assert!(!result.is_provisional());
        assert_eq!(result.suggestions().len(), 1);
        assert_eq!(result.suggestions()[0].value, "query");
        assert_eq!(ranges, vec![7..8]);
        Ok(())
    }

    #[test]
    fn completer_suggests_export_query_flags() {
        // Given
        let catalog = shared_catalog(Catalog::default());
        let line = "export query --s";

        // When
        let suggestions = command_suggestions(line, line.len(), &catalog);

        // Then
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].value, "--sql");
    }

    #[test]
    fn completer_suggests_named_subcommands() {
        // Given
        let catalog = shared_catalog(Catalog::default());
        let line = "named i";

        // When
        let suggestions = command_suggestions(line, line.len(), &catalog);

        // Then
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].value, "info");
        assert_eq!(suggestions[1].value, "list");
    }

    #[test]
    fn completer_matches_case_insensitive_flag_abbreviations() {
        // Given
        let catalog = shared_catalog(Catalog::default());
        let line = "export query --SL";

        // When
        let suggestions = command_suggestions(line, line.len(), &catalog);

        // Then
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].value, "--sql");
        assert_eq!(suggestions[0].match_indices, Some(vec![0, 1, 2, 4]));
    }

    #[test]
    fn completer_suggests_named_list_flags() {
        // Given
        let catalog = shared_catalog(Catalog::default());
        let line = "named list --s";

        // When
        let suggestions = command_suggestions(line, line.len(), &catalog);

        // Then
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].value, "--shared");
    }

    #[test]
    fn tokenizer_preserves_raw_quoted_identifier() {
        // Given
        let input = "describe \"Sales Data\".\"Orders\" -t";

        // When
        let tokens = tokenize(input).expect("command should tokenize");

        // Then
        assert_eq!(tokens[1].raw, "\"Sales Data\".\"Orders\"");
        assert_eq!(tokens[1].cooked, "Sales Data.Orders");
    }

    #[test]
    fn parser_uses_raw_target_for_describe() {
        // Given
        let input = "describe \"Sales Data\".\"Orders\" -t";

        // When
        let parsed = parse_command(input).expect("command should parse");

        // Then
        match parsed {
            ParsedCommand::Describe { target, kind, .. } => {
                assert_eq!(target, "\"Sales Data\".\"Orders\"");
                assert_eq!(kind, ObjectKindFilter::Table);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn double_dash_stops_help_parsing() {
        // Given
        let input = "describe -- --help";

        // When
        let parsed = parse_command(input).expect("command should parse");

        // Then
        match parsed {
            ParsedCommand::Describe { target, .. } => assert_eq!(target, "--help"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn object_target_parses_quoted_path() {
        // Given
        let raw = "\"Sales Data\".\"Orders\"";

        // When
        let target = parse_object_target(raw);

        // Then
        assert_eq!(target.schema.as_deref(), Some("Sales Data"));
        assert_eq!(target.name, "Orders");
    }

    #[test]
    fn object_target_lowercases_unquoted_path() {
        // Given
        let raw = "Public.Users";

        // When
        let target = parse_object_target(raw);

        // Then
        assert_eq!(target.schema.as_deref(), Some("public"));
        assert_eq!(target.name, "users");
    }

    #[test]
    fn object_target_parses_function_signature() {
        // Given
        let raw = "auth.login(text, text)";

        // When
        let target = parse_object_target(raw);

        // Then
        assert_eq!(target.schema.as_deref(), Some("auth"));
        assert_eq!(target.name, "login");
        assert_eq!(target.signature.as_deref(), Some("text, text"));
    }
}
