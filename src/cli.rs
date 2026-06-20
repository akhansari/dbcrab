use clap::{Arg, Command};

pub struct Args {
    pub connection: String,
}

pub fn parse() -> Args {
    let matches = Command::new("dbcrab")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A smart PostgreSQL REPL")
        .arg(
            Arg::new("connection")
                .help("PostgreSQL connection string, for example postgres://user@localhost/db")
                .required(true)
                .index(1),
        )
        .get_matches();

    let connection = matches
        .get_one::<String>("connection")
        .expect("clap enforces the required connection argument")
        .to_owned();

    Args { connection }
}
