use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::errors::{AppError, AppResult};

pub async fn connect(connection: &str) -> AppResult<PgPool> {
    let options = connection.parse::<PgConnectOptions>()?;

    match connect_with(options.clone()).await {
        Ok(pool) => Ok(pool),
        Err(err) if is_password_auth_error(&err) => {
            let password = read_password()?;
            connect_with(options.password(&password))
                .await
                .map_err(Into::into)
        }
        Err(err) => Err(err.into()),
    }
}

async fn connect_with(options: PgConnectOptions) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
}

fn is_password_auth_error(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.as_ref() == "28P01")
}

fn read_password() -> AppResult<String> {
    print!("Password: ");
    io::stdout().flush()?;
    terminal::enable_raw_mode()?;

    let result = read_password_raw();
    let disable_result = terminal::disable_raw_mode();
    println!();

    disable_result?;
    result
}

fn read_password_raw() -> AppResult<String> {
    let mut password = String::new();

    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Enter => return Ok(password),
            KeyCode::Backspace => {
                password.pop();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(AppError::message("password prompt interrupted"));
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(AppError::message("password prompt received EOF"));
            }
            KeyCode::Char(ch) => password.push(ch),
            _ => {}
        }
    }
}
