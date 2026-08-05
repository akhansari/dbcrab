---
title: Named SQL
description: Command syntax, naming, storage, scope resolution, parameters, validation, and transactional execution for reusable SQL.
---

Named SQL maps stable logical names to reusable `.sql` files. DBCrab analyzes
each file, binds named parameters, and executes all of its statements in one
DBCrab-managed transaction. Named SQL commands run in command mode and normally
do not use semicolons. SQL supplied to `named save`, and SQL files run by `run`,
may contain semicolons.

## Command syntax

| Command | Exact syntax | Options and result |
| --- | --- | --- |
| `run` | `run <NAME> [name=value ...]` | Short alias for `named run`. Prepares and executes the named SQL. |
| `named run` | `named run <NAME> [name=value ...]` | Fully qualified run form. |
| `named save` | `named save <NAME> [SQL]` | Save inline SQL, or open the target file in an external editor when SQL is omitted. |
| `named info` | `named info <NAME>` | Show resolved name, scope, path, parameters, statement count, validity diagnostics, and full SQL. |
| `named list` | `named list [--shared|--all]` | List names, parameter names, and validity. The two flags conflict. |
| `named status` | `named status` | Show active context/root and shared root, including each root's source. |
| `named delete` | `named delete <NAME>` | Delete one named-SQL file and prune empty parent directories below its root. |

Before `--`, `-h` or `--help` returns help for the first command word: `run` or
`named`. For `named save`, use `named save --help`; after `<NAME>`, all remaining
text, including `-h` or `--help`, is SQL to save.

## Names and files

A logical name is an extensionless slash-separated path:

```text
health                    -> health.sql
users/by-id               -> users/by-id.sql
reports/monthly.revenue   -> reports/monthly.revenue.sql
```

Each segment must:

- be non-empty and not `.` or `..`;
- not begin with `-`;
- contain only lowercase ASCII letters, digits, `.`, `_`, and `-`.

Names must not end in `.sql`; DBCrab adds the extension. The first segment
`shared` is reserved as a scope prefix. The same segment rules apply to context
names, and the context name itself may not be `shared`.

DBCrab recursively lists files whose extension is exactly `.sql`. A file with
an invalid logical path remains visible in `named list` with a diagnostic.

## Contexts and scopes

There are two roots:

- **Active/local root:** the selected named context's files.
- **Shared root:** files intended for explicit cross-context use.

Resolution is exact and does not fall back from a named context to shared SQL:

| Active context | Requested name | Resolution |
| --- | --- | --- |
| `billing` | `reports/monthly` | Active root: `billing/reports/monthly.sql`, or the project-configured active root. |
| `billing` | `shared/health` | Shared root: `shared/health.sql`. |
| no named context | `health` | Shared root: `shared/health.sql`. |
| no named context | `shared/health` | The same shared file. Displayed names omit the redundant prefix in this context. |

Context precedence is:

1. CLI `--context <NAME>`.
2. `context` in a discovered project `dbcrab.kdl`.
3. No named context, displayed as `shared`.

The project `named-sql-path` is used only when its project context is selected.
A different CLI context uses the default data root. See
[Paths and environment](../paths-environment/#project-configuration).

When a named context is active, its active and shared roots may not be the same
path or contain one another. Existing paths are canonicalized for this overlap
check; missing tails are resolved against their nearest existing parent. With
no named context, the active root intentionally is the shared root.

### Listing scopes

| Form | Named context active | No named context (`shared`) |
| --- | --- | --- |
| `named list` | Active context only. | Shared entries. |
| `named list --shared` | Shared entries, displayed with `shared/` prefixes. | Shared entries. |
| `named list --all` | Active context and shared entries. | Shared entries once. |

Listing is recursive, sorted by displayed name, and includes invalid `.sql`
files with diagnostics rather than hiding them.

## Storage behavior

`users/by-id` appends directories and the `.sql` extension below its resolved
root. Save creates missing parent directories. New regular files are written
through a temporary file and renamed; on Windows, overwriting an existing
regular file uses a direct write. Existing entries are overwritten without a
prompt.

With inline SQL, everything after `<NAME>` is treated as SQL rather than command
syntax. Quotes and semicolons are preserved:

```text
named save health select 1
named save users/by-id select * from users where id = :id::uuid;
```

Invalid or incomplete SQL is still saved; the result marks it invalid and adds
`Validation warnings`, allowing drafts to remain editable.

Without inline SQL, interactive DBCrab:

1. Resolves and creates the target's parent directory.
2. Uses non-empty `$EDITOR`, falling back to non-empty `$VISUAL`.
3. Tokenizes that command, appends the target path as its final argument, and
   waits for the editor process.
4. Reports `saved` if the file exists after a successful editor exit. A new file
   that was not created is reported as `cancelled`. A nonzero editor exit is an
   error.

Use a waiting editor command, for example `VISUAL="code --wait"`.
`named save <NAME>` without SQL is unavailable in non-interactive `-:` mode.
`named save <NAME> <SQL>` in that mode requires the top-level `--allow-write`
flag.

File symlinks are supported:

- read follows the file symlink;
- save writes through it and preserves the link;
- delete unlinks it without deleting the target.

Named-SQL roots and directories below them must not be symlinks. This prevents a
logical path from escaping its configured root.

### Delete

Interactive deletion prompts:

```text
Delete named SQL `users/by-id`? [y/N]
```

Only `y` or `yes`, case-insensitively, confirms. Any other input returns a
`cancelled` result. Non-interactive deletion does not prompt and requires the
top-level `--allow-write` flag.

Deleting a file symlink removes the link, not its target. Delete removes
now-empty parent directories but never removes the scope root.

Default and configured root paths are listed in
[Paths and environment](../paths-environment/#named-sql-data).

## Parameters

### SQL markers

Use `:name` in SQL:

```sql
select id, email
from users
where id = :id::uuid
  and created_at >= :since::timestamptz
limit :limit::integer;
```

A parameter name must start with a lowercase ASCII letter or `_`, then contain
only lowercase ASCII letters, digits, or `_`. Uppercase anywhere in a marker is
a validation error. Repeated occurrences use the same supplied value.

DBCrab replaces markers with PostgreSQL positional binds independently in each
statement. Values are never interpolated into SQL. Values are supplied as text
or null, so add PostgreSQL casts where type inference is insufficient.

Native `$1`, `$2`, and similar positional parameters are rejected when they
appear as SQL tokens. Use named markers instead.

### Run arguments

Each expected parameter must be supplied exactly once:

```text
run users/by-id id=8b7347d4-7a2d-4be0-86ad-e4cb035afc4c
run reports/search phrase="hello world" limit=50
```

`name=` binds an empty string. Unquoted, lowercase `null` binds SQL `NULL`.
Quote it to bind the four-character text value:

```text
run examples/nulls sql_null=null text_null="null"
```

Missing, duplicate, and unexpected names are reported together where possible.
Invalid argument names or arguments without `=` are rejected.

## SQL scanning and diagnostics

DBCrab recognizes parameters and statement-ending semicolons only in SQL code.
It ignores marker-like text and semicolons inside:

- standard single-quoted strings, including doubled quotes;
- `E'...'` escape strings, including backslash escapes;
- double-quoted identifiers;
- `--` line comments;
- nested `/* ... */` block comments;
- untagged and valid tagged dollar-quoted strings.

`::` casts and `:=` assignment tokens are not parameter starts. Scanning reports
unterminated strings, identifiers, block comments, and dollar quotes. A file
with no executable statement is invalid.

`named save` preserves invalid SQL and returns warnings. `named info` and
`named list` expose validity. `run` refuses any file with diagnostics before
opening its execution transaction.

## Statements and transactions

A file may contain multiple statements. DBCrab splits on code-level semicolons;
the final statement need not end with one. Comment-only and whitespace-only
fragments are ignored.

Explicit transaction control is invalid because DBCrab owns the transaction.
The rejected leading forms are:

- `ABORT`, `BEGIN`, `COMMIT`, `END`, `RELEASE`, `ROLLBACK`, or `SAVEPOINT`;
- `PREPARE TRANSACTION`, `SET TRANSACTION`, or `START TRANSACTION`.

An ordinary SQL `PREPARE name AS ...` is allowed.

Execution behavior:

1. DBCrab opens one transaction for the complete file.
2. It optionally sets the transaction read-only and applies the unattended
   statement timeout.
3. Statements execute in file order. Each statement's row result or affected
   row count is emitted as it completes.
4. Any failure rolls back all earlier statements in the file.
5. An interactive run commits on complete success. A write-enabled unattended
   run also commits. A default read-only unattended run rolls back on complete
   success by design.

Before a default unattended run connects the transaction, DBCrab rejects a
named file containing a statement whose first keyword is not `SELECT`, `WITH`,
`SHOW`, `VALUES`, `TABLE`, or `EXPLAIN`. `--allow-write` permits mutation.

## Interactive and unattended results

| Mode | Per-statement output | Final success | Failure |
| --- | --- | --- | --- |
| Interactive `run` | Row grids are printed inline; non-row statements print affected rows or `OK`. | Transaction commits; no separate success marker is printed. | PostgreSQL error, followed by `ROLLED BACK` when rollback succeeds or `FAILED` otherwise. |
| Unattended default | Each statement uses the selected `compact` or `column-json` schema and row limit. | Transaction rolls back and final status is `completed`. | Structured error plus `rolled_back` or `failed` status. |
| Unattended `--allow-write` | Same structured per-statement output. | Transaction commits and final status is `committed`. | Structured error plus `rolled_back` or `failed` status. |

The interactive run path does not apply `--statement-timeout`; unattended named
runs default to `10s` and accept the top-level override.

## Non-interactive examples

```sh
# Read-only run: successful work is rolled back and ends with status=completed
dbcrab "$PG_URL" -: 'run shared/health-check'

# A mutating named run commits only with explicit permission
dbcrab "$PG_URL" -: 'run maintenance/claim job_id=42' --allow-write

# Inline save and delete are guarded local-file writes
dbcrab "$PG_URL" -: 'named save health select 1' --allow-write
dbcrab "$PG_URL" -: 'named delete health' --allow-write

# Inspecting and listing do not require write permission
dbcrab "$PG_URL" -: 'named info users/by-id' --format column-json
dbcrab "$PG_URL" -: 'named list --all'
```

In unattended mode, `--config` and `--context` are loaded for `run` and all
`named` commands. See [Display and output](../display-output/#non-interactive-output)
for output and transaction status details.
