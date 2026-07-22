use std::path::PathBuf;

use clap::{Arg, ArgAction, Command, value_parser};

use crate::agent::{AgentFormat, AgentOptions, DEFAULT_MAX_ROWS, DEFAULT_STATEMENT_TIMEOUT};

pub enum Cli {
    Run(Args),
    PrintDefaultConfig,
    PrintConfigSchema,
    PrintAgentGuide,
}

pub struct Args {
    pub connection: String,
    pub config: Option<PathBuf>,
    pub history_context: Option<String>,
    pub mode: RunMode,
}

pub enum RunMode {
    Interactive,
    Execute {
        sql: String,
        options: AgentOptions,
    },
    Command {
        command: String,
        options: AgentOptions,
    },
}

pub fn parse() -> Cli {
    parse_from(std::env::args_os())
}

fn parse_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let matches = Command::new("dbcrab")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Modern REPL-first PostgreSQL client.")
        .arg(
            Arg::new("default-config")
                .long("default-config")
                .help("Print the default KDL configuration and exit")
                .conflicts_with_all(["config-schema", "agent-guide", "execute", "command"])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config-schema")
                .long("config-schema")
                .help("Print the KDL configuration schema and exit")
                .conflicts_with_all(["default-config", "agent-guide", "execute", "command"])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("agent-guide")
                .long("agent-guide")
                .help("Print a compact prompt for coding agents and exit")
                .conflicts_with_all(["default-config", "config-schema", "execute", "command"])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .help("Path to a dbcrab config file")
                .value_name("PATH"),
        )
        .arg(
            Arg::new("context")
                .short('c')
                .long("context")
                .help("Use a named context for history, stored as <NAME>.history")
                .value_name("NAME"),
        )
        .arg(
            Arg::new("execute")
                .short('e')
                .long("execute")
                .help("Run one SQL statement non-interactively")
                .value_name("SQL")
                .conflicts_with("command"),
        )
        .arg(
            Arg::new("command")
                .short(':')
                .long("command")
                .help("Run one DBCrab meta-command non-interactively")
                .value_name("COMMAND")
                .conflicts_with("execute"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format for non-interactive agent commands")
                .value_name("FORMAT")
                .value_parser(["compact", "column-json"])
                .default_value("compact"),
        )
        .arg(
            Arg::new("max-rows")
                .long("max-rows")
                .help("Maximum rows returned by non-interactive agent output")
                .value_name("ROWS")
                .value_parser(value_parser!(usize))
                .default_value("100"),
        )
        .arg(
            Arg::new("statement-timeout")
                .long("statement-timeout")
                .help("PostgreSQL statement_timeout for non-interactive SQL")
                .value_name("TIMEOUT")
                .default_value(DEFAULT_STATEMENT_TIMEOUT),
        )
        .arg(
            Arg::new("allow-write")
                .long("allow-write")
                .help("Allow mutating SQL and CSV import in non-interactive modes")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("connection")
                .help("PostgreSQL connection string, for example postgres://user@localhost/db")
                .required_unless_present_any(["default-config", "config-schema", "agent-guide"])
                .index(1),
        )
        .get_matches_from(args);

    if matches.get_flag("default-config") {
        return Cli::PrintDefaultConfig;
    }
    if matches.get_flag("config-schema") {
        return Cli::PrintConfigSchema;
    }
    let format = matches
        .get_one::<String>("format")
        .and_then(|value| AgentFormat::parse(value))
        .unwrap_or(AgentFormat::Compact);

    if matches.get_flag("agent-guide") {
        return Cli::PrintAgentGuide;
    }

    let connection = matches
        .get_one::<String>("connection")
        .expect("clap enforces the required connection argument")
        .to_owned();
    let config = matches.get_one::<String>("config").map(PathBuf::from);
    let history_context = matches.get_one::<String>("context").cloned();
    let agent_options = agent_options(&matches, format);
    let mode = if let Some(sql) = matches.get_one::<String>("execute") {
        RunMode::Execute {
            sql: sql.to_owned(),
            options: agent_options,
        }
    } else if let Some(command) = matches.get_one::<String>("command") {
        RunMode::Command {
            command: command.to_owned(),
            options: agent_options,
        }
    } else {
        RunMode::Interactive
    };

    Cli::Run(Args {
        connection,
        config,
        history_context,
        mode,
    })
}

fn agent_options(matches: &clap::ArgMatches, format: AgentFormat) -> AgentOptions {
    AgentOptions {
        format,
        max_rows: matches
            .get_one::<usize>("max-rows")
            .copied()
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_ROWS),
        read_only: !matches.get_flag("allow-write"),
        statement_timeout: matches
            .get_one::<String>("statement-timeout")
            .cloned()
            .unwrap_or_else(|| DEFAULT_STATEMENT_TIMEOUT.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_short_option_sets_history_context() {
        // Given
        let args = ["dbcrab", "-c", "app", "postgres://localhost/app"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::Run(args) => assert_eq!(args.history_context, Some("app".to_owned())),
            Cli::PrintDefaultConfig | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected run args")
            }
        }
    }

    #[test]
    fn execute_option_sets_agent_execute_mode() {
        // Given
        let args = [
            "dbcrab",
            "postgres://localhost/app",
            "--execute",
            "select 1",
            "--format",
            "column-json",
            "--max-rows",
            "5",
        ];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::Run(args) => match args.mode {
                RunMode::Execute { sql, options } => {
                    assert_eq!(sql, "select 1");
                    assert_eq!(options.format, AgentFormat::ColumnJson);
                    assert_eq!(options.max_rows, 5);
                    assert!(options.read_only);
                }
                RunMode::Interactive | RunMode::Command { .. } => panic!("expected execute mode"),
            },
            Cli::PrintDefaultConfig | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected run args")
            }
        }
    }

    #[test]
    fn execute_short_option_sets_agent_execute_mode() {
        // Given
        let args = ["dbcrab", "postgres://localhost/app", "-e", "select 1"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::Run(args) => match args.mode {
                RunMode::Execute { sql, options } => {
                    assert_eq!(sql, "select 1");
                    assert_eq!(options.format, AgentFormat::Compact);
                }
                RunMode::Interactive | RunMode::Command { .. } => panic!("expected execute mode"),
            },
            Cli::PrintDefaultConfig | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected run args")
            }
        }
    }

    #[test]
    fn command_option_sets_agent_command_mode() {
        // Given
        let args = [
            "dbcrab",
            "postgres://localhost/app",
            "--command",
            "describe users",
        ];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::Run(args) => match args.mode {
                RunMode::Command { command, options } => {
                    assert_eq!(command, "describe users");
                    assert_eq!(options.format, AgentFormat::Compact);
                }
                RunMode::Interactive | RunMode::Execute { .. } => panic!("expected command mode"),
            },
            Cli::PrintDefaultConfig | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected run args")
            }
        }
    }

    #[test]
    fn command_short_option_sets_agent_command_mode() {
        // Given
        let args = ["dbcrab", "postgres://localhost/app", "-:", "describe users"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::Run(args) => match args.mode {
                RunMode::Command { command, options } => {
                    assert_eq!(command, "describe users");
                    assert_eq!(options.format, AgentFormat::Compact);
                }
                RunMode::Interactive | RunMode::Execute { .. } => panic!("expected command mode"),
            },
            Cli::PrintDefaultConfig | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected run args")
            }
        }
    }

    #[test]
    fn agent_guide_option_does_not_require_connection() {
        // Given
        let args = ["dbcrab", "--agent-guide"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::PrintAgentGuide => {}
            Cli::Run(_) | Cli::PrintDefaultConfig | Cli::PrintConfigSchema => {
                panic!("expected agent guide output")
            }
        }
    }

    #[test]
    fn default_config_option_does_not_require_connection() {
        // Given
        let args = ["dbcrab", "--default-config"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::PrintDefaultConfig => {}
            Cli::Run(_) | Cli::PrintConfigSchema | Cli::PrintAgentGuide => {
                panic!("expected default config output")
            }
        }
    }

    #[test]
    fn config_schema_option_does_not_require_connection() {
        // Given
        let args = ["dbcrab", "--config-schema"];

        // When
        let cli = parse_from(args);

        // Then
        match cli {
            Cli::PrintConfigSchema => {}
            Cli::Run(_) | Cli::PrintDefaultConfig | Cli::PrintAgentGuide => {
                panic!("expected config schema output")
            }
        }
    }
}
