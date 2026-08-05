---
title: Import and export CSV
description: Stream local CSV files safely between PostgreSQL relations, queries, and DBCrab.
---

The leading `:` in the examples below is the command prompt.

## Export a table

```text
: export table --name users --output ./users.csv
```

DBCrab resolves the relation using PostgreSQL's search path, excludes generated columns, and streams `COPY (SELECT ...) TO STDOUT` to a local temporary file. It publishes the destination only after the read-only transaction and file sync succeed.

Table export supports ordinary, partitioned, and foreign tables, plus views and materialized views.

:::caution
An existing destination is never replaced by default. Add `--force` only when replacement is intended. Forced replacement is not supported on Windows.
:::

## Export a query

Pass the SQL as one quoted command argument:

```text
: export query --sql "select * from orders order by 1 limit 100" --output ./orders.csv
```

The query must parse as exactly one `SELECT`, `WITH`, `VALUES`, or `TABLE` statement. Mutating statements and multiple statements are rejected before `COPY` begins.

## Import into a table

Prepare a CSV whose headers are columns from the example `users` table, then run:

```text
: import table --name users --input ./new-users.csv
```

Import supports ordinary, partitioned, and foreign tables. It uses `COPY ... FROM STDIN` inside one transaction, so any malformed row, type error, constraint failure, or cancellation rolls back the entire import.

With the default header behavior, DBCrab validates the first CSV record before starting `COPY`:

- Header names match target columns exactly and may appear in any order.
- Omitted columns are left to PostgreSQL defaults.
- Duplicate and unknown names are rejected.
- Generated target columns are rejected.
- An empty file, invalid CSV header, non-UTF-8 header, or repeated header fails safely.

:::caution
Use `--no-header` only for a file already arranged in PostgreSQL's table column order. Without names, DBCrab cannot perform the header-to-column safety checks.
:::

## Control format and headers

CSV is currently the only transfer format. DBCrab infers it from a case-insensitive `.csv` extension. For another extension, specify it:

```text
: export table --name users --output ./users.data --format csv
: import table --name users --input ./new-users.data --format csv
```

Exports include a header and imports expect one by default. Add `--no-header` to either command when working with headerless CSV. Transfer uses UTF-8 encoding, ISO date style, and PostgreSQL interval style.

## Monitor or cancel a transfer

Progress is written to standard error: once per second in a terminal and once per five seconds when redirected. Import progress includes bytes, total size, percentage, and rate; export reports bytes and rate. The final result includes operation, source, path, format, bytes, imported row count when available, and elapsed milliseconds.

Press **Ctrl-C** to cancel. DBCrab aborts or cancels PostgreSQL `COPY`, rolls back the transaction, and removes an unpublished export temporary file. If export cancellation cannot recover the PostgreSQL session, that connection is reset rather than reused.

These commands stream local files through the client; they do not use server-side filenames or `COPY PROGRAM`.

For unattended transfers, use `-:`. Export is allowed by default; import is a database write and requires the top-level `--allow-write` flag:

```sh
dbcrab "$PG_URL" -: 'export table --name users --output ./users.csv'
dbcrab "$PG_URL" --allow-write -: 'import table --name users --input ./new-users.csv'
```

See [Use DBCrab with coding agents](../agentic-process/) for machine-readable results and write safeguards, and the [transfer command reference](../../reference/commands-transfer/) for all options.
