# DBCrab

DBCrab is a modern REPL-first PostgreSQL client with auto-completion and syntax highlighting.

## Features

- Connect with a PostgreSQL connection string: `dbcrab postgres://...`.
- Native multiline SQL editing with semicolon-gated execution.
- SQL syntax highlighting in the editor.
- Tab and Ctrl-Space completion menu.
- SQL keyword, schema, table, and column completion.
- Schema-aware completion loads database metadata at startup.
- Context-aware completion should suggest relations after clauses.
- Alias-aware completion should suggest columns for aliases.
- Execute semicolon-separated statements one by one.
- Format PostgreSQL errors with severity, SQLSTATE, message, detail, hint, caret position, and friendly suggestions where possible.
- Render multi-row results as terminal-width-aware tables.
- Wrap wide tables to the terminal width, then cap overly tall cells.
- Render single-row results in expanded display.
- Pretty-print and syntax-highlight `json` and `jsonb` values in expanded display.

## Libs

- Use `clap` for the command line parsing.
  - Docs: <https://docs.rs/clap>
  - Repo: <https://github.com/clap-rs/clap>
- Use `reedline` for the line editor.
  - Docs: <https://docs.rs/reedline>
  - Repo: <https://github.com/nushell/reedline>
- Use `crossterm` for terminal manipulation.
  - Docs: <https://docs.rs/crossterm>
  - Repo: <https://github.com/crossterm-rs/crossterm>
- Use `sqlx` as the SQL toolkit for PostgreSQL.
  - Docs: <https://docs.rs/sqlx>
  - Repo: <https://github.com/transact-rs/sqlx>
- Use `sqlparser` as the lexer and parser for SQL.
  - Docs: <https://docs.rs/sqlparser>
  - Repo: <https://github.com/apache/datafusion-sqlparser-rs>
- Use `tabled` to render SQL outputs.
  - Docs: <https://raw.githubusercontent.com/zhiburt/tabled/refs/heads/master/README.md>
  - Repo: <https://github.com/zhiburt/tabled>

## Coding Standards

- Favor Functional first programming with idiomatic modern Rust.
- Always generate the most optimized, simplified, succinct, and sustainable code possible.
  - Proactively detect too complex or not optimized code and suggest improvements and simplification.
- Use the most adapted design patterns for each situation.
  - Proactively detect bad design patterns or code smells and suggest improvements.
