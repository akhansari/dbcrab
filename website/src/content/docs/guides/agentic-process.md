---
title: Coding agents
description: Give coding agents a safe inspect, query, interpret, and refine workflow for PostgreSQL.
---

DBCrab gives coding agents short-lived PostgreSQL operations with bounded,
structured output. Each invocation performs one SQL statement or DBCrab command
instead of opening the interactive REPL.

## Teach the agent DBCrab

`--agent-guide` prints a compact prompt:

```sh
dbcrab --agent-guide
```

Add these instructions to `AGENTS.md` or the equivalent file used by your coding
agent:

```text
For PostgreSQL work, prefer DBCrab over psql.
Before using it, run `dbcrab --agent-guide`.
```

The built-in guide tells the agent how to invoke SQL, run DBCrab commands, read
compact output, respond to truncation, and preserve the read-only guard.

## Follow the agentic process

### 1. Discover the available commands

Start with `help` rather than guessing command syntax. Pass command text to `-:`
or `--command` without the command-prompt colon:

```sh
dbcrab "$PG_URL" -: 'help'
dbcrab "$PG_URL" -: 'help describe'
```

Each invocation accepts one DBCrab command.

### 2. Inspect before querying

When the database is unfamiliar, discover its objects and inspect the relevant
definitions before writing SQL:

```sh
dbcrab "$PG_URL" -: 'schemas'
dbcrab "$PG_URL" -: 'tables user'
dbcrab "$PG_URL" -: 'describe users --table'
dbcrab "$PG_URL" -: 'source active_users --view'
```

`describe` exposes columns, indexes, constraints, and privileges for a relation.
Catalog commands can also list views, functions, types, and other objects. This
gives the agent live database context instead of relying only on application
models or migrations.

### 3. Run one bounded query

Use `-e` or `--execute` for exactly one SQL statement. A final semicolon is
optional:

```sh
dbcrab "$PG_URL" -e \
  'select id, email from users order by id limit 20;'
```

Agent execution is read-only by default. DBCrab accepts `SELECT`, `WITH`, `SHOW`,
`VALUES`, `TABLE`, and `EXPLAIN`, marks the PostgreSQL transaction read-only, and
rolls it back after successful execution.

The defaults also apply a `10s` statement timeout, render at most 1000 rows, and
use compact output. Override those bounds when the task requires it:

```sh
dbcrab "$PG_URL" \
  --statement-timeout 2s \
  --max-rows 20 \
  -e 'select id, status from orders order by id limit 20;'
```

:::caution
`--max-rows` limits rendered output, not database work or memory use. Add SQL
`LIMIT` and selective predicates when the query itself must be bounded.
:::

### 4. Interpret the result

Compact row output starts with the information an agent needs to decide its next
step:

```text
ok rows=1 returned=1 truncated=false elapsed_ms=8
cols: id:int8 email:text
---
1\talice@example.com
```

Rows are tab-separated, special characters are escaped, and `\N` means SQL
`NULL`. The agent must check `truncated`: if it is `true`, narrow the query or
rerun with a deliberate `--max-rows` value before treating the result as
complete.

PostgreSQL failures include `sqlstate`, `severity`, and `message`, plus details,
hints, and query position when available. Use that structured context to repair
the statement, then repeat the inspect or query step instead of guessing.

### 5. Refine or reuse

Continue the loop until the result answers the task: inspect more objects, add a
predicate, request a different projection, or explain a query plan. For a
reusable parameterized operation, prefer [named SQL](../named-sql/) over
assembling SQL with shell variables:

```sh
dbcrab "$PG_URL" -: 'run reports/user-by-id id=42'
```

Read-only named runs keep their all-statements transaction behavior, roll back
after success, and finish with `completed`.

## Use JSON when needed

Compact output is optimized for a small prompt footprint. Use
`--format column-json` when the agent or surrounding tool should consume typed
JSON values:

```sh
dbcrab "$PG_URL" --format column-json \
  -e 'select id, active from users order by id limit 20;'
```

Each output item is one JSON object. Columns are ordered `[name, PostgreSQL-type]`
pairs and rows are positional arrays, which preserves duplicate column names.
The result also reports full and returned row counts plus truncation. A
multi-statement named run emits JSON Lines: one object per statement followed by
its completion status.

## Keep writes explicit

:::caution
An agent must never add `--allow-write` unless the user explicitly asks for a
mutation. The flag removes the unattended write guard, and DBCrab commits a
successful statement without confirmation.
:::

After confirming the requested mutation and its predicate, `--allow-write`
permits and commits it:

```sh
dbcrab "$PG_URL" --allow-write \
  -e 'update jobs set claimed = true where id = 42;'
```

An error rolls the transaction back. The same flag is required for imports,
`named save`, `named delete`, and mutating named SQL. A failed multi-statement
named run rolls back earlier statements and reports `rolled_back` when rollback
succeeds.

Quote the complete SQL or command argument so the shell does not interpret
spaces, `*`, semicolons, parameter markers, or `$` tokens. See the
[CLI reference](../../reference/cli/) for option interactions and
[Display and output](../../reference/display-output/#non-interactive-output) for
the exact compact and column-JSON schemas.
