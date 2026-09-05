---
title: Troubleshooting
description: Diagnose common DBCrab connection, editor, completion, result, configuration, history, transfer, and agent-execution problems.
---

## DBCrab cannot connect

- Confirm the host, port, user, and database in the connection string.
- Add `?sslmode=require` when the server requires TLS.
- Percent-encode reserved characters in URI credentials.
- In interactive mode, let DBCrab prompt after an invalid-password response
  instead of exposing the password in shell history.
- Read the PostgreSQL SQLSTATE, detail, and hint in the error output. DBCrab adds
  guidance for invalid passwords and missing databases.

See [Connecting](../connecting/) for examples and password-prompt behavior.

## The SQL prompt keeps adding lines

Interactive SQL must contain one or more complete semicolon-terminated
statements. Add the final `;` outside strings, identifiers, comments, and
dollar-quoted bodies. In `E'...'` strings, DBCrab's input scanner does not
interpret backslash-escaped quotes; use doubled quotes or dollar quoting around
content that includes semicolons.

## `:` does not enter command mode

Command mode opens only when the SQL buffer is known to be empty. Clear or submit
the current SQL first, then press `:`. Commands do not take a semicolon. In Vi
insert mode, the first **Esc** enters normal mode and the second cancels command
mode.

## Completion is missing or stale

Run `refresh` in command mode after creating, dropping, or altering objects.
DBCrab may have skipped column loading after you declined the startup prompt for
a database with more than 1,000 relations. Reconnect and allow the load if broad
column completion is required.

Completion is context-sensitive. Relation names appear in relation positions;
columns become more specific after a visible table or alias.

## The table viewer does not open

In `auto` mode, DBCrab stays inline when the result fits the terminal. Press
**Alt-V** to select `tui`, then rerun the query. Non-row statements continue to
render status output rather than opening a table.

See [Results and TUI](../guides/results-tui/) and the complete
[display reference](../reference/display-output/).

## A result cell is not editable

An editable cell must originate from a real table or partitioned table, must not
be a primary-key column, and the result must include that table's primary-key
columns. Expressions, views, missing keys, and columns from unsupported relation
kinds are read-only.

See [Edit query results](../guides/editing-results/) before staging changes.

## SQL history is not persistent

Persistence requires an available platform state directory. SQL inputs beginning
with a space are intentionally excluded. Command-mode history is always limited
to the current process. The `session` command shows the resolved history path;
check startup warnings to confirm that DBCrab opened it rather than falling back
to in-memory history.

See [Contexts and history](../guides/contexts-history/) and
[Paths and environment](../reference/paths-environment/).

## Named SQL cannot be found

Run `named status` to inspect the active context and storage roots, then
`named list --all` to include active-context and shared entries. In a named
context, qualify shared entries as `shared/name`. Names are lowercase,
extensionless paths; DBCrab adds `.sql` on disk.

## Configuration does not load

DBCrab accepts KDL user configuration, not TOML. When an invocation needs runtime
configuration, an explicitly supplied missing `--config` path is an error, while
a missing implicit user file uses defaults. `--execute` and most noninteractive
commands skip runtime configuration entirely.
Print known-good material with:

```sh
dbcrab --default-config
dbcrab --config-schema
```

Project `dbcrab.kdl` contains only `context` and `named-sql-path`; it is distinct
from the user configuration. See the [configuration guide](../guides/configuration/).

## CSV transfer fails

- Use a `.csv` path or pass `--format csv` explicitly.
- Import headers must not be duplicate, unknown, or generated columns.
- Use `--no-header` only when file columns already match the table's accepted
  input order.
- Export refuses to overwrite an existing file unless `--force` is present.
- DBCrab rolls back a failed or cancelled import transaction.

See [Import and export CSV](../guides/csv-transfer/) for complete workflows.

## Agent output is incomplete

Noninteractive output defaults to 1000 rendered rows. Check `truncated=true` and
raise `--max-rows` or narrow the SQL. The option limits rendering, not database
work, so use SQL `LIMIT` when the query itself must be bounded. The default
statement timeout is `10s`.

See [Use DBCrab with coding agents](../guides/agentic-process/) and the
[CLI reference](../reference/cli/).

## Report a bug

Search or open an issue in the
[DBCrab GitLab project](https://gitlab.com/akhansari/dbcrab/-/issues). Include:

- `dbcrab --version`.
- Operating system and terminal.
- PostgreSQL server version when relevant.
- Reproduction steps and the smallest SQL or command that demonstrates the
  problem.
- Sanitized error output, including SQLSTATE and hints but excluding credentials
  and private data.
