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

Press `Esc` or `Ctrl-D` to return to the SQL prompt. With `edit-mode vi`,
command mode uses Vi editing too, so `Esc` first leaves insert mode and a second
`Esc` returns to the SQL prompt.

Common commands:

- `help`, show all commands.
- `connection`, show safe connection details.
- `session`, show DBCrab session details.
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
- `export table --name users --output ./users.csv`, export a relation to CSV.
- `import table --name users --input ./users.csv`, import CSV rows into a table.
- `run reports/monthly month=2026-07`, run named SQL.
- `named list`, list named SQL in the active context.
- `quit`, exit DBCrab.

Most list commands accept a filter, for example `tables user`. Some commands can
include system objects with the `-x` flag, for example `tables pg_catalog -x`.
Use `help describe`, `help source`, `help import`, or `help export` for
command-specific examples.

### Named SQL

Named SQL stores reusable `.sql` files and isolates them by context. Use `run`
as the short execution command or `named run` as the fully qualified form:

```text
: named save reports/monthly
: named save users/by-id select * from users where id = :id::uuid;
: run users/by-id id=8b7347d4-7a2d-4be0-86ad-e4cb035afc4c
: named info users/by-id
: named list
: named delete users/by-id
```

Names are extensionless, lowercase paths. DBCrab appends `.sql`, so
`users/by-id` maps to `users/by-id.sql`. `named save` treats everything after
the name as SQL and overwrites an existing entry. Structurally invalid SQL is
saved with validation warnings so drafts remain editable.

In the interactive REPL, omit the SQL to open the target file with `$EDITOR`,
falling back to `$VISUAL`. The configured command may include arguments and
must wait for editing to finish; for example, use `VISUAL="code --wait"` rather
than `EDITOR=code`. Exiting without saving a new file cancels the save.

Parameters use lowercase `:name` markers and `name=value` arguments. Values are
bound through PostgreSQL rather than interpolated into SQL. Add PostgreSQL casts
for non-text values, such as `:limit::integer` or `:id::uuid`. Unquoted `null`
binds SQL `NULL`; quote it to bind the text `null`. Missing, duplicate, and
unexpected arguments are rejected.

A file may contain multiple statements. DBCrab executes all statements in one
transaction, streams each result inline, and finishes with `committed` or
`rolled_back`. Read-only unattended runs roll back after successful execution
and finish with `completed`, matching ordinary agent SQL safety. Explicit
`BEGIN`, `COMMIT`, and other transaction-boundary statements are rejected
because DBCrab owns the transaction.

Use `named list --shared` to list shared entries and `named list --all` to list
both active-context and shared entries. In a named context, shared SQL must be
qualified explicitly, for example `run shared/health-check`. Without a named
context, `health-check` and `shared/health-check` resolve to the same entry.

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

```
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

Use a named context when you want separate history and named SQL for different
projects or databases:

```sh
dbcrab postgres://user@localhost/app -c my_app
```

When no context is selected, the logical context is `shared`; it retains the
existing default history filename. Named SQL follows the platform data path.
It uses `$XDG_DATA_HOME/dbcrab/named-sql` when configured, falls back to
`$HOME/.local/share/dbcrab/named-sql` on Unix-like systems, and uses the local
application-data directory on Windows. Its default scope layout uses one
top-level directory per scope: `shared/` and `<context>/`.

Projects can define a strict `dbcrab.kdl` in the current directory. If absent,
DBCrab checks the Git root. A current-directory file wins rather than merging:

```kdl
context "billing"
named-sql-path "./sql"
```

The path resolves relative to `dbcrab.kdl` and points directly to that
context's query root. `--context` overrides the project context; when it selects
a different name, the project path is ignored and the default context path is
used.

### Configuration

DBCrab reads `dbcrab/config.kdl` from your user configuration directory: XDG
config paths when configured, `$HOME/.config/dbcrab/config.kdl` on Unix-like
systems, or `%APPDATA%\dbcrab\config.kdl` on Windows. Use `--config PATH` to
load a specific KDL file. TOML configuration files are not supported.

Start with only the settings you want to change:

```kdl
/- kdl-version 2

edit-mode emacs // or vi

named-sql {
    // shared-path "~/my-shared-sql"
}

keybindings {
    editor {
        // Applied to both Emacs and Vi insert mode.
        emacs-vi-insert {
            clear-screen ctrl-l
        }

        vi-normal {
            undo u
        }
    }

    prompt {
        complete do=add ctrl-y
        complete do=remove ctrl-space
        cycle-display ctrl-v
        command-mode ctrl-g
    }

    vi-remap modes=normal,visual {
        j do=swap n
    }

    shortcut-nav-remap {
        ctrl-j do=swap ctrl-n
    }

    command {
        clear-screen ctrl-l
    }

    tui {
        quit do=add ctrl-q
    }
}
```

Print the full default configuration with:

```sh
dbcrab --default-config
```

Keybindings are merged with DBCrab defaults. Omitting `do` means `do=set`, which
replaces an action's bindings. Use `do=add` to append keys and `do=remove` to
drop keys. Repeated action nodes are applied in document order.

`vi-remap` applies keys without Ctrl or Alt, optionally with Shift, only in the
selected non-insert Vi modes. Its required `modes` property accepts `normal`,
`visual`, or `normal,visual`.
`shortcut-nav-remap` applies globally and requires Ctrl or Alt on every source
key, optionally with Shift. Use `do=swap` in either group for a two-way remap;
both sides must satisfy that group's source modifier rule.

The configuration schema is available at [`config.schema.kdl`](config.schema.kdl)
or from the installed binary:

```sh
dbcrab --config-schema
```
