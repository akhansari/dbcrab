---
title: Inspect PostgreSQL in command mode
description: Use DBCrab commands to discover database objects, inspect definitions, and manage the current session.
---

## Enter and leave command mode

Press `:` while the SQL buffer is empty. The prompt changes from `> ` to `: `; if the SQL buffer contains text, `:` is inserted as SQL instead.

At the `: ` prompt, type a command without a terminating semicolon:

```text
: connection
```

After a command runs, DBCrab remains at the command prompt. Return to SQL in any of these ways:

- Press **Enter** on an empty command.
- Press **Esc** or **Ctrl-D**.
- Run `quit` to exit DBCrab instead.

**Ctrl-C** cancels the current command input and stays in command mode. With Vi editing, the first **Esc** from insert mode enters normal mode; press **Esc** again to return to SQL.

:::note
The leading `:` in transcript examples is the prompt, not part of the command. Commands themselves do not use semicolons. Commands that accept SQL as an argument, such as `export query` or `named save`, may contain semicolons inside that argument.
:::

## Discover commands

Run `help` for the command list and `help <command>` for usage, flags, and examples:

```text
: help
: help describe
```

Press **Tab** or **Ctrl-Space** for completion. Depending on the cursor position, command mode completes command names, subcommands, flags, database objects, and named SQL names. Press **Ctrl-R** to search command history for this DBCrab process.

## Inspect a table

Use this workflow when entering an unfamiliar database:

```text
: schemas
: tables user
: describe users
: privileges users
```

`tables user` filters relation names. `describe users` shows the resolved relation, columns, indexes, constraints, and privileges. Qualify an ambiguous object or constrain its kind:

```text
: describe users --table
```

Most catalog lists hide system objects. Add `-x` or `--system` when you need them:

```text
: tables pg_catalog -x
```

Other useful discovery commands include `databases`, `roles`, `extensions`, `views`, `functions`, `types`, and `source`. When the database contains a view, materialized view, function, or procedure, use `source` to show its definition:

```text
: source active_users --view
```

If the schema changed after connecting, reload the metadata used by SQL and command completion:

```text
: refresh
```

Use `session` to see the active context, history path, user configuration source, and project configuration. Use `connection` for safe connection details.

Continue with [CSV import and export](../csv-transfer/) or [Save and run named SQL](../named-sql/). Start with the [session and catalog command reference](../../reference/commands-session-catalog/) for exact command syntax and links to the other command groups.
