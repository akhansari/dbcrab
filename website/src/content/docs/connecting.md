---
title: Connecting
description: Supply PostgreSQL connection strings, handle passwords safely, and understand DBCrab's startup metadata loading.
---

DBCrab takes one PostgreSQL connection string as its positional argument:

```text
dbcrab [OPTIONS] <CONNECTION>
```

The complete documented feature set requires PostgreSQL 12 or newer because
catalog inspection and transfer metadata use server catalogs introduced in
PostgreSQL 12.

## Connection-string examples

```sh
# Local server with the default PostgreSQL port
dbcrab 'postgres://app@localhost/app'

# Explicit port and password
dbcrab 'postgres://app:secret@db.example.com:5432/app'

# Require TLS
dbcrab 'postgres://app@db.example.com/app?sslmode=require'
```

Percent-encode reserved characters in URI usernames, passwords, database names,
and query parameters.

:::caution
A password embedded directly in a command can be retained by shell history and
visible to other local processes. Prefer a protected environment variable,
password manager, or DBCrab's interactive password prompt.
:::

```sh
export DATABASE_URL='postgres://app@db.example.com/app?sslmode=require'
dbcrab "$PG_URL"
```

## Password retry

If PostgreSQL rejects authentication with SQLSTATE `28P01`, interactive DBCrab
prompts for a password and retries. Input is hidden. Press **Enter** to submit,
**Backspace** to edit, or **Ctrl-C**/**Ctrl-D** to cancel.

The same retry path is used before noninteractive `--execute` and `--command`
runs. Because the prompt reads terminal input, supply usable credentials before
starting DBCrab from a job without a terminal.

## Startup metadata

After connecting, DBCrab loads schemas, relations, and columns for completion and
object inspection. It excludes PostgreSQL catalog, information-schema, toast,
and temporary schemas from normal completion metadata.

When an interactive database has more than 1,000 relations, DBCrab asks before
loading all columns. Skipping them keeps startup bounded but limits broad column
completion. Run `refresh` from command mode after schema changes or when you want
to reload metadata.

DBCrab uses one pooled PostgreSQL connection and a 10-second pool acquire
timeout.

## Inspect the active connection

Press `:` at an empty SQL prompt, then run:

```text
: connection
```

The result includes the database, user, host and port, server version, backend
PID, SSL state, current schema, and search path without echoing the password.
Use `session` for DBCrab context, history, and configuration paths.

## Diagnose a failure

DBCrab reports PostgreSQL severity, SQLSTATE, message, detail, and hint when the
server supplies them. It adds friendlier guidance for invalid passwords and
missing databases.

See [Troubleshooting](../troubleshooting/) for common connection and completion
problems, or continue to the [Quickstart](../quickstart/).
