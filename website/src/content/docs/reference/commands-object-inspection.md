---
title: Object Inspection Commands
description: Exact describe and source syntax, object-kind flags, resolution, candidates, and output sections.
---

Object inspection commands run in command mode and do not use semicolons.

## Syntax

```text
describe <name> [-f|--function|-s|--schema|-t|--table|-v|--view|-T|--type|-r|--relation] [-x|--system]
source <function-or-view> [-f|--function|-v|--view] [-x|--system]
```

The kind flags within each command are mutually exclusive. With no kind flag,
DBCrab searches every kind supported by that command. Both commands also accept
`-h`/`--help`.

## `describe`

| Flag | Objects considered |
| --- | --- |
| none | Relations, functions/procedures, schemas, and types. |
| `-f`, `--function` | Functions and procedures. |
| `-s`, `--schema` | Schemas. |
| `-t`, `--table` | Ordinary, partitioned, and foreign tables. |
| `-v`, `--view` | Views and materialized views. |
| `-T`, `--type` | Base, composite, domain, enum, multirange, and range types. |
| `-r`, `--relation` | Ordinary and partitioned tables, views, materialized views, sequences, foreign tables, and indexes. |
| `-x`, `--system` | Include objects from system, information, TOAST, and temporary schemas in the search. This can be combined with one kind flag. |

### Output by object

| Object | Sections |
| --- | --- |
| Relation | `Relation` summary; `Columns`; `Indexes`; `Constraints`; explicit `Privileges`. |
| Function or procedure | `Function` summary; full `Arguments`; explicit `Privileges`. |
| Schema | `Schema` summary; counts in `Objects`; explicit `Privileges`. |
| Type | `Type` summary; `Enum Values`; `Composite Fields`; `Domain`; `Domain Constraints`; `Range`; explicit `Privileges`. Empty non-applicable sections are retained. |

Relation summaries include schema, name, kind, owner, estimated rows, total size,
and comment. Function summaries include identity arguments, return type,
language, volatility, parallel safety, security mode, owner, and comment.

## `source`

| Flag | Objects considered | Returned definition |
| --- | --- | --- |
| none | Functions, procedures, views, and materialized views. | Function/procedure DDL or view query definition. |
| `-f`, `--function` | Functions and procedures only. | PostgreSQL `pg_get_functiondef` output. |
| `-v`, `--view` | Views and materialized views only. | PostgreSQL `pg_get_viewdef(..., true)` output. |
| `-x`, `--system` | Include system-schema candidates. | Same output as above. |

Function output has `Function` and `Source` sections. View output has `View` and
`Source` sections.

## Name resolution

Accepted target forms are:

```text
name
schema.name
function(type, type)
schema.function(type, type)
```

Unquoted identifiers are folded to lowercase. Double-quoted PostgreSQL
identifiers preserve case and may contain spaces or doubled quotes:

```text
describe "Sales Data"."Orders" -t
```

Resolution proceeds as follows:

1. DBCrab searches for exact object names in the selected kinds. Supplying a
   schema restricts exact matches to that schema; omitting it searches all
   non-system schemas, not only the current `search_path`.
2. A single exact match is inspected immediately.
3. Multiple exact matches produce a `Candidates` table instead of choosing one.
4. With no exact match, DBCrab performs a case-insensitive substring search and
   returns candidates. If none match, it returns `No object matched <target>`.

Candidates contain `kind`, `schema`, `name`, and `detail`. Function details show
identity arguments and return type. Candidates in current search-path schemas
sort first, but sorting never resolves ambiguity automatically.

For overloaded functions, add a type-only identity signature. The signature is
compared case-insensitively but is otherwise text-exact against PostgreSQL's
`oidvectortypes` list, including comma spacing:

```text
describe auth.login(text, text) -f
source auth.login(text, text) -f
```

Argument names are not part of the matching signature. Use the schema and a
type list such as `text, text`; do not copy argument names that PostgreSQL may
include in descriptive identity-argument output.

## Examples

```text
describe users
describe public.users -t
describe active_users -v
describe billing.invoice_status -T
describe auth -s
describe login -f
describe pg_catalog.pg_class -t -x

source active_users -v
source auth.login(text, text) -f
source pg_catalog.now -f -x
```
