# 🗃️ DBCrab 🦀

DBCrab is a modern REPL-first PostgreSQL client.

## Screenshots

REPL view:
![DBCrab REPL](docs/assets/repl.png)

TUI view:
![DBCrab TUI](docs/assets/tui.png)

## Quick Start

Installation (tested only on Linux):

- Local: `cargo install --path .`
- [Crates](https://crates.io/crates/dbcrab): `cargo install dbcrab`

Start DBCrab with a PostgreSQL connection string:

```sh
dbcrab postgres://user@localhost/database
```

After connecting, DBCrab loads database metadata for completion and object
inspection. Type SQL at the `>` prompt and end each statement with a semicolon.

```sql
select *
from users
limit 10;
```

## Common Usage

```sh
dbcrab <conn>                         # interactive REPL
dbcrab --agent-guide                  # instructions for coding agents
dbcrab --help                         # all CLI options
```

## Main Features

### Smart SQL REPL

DBCrab is built for interactive database work:

- Write single-line or multi-line SQL statements.
- Run several complete statements from one input.
- Use syntax highlighting for keywords, strings, identifiers, comments, and numbers.
- Keep typing until the statement is complete, DBCrab waits for the final semicolon.
- Use persistent SQL history and reverse history search.
- See clearer PostgreSQL errors, including query positions and helpful hints when possible.

### Autocomplete

DBCrab uses loaded database metadata to suggest useful completions:

- SQL keywords.
- Schema-aware completion: Schemas, tables, views, and other relations.
- Context-aware completion: Columns, including qualified column completion.
- Command names and command flags in command mode.

Use `Tab` or `Ctrl-Space` to open completion suggestions.

### Command Mode

Press `:` on an empty SQL prompt to enter command mode. Commands do not need a
semicolon.

Press `Esc` or `Ctrl-D` to return to the SQL prompt. With `edit_mode = "vi"`,
command mode uses Vi editing too, so `Esc` first leaves insert mode and a second
`Esc` returns to the SQL prompt.

Common commands:

- `help`, show all commands.
- `connection`, show safe connection and session details.
- `refresh`, reload metadata used by autocomplete.
- `schemas`, list schemas.
- `databases`, list databases.
- `roles`, list roles.
- `extensions`, list installed extensions.
- `tables`, list tables, partitioned tables, and foreign tables.
- `views`, list views and materialized views.
- `functions`, list functions and procedures.
- `types`, list PostgreSQL data types.
- `privileges`, list explicit object privileges.
- `describe users`, inspect a database object.
- `source active_users`, show a function, procedure, or view definition.
- `quit`, exit DBCrab.

Most list commands accept a filter, for example `tables user`. Some commands can
include system objects with the `-x` flag, for example `tables pg_catalog -x`.
Use `help describe` or `help source` for command-specific examples.

### Result Display

DBCrab shows small results directly in the terminal. For larger or wider results,
it can open a full-screen table viewer.

Full-screen mode also supports cell preview and, for editable result rows,
staging updates and writing them back.

In the table viewer:

- Move with arrow keys or `h`, `j`, `k`, `l`.
- Press `Enter` to preview the selected cell.
- Press `y` to yank the selected cell to the clipboard.
- Press `Tab` to switch focus between the table and preview.
- Press `q`, `Esc`, or `Ctrl-C` to close the viewer.

Press `Alt-v` in the SQL prompt to cycle result display modes:

- `auto`, let DBCrab choose inline output or the table viewer.
- `full` (TUI), prefer the table viewer for row results.
- `inline`, print results directly in the terminal.

### Editing In TUI View

Some query results are editable in full-screen TUI mode. A cell becomes editable
when DBCrab can trace it to a real table or partitioned table column and the
result also includes that table's primary key columns.

Select an editable cell and press `c` to edit it in the preview pane. Press
`Ctrl-S` to stage the cell change, `Ctrl-X` to stage `NULL` for nullable cells,
or `Esc` to cancel editing. Staged rows are highlighted; press `Ctrl-U` on the
row to write staged changes back to PostgreSQL.

### Agent Mode

DBCrab can run without the interactive REPL for coding agents and scripts. Use
`-e` for one SQL statement and `-:` for one DBCrab command. Run
`dbcrab --agent-guide` to print a compact prompt for coding agents.

Agent SQL execution is read-only by default, uses compact output, returns up to
100 rows, and applies a 10s statement timeout. Use `dbcrab --help` for overrides
when needed.

Suggested prompt for `AGENTS.md` or other coding-agent instructions:

```text
For PostgreSQL work, prefer DBCrab over psql.
Run `dbcrab --agent-guide` before using it.
```

### History And Contexts

DBCrab keeps a persistent SQL history when a state directory is available. By
default, history follows XDG state paths when configured. On Unix-like systems,
it falls back to `$HOME/.local/state/dbcrab/history`. On Windows, it uses
`%LOCALAPPDATA%\dbcrab\history` and falls back to
`%USERPROFILE%\AppData\Local\dbcrab\history`.

Use a named history context when you want separate histories for different
projects or databases:

```sh
dbcrab postgres://user@localhost/app -c my_app
```

### Configuration

DBCrab reads `dbcrab/config.toml` from your user configuration directory: XDG
config paths when configured, `$HOME/.config/dbcrab/config.toml` on Unix-like
systems, or `%APPDATA%\dbcrab\config.toml` on Windows. Use `--config PATH` to
load a specific file.

Start with only the settings you want to change:

```toml
edit_mode = "emacs" # or "vi"

[keybindings.prompt]
complete.add = ["ctrl-y"]      # keep defaults and add ctrl-y
complete.remove = ["ctrl-space"]
cycle_display.set = ["ctrl-v"] # replace all bindings for this action
command_mode = ["ctrl-g"]      # plain assignment is also replacement

[keybindings.remap]
j.swap = "n" # swap j and n in navigation contexts

[keybindings.tui]
quit.add = ["ctrl-q"]
```

Print the full default keybinding template with:

```sh
dbcrab --default-keybindings
```

Keybindings are merged with DBCrab defaults. A plain assignment is the same as
`.set`: it replaces an action's bindings. Use `.add` to append keys, `.remove`
to drop keys, and `.swap` under `[keybindings.remap]` to create a two-way key
remap. The swap is useful for those that have a different layout than QWERTY.
