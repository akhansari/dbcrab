---
title: Save and run named SQL
description: Build reusable, parameter-bound SQL workflows in shared or context-specific files.
---

In the examples below, the leading `:` is the command prompt.

## Save a query

Save SQL directly after an extensionless name:

```text
: named save reports/user-by-id select * from users where id = :id::bigint;
```

DBCrab writes `reports/user-by-id.sql`. Saving the same name again overwrites it. A name may contain `/`-separated segments made from lowercase ASCII letters, digits, `.`, `_`, and `-`. A segment cannot be empty, `.` or `..`, or start with `-`; omit the `.sql` extension. `shared` is reserved as the shared-scope prefix.

To create or edit a longer file, omit the SQL:

```text
: named save reports/user-by-id
```

DBCrab starts `$EDITOR`, falling back to `$VISUAL`, and waits for it to finish. Commands with arguments are supported, such as `VISUAL="code --wait"`. If a new editor target is never saved, the operation is cancelled.

Structurally invalid SQL is still saved with validation warnings so you can keep a draft. It cannot run until fixed.

## Inspect and run it

Use `info` to review the path, scope, parameters, statement count, validity, and SQL:

```text
: named info reports/user-by-id
: run reports/user-by-id id=42
```

`run` is the short form of `named run`. Parameter values are bound through PostgreSQL, never interpolated into SQL. Add explicit PostgreSQL casts because supplied non-null values begin as text, for example `:id::bigint`, `:limit::integer`, or `:at::timestamptz`.

Parameter markers must start with a lowercase letter or `_` and then use lowercase letters, digits, or `_`. DBCrab ignores marker-like text inside strings, quoted identifiers, comments, and dollar quotes. Native `$1` parameters are not supported in named SQL.

Unquoted `null` binds SQL `NULL`; quote it to bind the text `null`. This complete text-parameter example demonstrates both:

```text
: named save examples/show-value select :value::text as value;
: run examples/show-value value=null
: run examples/show-value value="null"
```

Missing, duplicate, and unexpected `name=value` arguments are rejected.

## Run a multi-statement workflow

Save and run two read-only statements together:

```text
: named save reports/counts select count(*) from users; select count(*) from orders;
: run reports/counts
```

A named SQL file may contain several statements. DBCrab runs all of them in one transaction and prints each result as it finishes. Interactive runs commit only after every statement succeeds; a failure rolls back earlier statements and reports `ROLLED BACK`.

:::caution
Do not put `BEGIN`, `COMMIT`, `ROLLBACK`, savepoints, or other transaction-boundary statements in named SQL. DBCrab owns the transaction and rejects those boundaries during validation.
:::

## List and resolve scopes

Check the active storage roots, then list entries:

```text
: named status
: named list
: named list --shared
: named list --all
```

Without a named context, the active scope is `shared`; `health` and `shared/health` resolve to the same file. Inside a named context, unqualified names resolve only in that context and shared names require the prefix:

```text
: run shared/health
```

`named list` shows the active scope, `--shared` shows only shared entries, and `--all` shows both when a named context is active. Learn how roots are selected in [Contexts and history](../contexts-history/).

## Delete an entry

```text
: named delete reports/user-by-id
```

Interactive deletion asks for `y` or `yes`; the default answer is no. Empty parent directories below the scope root are removed.

File symlinks are supported: saving writes through the link, while deleting removes the link and leaves its target. Directory symlinks below a named SQL root are rejected.

## Run named SQL unattended

Use `-:` with the command text as one shell argument:

```sh
dbcrab "$PG_URL" -: 'run reports/user-by-id id=42'
```

Unattended execution is read-only by default. Every statement must belong to the allowed read-only families, the successful transaction is rolled back, and the final status is `completed`. `--allow-write` permits mutating named SQL and commits a fully successful transaction with status `committed`.

Unattended `named save` must include SQL and requires `--allow-write`; editor-based save is interactive only. Unattended `named delete` also requires `--allow-write` and does not prompt.

See [Use DBCrab with coding agents](../agentic-process/) for the inspect, execute, and refine workflow, and the [named SQL reference](../../reference/named-sql/) for complete validation and resolution rules.
