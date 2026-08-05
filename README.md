# DBCrab

DBCrab is a modern REPL-first PostgreSQL client.

[Documentation](https://akhansari.gitlab.io/dbcrab/) | [crates.io](https://crates.io/crates/dbcrab) | [Releases](https://gitlab.com/akhansari/dbcrab/-/releases)

REPL view:

![DBCrab REPL](docs/assets/repl.png)

Full-screen TUI view:

![DBCrab TUI](docs/assets/tui.png)

## Features

- Multiline SQL editing with highlighting, persistent history, and schema-aware,
  context-aware completion.
- A separate command editor for catalog discovery, object inspection, reusable
  named SQL, and CSV transfer.
- Automatic inline or full-screen result display, including cell preview and
  guarded updates for eligible table-backed results.
- Emacs and Vi editing with configurable editor, prompt, command, and TUI keys.
- Context-isolated history and named SQL for different projects or databases.
- Agentic mode for safe, bounded SQL and command execution by LLM agents.

## Install

### Cargo

```sh
cargo install --locked dbcrab
```

### Prebuilt release

```sh
curl -fsSL https://gitlab.com/akhansari/dbcrab/-/raw/main/install.sh | sh
```

Prebuilt macOS releases are not available yet.
If you can help build and test them,
contributions are welcome: [open an issue](https://gitlab.com/akhansari/dbcrab/-/issues)
or submit a merge request on GitLab.

## Start

Pass a PostgreSQL connection string:

```sh
dbcrab postgres://user@localhost/database
```

Type SQL at the `>` prompt and end each statement with a semicolon.\
Press `:` on an empty prompt to enter DBCrab command mode.

Read the [Quickstart](https://akhansari.gitlab.io/dbcrab/quickstart/)
or browse the [full documentation](https://akhansari.gitlab.io/dbcrab/).
