use std::path::PathBuf;

use clap::{Arg, ArgAction, Command};

pub enum Cli {
    Run(Args),
    PrintDefaultKeybindings,
}

pub struct Args {
    pub connection: String,
    pub config: Option<PathBuf>,
    pub history_context: Option<String>,
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
        .about("A smart PostgreSQL REPL")
        .arg(
            Arg::new("default-keybindings")
                .long("default-keybindings")
                .help("Print the default keybinding configuration and exit")
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
            Arg::new("connection")
                .help("PostgreSQL connection string, for example postgres://user@localhost/db")
                .required_unless_present("default-keybindings")
                .index(1),
        )
        .get_matches_from(args);

    if matches.get_flag("default-keybindings") {
        return Cli::PrintDefaultKeybindings;
    }

    let connection = matches
        .get_one::<String>("connection")
        .expect("clap enforces the required connection argument")
        .to_owned();
    let config = matches.get_one::<String>("config").map(PathBuf::from);
    let history_context = matches.get_one::<String>("context").cloned();

    Cli::Run(Args {
        connection,
        config,
        history_context,
    })
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
            Cli::PrintDefaultKeybindings => panic!("expected run args"),
        }
    }
}
