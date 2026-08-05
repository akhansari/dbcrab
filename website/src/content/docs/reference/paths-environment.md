---
title: Paths and Environment
description: DBCrab 0.6 user config, history, named-SQL and project paths, environment precedence, editors, and connection prompting.
---

DBCrab derives user paths directly from environment variables. Empty variable
values are ignored. XDG variables take precedence on both Unix and Windows when
set.

## User configuration

The implicit user configuration path is selected in this order:

| Priority | Platform | Path |
| --- | --- | --- |
| 1 | all | `$XDG_CONFIG_HOME/dbcrab/config.kdl` |
| 2 | Windows | `%APPDATA%\dbcrab\config.kdl` |
| 3 | Windows | `%USERPROFILE%\AppData\Roaming\dbcrab\config.kdl` |
| 4 | Windows fallback | `$HOME/.config/dbcrab/config.kdl` |
| 2 | Unix fallback | `$HOME/.config/dbcrab/config.kdl` |

`--config <PATH>` bypasses this lookup. An implicit missing file or unavailable
config base uses built-in defaults; an explicit missing file is an error. DBCrab
does not create a default user config automatically. Loading rules and KDL
schema are in [Configuration](../configuration/).

## SQL history

The history state base is selected in this order:

| Priority | Platform | State base |
| --- | --- | --- |
| 1 | all | `$XDG_STATE_HOME` |
| 2 | all | `$HOST_XDG_STATE_HOME` |
| 3 | Windows | `%LOCALAPPDATA%` |
| 4 | Windows | `%USERPROFILE%\AppData\Local` |
| 5 | Windows fallback | `$HOME/.local/state` |
| 3 | Unix fallback | `$HOME/.local/state` |

`HOST_XDG_STATE_HOME` is implemented only for history. It is useful when a
container or sandbox receives a host state directory separately from its own
`XDG_STATE_HOME`.

DBCrab appends `dbcrab/history/` and chooses the filename:

| Context | Filename |
| --- | --- |
| no named context (`shared`) | `history` |
| `--context billing` | `billing.history` |

The SQL history stores at most 1,000 entries and preserves multiline input.
Consecutive duplicate and empty entries are not added. SQL input beginning with
a space is excluded. The command editor also keeps up to 1,000 entries, but its
history is process-local and never written to this path.

The history layer creates missing parent directories and the file. If no state
base is available, or creation/opening fails, DBCrab warns and continues with
in-memory SQL history. A later sync failure also produces a warning rather than
ending the REPL.

### Context filename sanitization

The history filename function trims a context and preserves ASCII letters,
digits, `.`, `-`, and `_`; every other character becomes `_`, and an empty
result becomes `context`. Current CLI/project context validation is stricter:
it accepts only lowercase ASCII letters, digits, `.`, `_`, and `-`, forbids an
initial `-`, and rejects empty, `.`/`..`, and reserved `shared`. Therefore a
valid 0.6 context normally reaches the filename unchanged.

## Named-SQL data

The default named-SQL base is selected in this order:

| Priority | Platform | Base before DBCrab suffix |
| --- | --- | --- |
| 1 | all | `$XDG_DATA_HOME` |
| 2 | Windows | `%LOCALAPPDATA%` |
| 3 | Windows | `%USERPROFILE%\AppData\Local` |
| 2 | Unix fallback | `$HOME/.local/share` |

DBCrab appends `dbcrab/named-sql`, then uses one top-level directory per scope:

```text
<data-base>/dbcrab/named-sql/shared/
<data-base>/dbcrab/named-sql/<context>/
```

Windows has no `$HOME` fallback for this data lookup. A user-configured
`named-sql.shared-path` replaces the shared root. A project
`named-sql-path` can replace its matching active context root.

Save creates missing directories. Listing a missing root returns an empty list;
read/delete of a missing file reports an error. If DBCrab needs named SQL but
cannot determine all required roots, startup or the relevant unattended command
fails with guidance to set `XDG_DATA_HOME`, `HOME`, or explicit paths. Plain
`--execute` and commands that do not load runtime configuration are unaffected.

See [Named SQL](../named-sql/) for file mapping, scope resolution, overlap
checks, and symlink rules.

## Project configuration

The project file is named exactly `dbcrab.kdl`. Discovery checks:

1. `dbcrab.kdl` in the current working directory.
2. If absent, the nearest ancestor containing `.git`, then `dbcrab.kdl` at that
   Git root.

DBCrab does not search arbitrary intermediate parent directories. A current
directory file wins; files are not merged.

The strict format accepts only:

```kdl
context "billing"
named-sql-path "./sql"
```

`named-sql-path` requires `context`. A relative path resolves from the directory
containing `dbcrab.kdl`; an absolute path remains absolute; leading `~` is
expanded. It points directly to that context's query root, not to a base that
will receive another context segment.

CLI `--context` has higher context precedence. If it equals the project context,
the project path is used while the context source is reported as `--context`.
If it selects a different name, the project path is ignored and the default
`<context>/` data path is used. The discovered project file is still parsed, so
an invalid project file remains an error even with a CLI override.

Use `session` and `named status` to inspect the selected context, source, project
file, history path, and active/shared roots.

## Home expansion

DBCrab expands only a first path component equal to `~`, as in `~/sql`. It does
not expand `~alice`.

| Platform | Home lookup for `~` |
| --- | --- |
| Unix | `$HOME` |
| Windows | `%USERPROFILE%`, then `$HOME` |

Expansion is used for configured named-SQL paths and transfer input/output
paths. If no home value exists, the path remains literal.

## External editor

Interactive `named save <NAME>` without inline SQL chooses:

1. non-empty `$EDITOR`;
2. otherwise non-empty `$VISUAL`.

A non-Unicode value is an error. DBCrab tokenizes the selected command using its
command quoting/escape rules, launches the first token directly rather than
through a shell, appends the named-SQL path as the final argument, and waits for
exit. Arguments are supported:

```sh
export EDITOR='nvim'
export VISUAL='code --wait'
```

Because `EDITOR` wins, an existing non-empty `EDITOR` prevents `VISUAL` from
being considered. A missing editor is an error only for this no-SQL save form.
A nonzero editor exit is an error; a successful editor that does not create a
new target produces a cancelled save.

`Ctrl-O` in a line editor is Reedline's separate `open-editor` action. It is a
configurable keybinding and is not the named-SQL save selection logic above.

## Connection and password prompt

The required CLI connection value is parsed as PostgreSQL connection options:

```sh
dbcrab 'postgres://user@localhost/database'
```

DBCrab first connects with those options. Only when PostgreSQL returns SQLSTATE
`28P01` (invalid password) does it print `Password: ` and retry once with the
entered password. Input is read in terminal raw mode and is not echoed. `Enter`
submits, `Backspace` removes the previous character, `Ctrl-C` interrupts, and
`Ctrl-D` reports EOF.

The prompt behavior is the same before interactive and non-interactive modes;
include a usable password/authentication method when unattended prompting would
be inappropriate. DBCrab uses one pooled PostgreSQL session and a 10-second pool
acquire timeout. The `connection` command reports safe current connection
details and never prints the password or original connection string.
