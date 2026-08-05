---
title: Quickstart
description: Connect to PostgreSQL, run a read-only query, inspect your connection, and explore results in DBCrab's table viewer.
---

This walkthrough starts with read-only SQL that works in any PostgreSQL
database. An optional setup at the end creates `public.users` and
`public.orders` for the examples used in the rest of the guides.

## 1. Connect

Pass your PostgreSQL connection string directly.\
DBCrab asks for the secret if the URI does not include one:

```sh
dbcrab postgres://user@localhost/database
```

## 2. Run a query

Paste this complete input at the `>` prompt:

```sql
with activity (id, customer, total_cents, status, details) as (
    values
        (1, 'alice@example.com', 4990, 'paid',
         '{"items": 2, "priority": false}'::jsonb),
        (2, 'bob@example.com', 1250, 'pending',
         '{"items": 1, "priority": true}'::jsonb),
        (3, 'carol@example.com', 8000, 'paid',
         '{"items": 4, "priority": false}'::jsonb)
)
select *
from activity
order by id;
```

DBCrab keeps accepting lines until the statement is complete. Small multi-row
results use an inline table; a one-row result uses an expanded field/value
layout.

## 3. Inspect the connection

Press `:` on the now-empty SQL prompt to enter command mode, then run these
commands without semicolons:

```text
: connection
: schemas
: tables
```

`connection` shows safe session details without echoing the password. `schemas`
and `tables` show the application objects available in the current database;
an empty `tables` result is valid for a new database.

## 4. Open the table viewer

Press **Enter** on the empty `:` prompt to return to SQL. Press **Alt-V** once
to switch from `auto` to `tui`, press **Up** to recall the query, and press
**Enter** to run it again. Use arrow keys or `h`, `j`, `k`, `l` to move,
**Enter** to preview the JSON cell, and `q` to close the viewer.

Press **Alt-V** repeatedly to cycle through `auto`, `tui`, `inline`, and
`inline-blank`.

## 5. Try database-aware completion

If the database already has application tables, type `select * from` and press
**Tab** or **Ctrl-Space** to complete a relation name. After adding an alias,
completion following `alias.` is scoped to that relation's columns.

Enter command mode and run `describe <name>` to inspect one of the relations
reported by `tables`. Try `views`, `functions`, or `help describe` to explore
further.

## Next steps

- Learn the [SQL REPL and completion](../guides/sql-repl/).
- Explore [command mode](../guides/command-mode/) and [result views](../guides/results-tui/).
- Save reusable queries with [named SQL](../guides/named-sql/).
- Review the complete [CLI reference](../reference/cli/).
