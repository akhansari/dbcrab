# DBCrab

DBCrab is a modern REPL-first PostgreSQL client.

## Libs

- Use `clap` for the command line parsing.
  - Docs: <https://docs.rs/clap>
  - Repo: <https://github.com/clap-rs/clap>
- Use `reedline` for the line editor.
  - Docs: <https://docs.rs/reedline>
  - Repo: <https://github.com/nushell/reedline>
- Use `sqlx` as the SQL toolkit for PostgreSQL.
  - Docs: <https://docs.rs/sqlx>
  - Repo: <https://github.com/transact-rs/sqlx>
- Use `sqlparser` as the lexer and parser for SQL.
  - Docs: <https://docs.rs/sqlparser>
  - Repo: <https://github.com/apache/datafusion-sqlparser-rs>
- Use `crossterm` for terminal manipulation.
  - Docs: <https://docs.rs/crossterm>
  - Repo: <https://github.com/crossterm-rs/crossterm>
- Use `tabled` to render SQL outputs.
  - Docs: <https://raw.githubusercontent.com/zhiburt/tabled/refs/heads/master/README.md>
  - Repo: <https://github.com/zhiburt/tabled>
- Use `ratatui` for the TUI display mode.
  - Docs: <https://docs.rs/ratatui>
  - Repo: <https://github.com/ratatui/ratatui>

## App features

- Bad input, SQL errors, and connection failures should return actionable errors without crashing the session.

- REPL-first PostgreSQL client behavior
- SQL execution and result rendering
- Optional TUI display mode for richer result exploration
- Schema-aware and context-aware auto-complete and suggestions
- Meta-commands like database discovery and helpers
- Error handling expectations for bad SQL, bad input, and connection failures

## Rust Standards

- Prefer expression-oriented, functional code when it improves readability.
- Write idiomatic modern Rust with clear ownership and minimal to zero cloning.
- Prefer borrowing over cloning, but optimize for clarity before micro-optimizing allocations.
- Avoid `unwrap`, `expect`, and `panic!`.
- Use typed errors in reusable code and add application-level context at the boundaries.
- Preserve error sources and add context at process boundaries, user-facing boundaries, and async task boundaries.

## Testing

- Read the Unit Testing skill before modifying or adding unit tests.
- Gate tests that require a live PostgreSQL instance behind explicit environment setup.

## Ask Before

- Adding runtime dependencies.
- Introducing persisted configuration or changing its format.

## Development Commands

Before finishing code changes, run:

```bash
cargo fmt
cargo clippy
cargo check
cargo test
```
