---
title: CLI Reference
description: DBCrab 0.6 command-line syntax, modes, options, defaults, and safety rules.
---

This page defines DBCrab's exact command-line behavior. For the coding-agent
workflow, see [Use DBCrab with coding agents](../../guides/agentic-process/).

## Synopsis

Run DBCrab with a PostgreSQL connection:

```text
dbcrab [OPTIONS] <CONNECTION>
```

Print built-in material without connecting:

```text
dbcrab --default-config
dbcrab --config-schema
dbcrab --agent-guide
dbcrab --help
dbcrab --version
```

`CONNECTION` is a PostgreSQL connection string, for example
`postgres://user@localhost/database`. It is required for interactive,
`--execute`, and `--command` modes. The three standalone print options, help,
and version do not require it.

## Options

| Option | Value | Default | Behavior |
| --- | --- | --- | --- |
| `--default-config` | none | off | Print the complete generated KDL default configuration and exit. |
| `--config-schema` | none | off | Print the embedded KDL configuration schema and exit. |
| `--agent-guide` | none | off | Print a compact usage prompt intended for coding agents and exit. |
| `--config <PATH>` | path | platform user config path | Load this user configuration file when runtime configuration is needed. An explicitly named missing file is an error. |
| `-c <NAME>`, `--context <NAME>` | context name | no named context (`shared`) | Select history and named-SQL context. See [Paths and environment](../paths-environment/) and [Named SQL](../named-sql/). |
| `-e <SQL>`, `--execute <SQL>` | SQL text | none | Run exactly one SQL statement non-interactively. Conflicts with `--command`. A trailing semicolon is optional. |
| `-: <COMMAND>`, `--command <COMMAND>` | DBCrab command text | none | Run exactly one meta-command non-interactively. Conflicts with `--execute`. Do not prefix the value with `:`. |
| `--format <FORMAT>` | `compact` or `column-json` | `compact` | Select non-interactive output encoding. Ignored in the interactive REPL. |
| `--max-rows <ROWS>` | unsigned integer | `100` | Limit rows emitted for each non-interactive result grid. DBCrab still fetches and counts the full result. `0` resolves back to the default of `100`. |
| `--statement-timeout <TIMEOUT>` | PostgreSQL timeout value | `10s` | Set transaction-local PostgreSQL `statement_timeout` for `--execute` and non-interactive named-SQL runs. PostgreSQL validates the value. |
| `--allow-write` | none | off | Permit database mutations, non-interactive imports, and non-interactive named-SQL save/delete operations. |
| `-h`, `--help` | none | off | Print CLI help and exit. |
| `-V`, `--version` | none | off | Print the DBCrab version and exit. |

`--default-config`, `--config-schema`, and `--agent-guide` conflict with one
another and with `--execute` and `--command`. They print embedded text and do
not connect or load configuration.

## Run modes

| Mode | Selection | Configuration | Output and transaction behavior |
| --- | --- | --- | --- |
| Interactive REPL | no `-e` or `-:` | User config, project config, context, named SQL, and history are loaded. | SQL results use interactive display modes. Statements run normally and are not subject to the CLI statement timeout. |
| Execute | `-e <SQL>` | User/project configuration and context are not loaded. | Exactly one statement. Read-only by default; a successful read-only transaction is rolled back, while `--allow-write` permits and commits mutation. |
| Command | `-: <COMMAND>` | Runtime config is loaded only when the first command word is `session`, `run`, or `named`. | One DBCrab command. Output uses `--format` and `--max-rows`. Named runs use `--statement-timeout`. |

Consequently, `--config` and `--context` have no effect with `--execute` or
with commands such as `tables` and `describe`; they apply to the interactive
REPL and to non-interactive `session`, `run`, and `named` commands.

## Non-interactive safety

Without `--allow-write`, `--execute` accepts statements whose first SQL keyword
is `SELECT`, `WITH`, `SHOW`, `VALUES`, `TABLE`, or `EXPLAIN`. DBCrab also sets
the PostgreSQL transaction to read-only and rolls it back after successful
execution.

`--allow-write` has these effects:

- `--execute` may run mutating SQL and commits on success.
- `-: run ...` and `-: named run ...` may run mutating named SQL and commit it.
- `-: import table ...`, `-: named save ...`, and `-: named delete ...` are
  permitted.
- Export remains available without the flag because it reads the database and
  writes a requested local output file.

The option does not bypass PostgreSQL permissions or DBCrab validation.

## Examples

```sh
# Interactive REPL
dbcrab 'postgres://app@localhost/app'

# Interactive REPL with an explicit user config and context
dbcrab --config './config.kdl' --context billing \
  'postgres://app@localhost/app'

# One read-only SQL statement, emitted as one JSON line
dbcrab 'postgres://app@localhost/app' \
  --execute 'select id, email from users order by id' \
  --format column-json --max-rows 25

# One catalog command; command syntax has no leading colon or semicolon
dbcrab 'postgres://app@localhost/app' \
  --command 'tables billing' --format compact

# Mutating SQL requires explicit permission
dbcrab 'postgres://app@localhost/app' \
  --execute 'update jobs set claimed = true where id = 42' \
  --allow-write --statement-timeout 30s
```

See [Display and output](../display-output/) for the exact non-interactive
schemas and [Command reference](../commands-session-catalog/) for meta-command
syntax.
