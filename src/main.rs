mod catalog;
mod cli;
mod completion;
mod config;
mod connection;
mod errors;
mod highlight;
mod meta;
mod prompt;
mod render;
mod repl;
mod sql;
mod tui;
mod validator;

use errors::AppResult;

fn main() {
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(errors::AppError::from)
        .and_then(|runtime| runtime.block_on(run()));

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let cli = cli::parse();
    let args = match cli {
        cli::Cli::Run(args) => args,
        cli::Cli::PrintDefaultKeybindings => {
            print!("{}", config::DEFAULT_KEYBINDINGS_TOML);
            return Ok(());
        }
    };
    let config = config::load(args.config)?;

    println!("Connecting...");
    let pool = connection::connect(&args.connection).await?;

    println!("Connected. Loading metadata...");
    let catalog = catalog::Catalog::load(&pool).await?;
    println!("Loaded {}.", catalog.summary());
    let catalog = catalog::shared_catalog(catalog);

    repl::run(pool, catalog, config.keybindings.tui, args.history_context).await
}
