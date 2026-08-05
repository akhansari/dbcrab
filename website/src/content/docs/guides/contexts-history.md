---
title: Separate projects with contexts
description: Isolate SQL history and named SQL, and select project defaults with dbcrab.kdl.
---

## Choose a context explicitly

Pass `-c` or `--context` when starting DBCrab:

```sh
dbcrab "$PG_URL" -c billing
```

A named context isolates both persistent SQL history and the default named SQL root. Context names use lowercase ASCII letters, digits, `.`, `_`, and `-`; they cannot start with `-`, and `.` and `..` are not valid names. The name `shared` is reserved.

When `-c` is omitted and no project context applies, the logical context is `shared`. It keeps the original default history filename and makes the active named SQL root the shared root.

## Locate persistent state

SQL history keeps up to 1,000 entries. Its path is selected in this order:

1. `$XDG_STATE_HOME/dbcrab/history/`
2. `$HOST_XDG_STATE_HOME/dbcrab/history/`
3. `$HOME/.local/state/dbcrab/history/` on Unix-like systems
4. `%LOCALAPPDATA%\dbcrab\history\` on Windows, falling back through
   `%USERPROFILE%` and then `$HOME/.local/state`

The shared filename is `history`; a named context uses `<context>.history`. History path construction trims the context, preserves ASCII letters, digits, `.`, `_`, and `-`, replaces other characters with `_`, and falls back to `context` if nothing remains. Accepted CLI/project context names are stricter, so normally no replacement is needed.

Only SQL history persists. Command-mode history is kept in memory for the current DBCrab process. Start SQL input with one space to exclude that input from history.

Named SQL uses `$XDG_DATA_HOME/dbcrab/named-sql`, `$HOME/.local/share/dbcrab/named-sql` on Unix-like systems, or the local application-data directory on Windows. The default layout is:

```text
named-sql/
  shared/
  billing/
```

See [Save and run named SQL](../named-sql/) for scope resolution.

## Add a project context

Create a strict `dbcrab.kdl` in the directory where DBCrab is started:

```kdl
context "billing"
named-sql-path "./sql"
```

The file accepts only one `context` and one optional `named-sql-path`. A path requires a context. Relative paths resolve from the directory containing `dbcrab.kdl`; `~` is expanded. The path points directly to that context's query root, not to a parent containing multiple contexts.

DBCrab checks exactly these project locations:

1. `./dbcrab.kdl` in the current directory.
2. If absent, `dbcrab.kdl` at the nearest ancestor marked by `.git`.

A current-directory file wins. The two project files are not merged.

:::note
Project `dbcrab.kdl` is distinct from user `config.kdl`. The project file selects a context and its local named SQL root; user configuration controls editing, shared named SQL, and keybindings. See [Configure DBCrab](../configuration/).
:::

## Override the project

`--context` takes precedence over the project's `context` value:

```sh
dbcrab "$PG_URL" --context incident-review
```

If the CLI context differs from the project context, DBCrab ignores the project's `named-sql-path` and uses the default data-directory root for the CLI context. If the names match, the project path still applies, but `session` reports the context source as `--context`.

From command mode, inspect all resolved sources and paths:

```text
: session
: named status
```

DBCrab rejects local and shared named SQL roots that overlap. See [Paths and environment](../../reference/paths-environment/) for platform path details.
