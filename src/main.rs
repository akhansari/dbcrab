mod agent;
mod catalog;
mod cli;
mod completion;
mod config;
mod connection;
mod errors;
mod fuzzy;
mod highlight;
mod meta;
mod named_sql;
mod paths;
mod prompt;
mod render;
mod repl;
mod sql;
mod transfer;
mod tui;
mod validator;

use std::io::{self, Write};

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
        cli::Cli::PrintDefaultConfig => {
            print!("{}", config::default_config());
            return Ok(0);
        }
        cli::Cli::PrintConfigSchema => {
            print!("{}", config::CONFIG_SCHEMA_KDL);
            return Ok(0);
        }
        cli::Cli::PrintAgentGuide => {
            print!("{}", agent::agent_guide());
            return Ok(0);
        }
    };
    let mode = args.mode;
    let interactive = matches!(mode, cli::RunMode::Interactive);
    let needs_runtime_config = match &mode {
        cli::RunMode::Interactive => true,
        cli::RunMode::Command { command, .. } => meta::command_uses_runtime_config(command),
        cli::RunMode::Execute { .. } => false,
    };
    let runtime_config = if !needs_runtime_config {
        None
    } else {
        let config = config::load(args.config)?;
        let named_sql =
            named_sql::NamedSqlContext::load(args.context.as_deref(), &config.settings.named_sql)?;
        Some((config, named_sql))
    };

    print_status(interactive, "Connecting...");
    let pool = connection::connect(&args.connection).await?;

    match mode {
        cli::RunMode::Interactive => {
            let (config, named_sql) = runtime_config.ok_or_else(|| {
                AppError::message("internal error: interactive configuration was not loaded")
            })?;
            print_status(interactive, "Connected. Loading metadata...");
            let catalog = catalog::Catalog::load(&pool).await?;
            print_status(interactive, &format!("Loaded {}.", catalog.summary()));
            let catalog = catalog::shared_catalog(catalog);
            repl::run(
                pool,
                catalog,
                config.settings.edit_mode,
                config.settings.keybindings,
                config.source,
                named_sql,
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
            let named_sql = runtime_config.as_ref().map(|(_, named_sql)| named_sql);
            let config_source = runtime_config.as_ref().map(|(config, _)| &config.source);
            let catalog = catalog::shared_catalog(catalog::Catalog::default());
            match agent::execute_command(
                &pool,
                &catalog,
                named_sql,
                config_source,
                &command,
                &options,
                |output| {
                    print!("{}", agent::render_output(output, &options));
                    let _ = io::stdout().flush();
                },
            )
            .await
            {
                Ok(output) => {
                    print!("{}", agent::render_output(&output, &options));
                    Ok(0)
                }
                Err(failure) => {
                    let statement = failure.statement.as_deref().or(Some(&command));
                    if matches!(&failure.error, AppError::Sqlx(_)) {
                        print_agent_error_with_lazy_catalog(
                            &pool,
                            &failure.error,
                            statement,
                            options.format,
                        )
                        .await;
                    } else {
                        print_agent_error(&failure.error, statement, &catalog, options.format);
                    }
                    if failure.named_run {
                        print!(
                            "{}",
                            agent::render_failure_status(failure.rolled_back, options.format)
                        );
                    }
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
