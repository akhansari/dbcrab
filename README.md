# 🗃️ DBCrab 🦀

DBCrab is a modern REPL-first PostgreSQL client.

## Quick Start

Installation:

- Local: `cargo install --path .`

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

## Main Features

### Smart SQL REPL

DBCrab is built for interactive database work:

- Write single-line or multi-line SQL statements.
- Run several complete statements from one input.
- Use syntax highlighting for keywords, strings, identifiers, comments, and numbers.
- Keep typing until the statement is complete, DBCrab waits for the final semicolon.
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

In the table viewer:

- Move with arrow keys or `h`, `j`, `k`, `l`.
- Press `Enter` to preview the selected cell.
- Press `Tab` to switch focus between the table and preview.
- Press `q`, `Esc`, or `Ctrl-C` to close the viewer.

Press `Alt-v` in the SQL prompt to cycle result display modes:

- `auto`, let DBCrab choose inline output or the table viewer.
- `full`, prefer the table viewer for row results.
- `inline`, print results directly in the terminal.

### History And Contexts

DBCrab keeps a persistent SQL history when a state directory is available. By
default, history is stored under your user state directory.

Use a named history context when you want separate histories for different
projects or databases:

```sh
dbcrab postgres://user@localhost/app -c my_app
```

### Configuration

DBCrab reads configuration from `dbcrab/config.toml` in your user configuration
directory. Use `--config PATH` to load a specific file.

Print the default keybinding configuration with:

```sh
dbcrab --default-keybindings
```

Choose the prompt editing mode with:

```toml
edit_mode = "emacs" # or "vi"
```

Keybindings are merged with DBCrab and Reedline defaults. A plain assignment
replaces an action's bindings, while `.add`, `.remove`, and `.set` patch the
existing defaults.

```toml
[keybindings.remap]
# Remaps navigation/action input. Plain character swaps are skipped while typing.
j.swap = "n"
# Modified remaps still apply in typing modes.
ctrl-h.swap = "ctrl-i"

[keybindings.prompt]
complete.add = ["ctrl-y"]
complete.remove = ["ctrl-space"]
cycle_display = ["alt-v"]
command_mode = [":"]

[keybindings.prompt.insert]
ClearScreen = ["ctrl-l"]
HistoryMenu = ["ctrl-r"]

[keybindings.prompt.vi_normal]
# Reedline's built-in vi grammar still handles h/j/k/l, w, b, d, c, y, etc.

[keybindings.command]
complete = ["tab", "ctrl-space"]
cancel = ["esc", "ctrl-d"]

[keybindings.tui]
left = ["left", "h"]
up = ["up", "k"]
right = ["right", "l"]
down = ["down", "j"]
edit_preview = ["c"]
stage_preview = ["ctrl-s"]
set_null = ["ctrl-x"]
update_row = ["ctrl-u"]
quit.add = ["q"]
```

Prompt line-editor sections use Reedline action names such as `ClearScreen`,
`MoveToLineStart`, `BackspaceWord`, `Undo`, and `PasteCutBufferBefore`.
Plain character remaps also apply to shifted input in non-typing contexts, so
`j.swap = "n"` maps `j <-> n` and `J <-> N` in TUI and Vi normal mode. While
typing SQL in Emacs/Vi insert mode or entering meta-commands, unmodified and
Shift-only characters are left unchanged. Modified keys such as `ctrl-j` are
only remapped when listed explicitly and still apply in typing modes.
