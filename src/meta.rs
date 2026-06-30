use std::collections::HashSet;

use clap::{Arg, ArgAction, ArgGroup, Command};
use nu_ansi_term::{Color, Style};
use reedline::{Completer, Highlighter, Span, StyledText, Suggestion, ValidationResult, Validator};
use sqlx::{PgPool, Row};

use crate::{
    catalog::{Catalog, SharedCatalog, identifier_matches_prefix, quote_identifier},
    errors::{AppError, AppResult},
    render::ResultGrid,
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
        description: "Show safe connection and session details",
        flags: "-h --help",
        examples: "connection",
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

#[derive(Debug, Clone)]
pub enum CommandOutcome {
    None,
    Exit,
    Output(MetaOutput),
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
}

impl CommandCompleter {
    pub fn new(catalog: SharedCatalog) -> Self {
        Self { catalog }
    }
}

impl Completer for CommandCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        command_suggestions(line, pos, &self.catalog)
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
) -> AppResult<CommandOutcome> {
    execute_with_metadata_mode(input, pool, completion_catalog, MetadataMode::Interactive).await
}

pub async fn execute_unattended(
    input: &str,
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
) -> AppResult<CommandOutcome> {
    execute_with_metadata_mode(input, pool, completion_catalog, MetadataMode::Unattended).await
}

#[derive(Debug, Clone, Copy)]
enum MetadataMode {
    Interactive,
    Unattended,
}

async fn execute_with_metadata_mode(
    input: &str,
    pool: &PgPool,
    completion_catalog: &SharedCatalog,
    metadata_mode: MetadataMode,
) -> AppResult<CommandOutcome> {
    match parse_command(input)? {
        ParsedCommand::None => Ok(CommandOutcome::None),
        ParsedCommand::Help(command) => Ok(output(help_output(command.as_deref()))),
        ParsedCommand::Connection => Ok(output(connection_output(pool).await?)),
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
        ParsedCommand::Quit => Ok(CommandOutcome::Exit),
    }
}

fn output(sections: Vec<MetaSection>) -> CommandOutcome {
    CommandOutcome::Output(MetaOutput { sections })
}

fn parse_command(input: &str) -> AppResult<ParsedCommand> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(ParsedCommand::None);
    }
    if input.contains(';') {
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
        .subcommand(Command::new("quit"))
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
        MetadataMode::Unattended => Catalog::load_unattended(pool).await?,
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

fn command_suggestions(line: &str, pos: usize, catalog: &SharedCatalog) -> Vec<Suggestion> {
    let pos = pos.min(line.len());
    let span = command_token_span(line, pos);
    let prefix = &line[span.start..span.end];
    let command = line.split_whitespace().next().unwrap_or_default();

    let candidates = if span.start == 0 {
        COMMANDS
            .iter()
            .filter(|info| info.name.starts_with(prefix))
            .map(|info| command_suggestion(info.name, info.description, span, true))
            .collect::<Vec<_>>()
    } else if prefix.starts_with('-') {
        flag_suggestions(command, prefix, span)
    } else if matches!(command, "describe" | "source" | "tables" | "views") {
        object_suggestions(prefix, span, catalog)
    } else {
        Vec::new()
    };

    dedupe_suggestions(candidates)
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

fn flag_suggestions(command: &str, prefix: &str, span: Span) -> Vec<Suggestion> {
    let flags = match command {
        "describe" => DESCRIBE_FLAGS,
        "source" => SOURCE_FLAGS,
        "schemas" | "extensions" | "tables" | "views" | "functions" | "types" | "privileges" => {
            SYSTEM_FLAG
        }
        _ => &[],
    };

    flags
        .iter()
        .copied()
        .filter(|flag| flag.starts_with(prefix))
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
        .filter(|schema| identifier_matches_prefix(schema, prefix))
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
        .filter(|table| identifier_matches_prefix(&table.name, prefix))
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
