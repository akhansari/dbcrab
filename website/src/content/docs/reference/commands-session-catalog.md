---
title: Session and Catalog Commands
description: Exact syntax, filters, flags, and output purpose for DBCrab session and catalog commands.
---

Enter command mode by pressing `:` on an empty SQL prompt. The command prompt
already displays `:`, so type the command without a leading colon. Commands do
not use semicolons.

For non-interactive use, pass the same text to `dbcrab <CONNECTION> -: <COMMAND>`.

## Syntax

| Command | Exact syntax | Purpose |
| --- | --- | --- |
| `help` | `help [command]` | List all commands, or show the usage, description, flags, and examples for one command. Unknown names produce an `Unknown command` result. |
| `connection` | `connection` | Show safe details for the current PostgreSQL connection. |
| `session` | `session` | Show the active DBCrab context and configuration/storage sources. |
| `refresh` | `refresh` | Reload schema, relation, and column metadata used by completion. |
| `schemas` | `schemas [filter] [-x|--system]` | List schemas. |
| `databases` | `databases [filter]` | List databases. This command has no system flag. |
| `roles` | `roles [filter]` | List roles. This command has no system flag. |
| `extensions` | `extensions [filter] [-x|--system]` | List installed extensions. |
| `tables` | `tables [filter] [-x|--system]` | List ordinary, partitioned, and foreign tables. |
| `views` | `views [filter] [-x|--system]` | List views and materialized views. |
| `functions` | `functions [filter] [-x|--system]` | List functions and procedures. Aggregates and window functions are not listed. |
| `types` | `types [filter] [-x|--system]` | List base, composite, domain, enum, multirange, and range types. Array helper types and table row types are excluded. |
| `privileges` | `privileges [filter] [-x|--system]` | List explicit ACL entries on relations, functions/procedures, types, and schemas. |
| `quit` | `quit` | Exit DBCrab. |

Every command accepts `-h` or `--help`. A help token before `--` returns the
top-level help row for that command. For example, `tables --help` is equivalent
to `help tables`.

## Filters

`filter` may contain multiple command tokens; DBCrab joins them with spaces.
Matching is case-insensitive and uses substring matching.

| Command | Fields searched by `filter` |
| --- | --- |
| `schemas` | Schema name. |
| `databases` | Database name. |
| `roles` | Role name. |
| `extensions` | Extension name and `schema.extension`. |
| `tables`, `views`, `functions`, `types` | Object name and `schema.name`. |
| `privileges` | Object name, `schema.object`, and grantee role or `PUBLIC`. |

DBCrab wraps the filter in `%...%`, so you normally pass `user`, not `%user%`.
PostgreSQL `ILIKE` semantics still apply inside it: `%` matches any sequence and
`_` matches one character.

## System objects

Without `-x`/`--system`, commands that support the flag omit objects in:

- `pg_catalog`
- `information_schema`
- schemas beginning with `pg_toast`
- schemas beginning with `pg_temp_`

The flag includes those schemas; it does not limit output to system objects.
`databases` and `roles` always list their full catalog and do not accept `-x`.

## Output fields

| Command | Output purpose and fields |
| --- | --- |
| `help` | `command`, `usage`, `description`, `flags`, and newline-separated `examples`. |
| `connection` | Current `database`, `user`, server `host` and `port`, server version, backend `pid`, SSL state, current `schema`, and `search_path`. It does not print the connection string or password. Local sockets are labeled `local socket`. |
| `session` | `context`, `context source`, resolved history path or `disabled`, user config source, and project config path or `none`. A startup warning indicates if history fell back to memory instead of opening that path. |
| `refresh` | `refreshed` status and a count summary such as `4 schemas, 18 relations, 126 columns`. |
| `schemas` | Schema and owner. Search-path schemas sort first. |
| `databases` | Database, owner, encoding, and size when connections and permissions allow size lookup. |
| `roles` | Role and enabled attributes: superuser, inherit, create role, create DB, login, replication, and bypass RLS. |
| `extensions` | Extension, installed version, schema, and description. |
| `tables` | Schema, name, relation kind, owner, estimated rows, and total relation size. |
| `views` | Schema, name, view kind, and owner. |
| `functions` | Schema, name, identity arguments, return type, language, and function/procedure kind. |
| `types` | Schema, name, type kind, and owner. |
| `privileges` | Schema, object, kind, owner, grantee, aggregated privileges, and whether any listed privilege has grant option. Only stored ACL entries are shown; ownership and implicit/default privileges are not expanded into rows. |

In interactive mode, `refresh` asks before loading columns when more than 1,000
non-system relations exist. In non-interactive mode it does not prompt and
skips column metadata above that threshold.

## Examples

```text
help
help functions
connection
session
refresh

schemas
schemas auth
databases prod
roles app
extensions postgis

tables user
tables pg_catalog -x
views reporting
functions auth.login
types status
privileges users

quit
```

Do not write `tables;`. DBCrab reports: `Commands do not use semicolons.`
