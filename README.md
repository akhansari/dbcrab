# 🗃️ DBCrab 🦀

DBCrab is a modern REPL-first PostgreSQL client.

## Quick Start

Start DBCrab with a PostgreSQL connection string:

```bash
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

```bash
dbcrab postgres://user@localhost/app -c my_app
```

### Configuration

DBCrab reads configuration from `dbcrab/config.toml` in your user configuration
directory. You can customize table viewer keybindings in the `[keybindings.tui]`
section.
