use std::path::PathBuf;

use clap::{Arg, ArgAction, Command};

pub enum Cli {
    Run(Args),
    PrintDefaultKeybindings,
}

pub struct Args {
    pub connection: String,
    pub config: Option<PathBuf>,
}

pub fn parse() -> Cli {
    let matches = Command::new("dbcrab")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A smart PostgreSQL REPL")
        .arg(
            Arg::new("default-keybindings")
                .long("default-keybindings")
                .help("Print the default [keybindings.tui] configuration and exit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .help("Path to a dbcrab config file")
                .value_name("PATH"),
        )
        .arg(
            Arg::new("connection")
                .help("PostgreSQL connection string, for example postgres://user@localhost/db")
                .required_unless_present("default-keybindings")
                .index(1),
        )
        .get_matches();

    if matches.get_flag("default-keybindings") {
        return Cli::PrintDefaultKeybindings;
    }

    let connection = matches
        .get_one::<String>("connection")
        .expect("clap enforces the required connection argument")
        .to_owned();
    let config = matches.get_one::<String>("config").map(PathBuf::from);

    Cli::Run(Args { connection, config })
}
