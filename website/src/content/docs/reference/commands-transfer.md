---
title: Import and Export Commands
description: CSV transfer syntax, defaults, validation, transactions, progress, cancellation, and file replacement.
---

DBCrab 0.6 transfers local UTF-8 CSV files through PostgreSQL `COPY`. Transfer
commands run in command mode and do not use a command-terminating semicolon.

## Syntax

### Import a table

```text
import table --name <TABLE> --input <PATH> [--format csv] [--no-header]
```

| Argument or option | Required | Default | Behavior |
| --- | --- | --- | --- |
| `--name <TABLE>` | yes | none | Target ordinary, partitioned, or foreign table. PostgreSQL resolves unqualified names through `search_path`. |
| `--input <PATH>` | yes | none | Local input file. A leading `~/` is expanded from the platform home variable. |
| `--format csv` | conditional | inferred | Required unless the input extension is `.csv`, compared case-insensitively. `csv` is the only accepted value. |
| `--no-header` | no | header enabled | Treat the first record as data and omit PostgreSQL's CSV `HEADER` option. |

### Export a table or view

```text
export table --name <RELATION> --output <PATH> [--format csv] [--no-header] [--force]
```

| Argument or option | Required | Default | Behavior |
| --- | --- | --- | --- |
| `--name <RELATION>` | yes | none | Source ordinary, partitioned, or foreign table, view, or materialized view. |
| `--output <PATH>` | yes | none | Local destination file. Missing parent directories are created. A leading `~/` is expanded. |
| `--format csv` | conditional | inferred | Required unless the destination extension is `.csv`, compared case-insensitively. |
| `--no-header` | no | header enabled | Omit the CSV header row. |
| `--force` | no | off | Replace an existing destination atomically on Unix. Unsupported on Windows, even when the destination does not yet exist. |

Generated columns are omitted from table exports. If no exportable columns
remain, export fails.

### Export a query

```text
export query --sql <SQL> --output <PATH> [--format csv] [--no-header] [--force]
```

`--output`, `--format`, `--no-header`, and `--force` behave as for table export.
`--sql` is required and must parse as exactly one query statement: `SELECT`,
`WITH`, `VALUES`, or `TABLE`. One trailing semicolon is accepted. Mutation and
multiple statements are rejected before PostgreSQL execution.

Quote SQL containing whitespace for command tokenization:

```text
export query --sql "select * from users where active" --output ./active.csv
```

All transfer commands accept `-h`/`--help`.

## Header validation

Headers are enabled by default.

For import, DBCrab parses the first CSV record before opening `COPY` and requires
it to be valid UTF-8. Header names are matched exactly and case-sensitively to
target columns. The header:

- may select a subset of target columns;
- controls input column order;
- may omit columns that PostgreSQL can fill with defaults or nulls;
- must not contain duplicate names;
- must not name a missing column;
- must not name a generated column;
- must exist; an empty file is rejected as having no header.

With `--no-header`, no column list is generated. PostgreSQL `COPY` consumes data
in the relation's physical column order, so the file must fit that order and the
target must permit writes to every implied column.

PostgreSQL validates all data rows, CSV encoding, types, constraints, and table
permissions. DBCrab always requests CSV format and `UTF8` encoding.

## Transactions and files

Each transfer has its own transaction. DBCrab sets local `DateStyle` to `ISO`
and `IntervalStyle` to `postgres` for stable conversion.

- Import uses a read-write transaction and rolls back every imported row if any
  row, constraint, file read, or `COPY` operation fails.
- Export uses a read-only transaction.
- Export writes a temporary file beside the destination, flushes and syncs it,
  then publishes it. Failure or cancellation removes the temporary file and
  leaves an existing destination unchanged.
- Without `--force`, an existing destination is rejected and publication also
  avoids replacing a file created concurrently.
- Import never modifies its input file.

Interactive imports are allowed. A non-interactive import through `-:` requires
the top-level `--allow-write` flag. Exports do not require that flag.

## Progress and cancellation

Progress is written to standard error, not into CSV or structured command
output. On a terminal it refreshes about once per second; when redirected it
reports about every five seconds. Import can show bytes, total size, percentage,
and rate. Export shows bytes and rate because the total is unknown.

Press `Ctrl-C` to cancel:

- Import aborts `COPY FROM` and rolls back. If PostgreSQL cannot recover the
  copy, DBCrab resets that session connection.
- Export opens a separate connection to call `pg_cancel_backend`, drains the
  copy stream, and rolls back. If cancellation cannot be confirmed, DBCrab
  resets the session connection.

## Result

Successful commands return one `Transfer` row:

| Field | Meaning |
| --- | --- |
| `operation` | `import` or `export`. |
| `source` | Fully quoted `schema.relation`, or `query`. |
| `path` | Resolved local path. |
| `format` | `csv`. |
| `bytes` | Bytes transferred. |
| `rows` | Imported row count; null for exports. |
| `elapsed_ms` | Total elapsed milliseconds. |

## Examples

```text
import table --name users --input ./users.csv
import table --name staging.users --input ./users.data --format csv --no-header

export table --name public.users --output ./exports/users.csv
export table --name reporting.active_users --output ~/exports/active.CSV --force

export query --sql "select id, email from users order by id" --output ./users.csv
export query --sql "with recent as (select * from events limit 100) select * from recent;" --output ./recent.csv --no-header
```
