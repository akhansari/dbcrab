mod catalog;
mod cli;
mod completion;
mod connection;
mod errors;
mod highlight;
mod prompt;
mod render;
mod repl;
mod sql;
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
    let args = cli::parse();

    println!("Connecting...");
    let pool = connection::connect(&args.connection).await?;

    println!("Connected. Loading metadata...");
    let catalog = catalog::Catalog::load(&pool).await?;
    println!("Loaded {}.", catalog.summary());

    repl::run(pool, catalog).await
}
