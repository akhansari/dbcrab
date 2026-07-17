mod agent;
mod catalog;
mod cli;
mod completion;
mod config;
mod connection;
mod errors;
mod highlight;
mod meta;
mod paths;
mod prompt;
mod render;
mod repl;
mod sql;
mod transfer;
mod tui;
mod validator;

use errors::{AppError, AppResult};

fn main() {
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(errors::AppError::from)
        .and_then(|runtime| runtime.block_on(run()));

    match result {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

async fn run() -> AppResult<i32> {
    let cli = cli::parse();
    let args = match cli {
        cli::Cli::Run(args) => args,
        cli::Cli::PrintDefaultKeybindings => {
            print!("{}", config::DEFAULT_KEYBINDINGS_TOML);
            return Ok(0);
        }
        cli::Cli::PrintAgentGuide => {
            print!("{}", agent::agent_guide());
            return Ok(0);
        }
    };
    let mode = args.mode;
    let interactive = matches!(mode, cli::RunMode::Interactive);
    let config = if interactive {
        Some(config::load(args.config)?)
    } else {
        None
    };

    print_status(interactive, "Connecting...");
    let pool = connection::connect(&args.connection).await?;

    match mode {
        cli::RunMode::Interactive => {
            print_status(interactive, "Connected. Loading metadata...");
            let catalog = catalog::Catalog::load(&pool).await?;
            print_status(interactive, &format!("Loaded {}.", catalog.summary()));
            let catalog = catalog::shared_catalog(catalog);
            let config = config.expect("interactive mode loads config");
            repl::run(
                pool,
                catalog,
                config.edit_mode,
                config.keybindings,
                args.history_context,
            )
            .await?;
            Ok(0)
        }
        cli::RunMode::Execute { sql, options } => {
            match agent::execute_sql(&pool, &sql, &options).await {
                Ok(output) => {
                    print!("{}", agent::render_output(&output, &options));
                    Ok(0)
                }
                Err(err) => {
                    print_agent_error_with_lazy_catalog(&pool, &err, Some(&sql), options.format)
                        .await;
                    Ok(1)
                }
            }
        }
        cli::RunMode::Command { command, options } => {
            let catalog = catalog::shared_catalog(catalog::Catalog::default());
            match agent::execute_command(&pool, &catalog, &command, &options).await {
                Ok(output) => {
                    print!("{}", agent::render_output(&output, &options));
                    Ok(0)
                }
                Err(err) => {
                    print_agent_error(&err, Some(&command), &catalog, options.format);
                    Ok(1)
                }
            }
        }
    }
}

async fn print_agent_error_with_lazy_catalog(
    pool: &sqlx::PgPool,
    err: &AppError,
    statement: Option<&str>,
    format: agent::AgentFormat,
) {
    let catalog = match err {
        AppError::Sqlx(err) if errors::sql_error_needs_catalog(err, statement) => {
            catalog::Catalog::load_unattended(pool).await.ok()
        }
        AppError::Sqlx(_) => None,
        AppError::Io(_) | AppError::Message(_) => None,
    };
    print!(
        "{}",
        agent::render_error(err, statement, catalog.as_ref(), format)
    );
}

fn print_status(interactive: bool, message: &str) {
    if interactive {
        println!("{message}");
    }
}

fn print_agent_error(
    err: &AppError,
    statement: Option<&str>,
    catalog: &catalog::SharedCatalog,
    format: agent::AgentFormat,
) {
    let rendered = catalog.read().map_or_else(
        |_| agent::render_error(err, statement, None, format),
        |catalog| agent::render_error(err, statement, Some(&catalog), format),
    );
    print!("{rendered}");
}
