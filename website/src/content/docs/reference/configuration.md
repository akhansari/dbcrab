---
title: Configuration
description: Complete DBCrab 0.6 user KDL configuration schema, defaults, loading, validation, and key update operations.
---

DBCrab 0.6 uses KDL for user configuration. TOML is not supported. A file may
contain only the settings being changed; omitted settings retain built-in
defaults.

This user configuration is distinct from a project's small `dbcrab.kdl` context
file. See [Project configuration](#project-configuration).

## Print built-ins

These commands do not require a connection or load a configuration file:

```sh
dbcrab --default-config  # generated defaults and every default key
dbcrab --config-schema   # embedded KDL schema
```

The current schema is also available in GitLab as
[`config.schema.kdl`](https://gitlab.com/akhansari/dbcrab/-/blob/main/config.schema.kdl).

## KDL model

The generated file uses KDL 2 syntax; `/- kdl-version 2` is its version marker.
The embedded schema identifies itself as version `1.0.0`. Configuration is a
flat KDL document whose supported nodes use three shapes:

- a leaf node with positional string values, such as `edit-mode vi`;
- a named group with a children block, such as `keybindings { ... }`;
- an action/remap leaf with key arguments and an optional string `do` property.

Node names, nesting, value counts, properties, and child-block presence are all
validated rather than ignored.

## Loading

| Invocation | User config loaded? | Project config loaded? |
| --- | --- | --- |
| Interactive `dbcrab <CONNECTION>` | yes | yes |
| `dbcrab <CONNECTION> -e <SQL>` | no | no |
| `dbcrab <CONNECTION> -: 'session'` | yes | yes |
| `dbcrab <CONNECTION> -: 'run ...'` | yes | yes |
| `dbcrab <CONNECTION> -: 'named ...'` | yes | yes |
| Other non-interactive commands, such as `-: 'tables'` | no | no |
| `--default-config`, `--config-schema`, help, version | no | no |

Without `--config`, DBCrab checks the platform user path documented in
[Paths and environment](../paths-environment/#user-configuration). If no path
can be determined or the implicit file does not exist, built-in defaults are
used. An existing implicit file that cannot be read or parsed is an error.

`--config <PATH>` selects one file; it does not merge with the implicit file.
When the invocation loads runtime configuration, a missing explicit file is an
error. Relative CLI paths are interpreted from the process working directory.

## Defaults

| Setting | Default |
| --- | --- |
| `edit-mode` | `emacs` |
| `named-sql.shared-path` | Platform data directory under `dbcrab/named-sql/shared` |
| Keybindings | Generated Reedline, prompt, command, and TUI defaults in [Keybindings](../keybindings/) |
| Remaps | None |

## Complete node hierarchy

| Node path | Cardinality | Value or children |
| --- | --- | --- |
| `edit-mode` | zero or one | Exactly one string: `emacs` or `vi`. |
| `named-sql` | zero or one | Group containing only `shared-path`. |
| `named-sql.shared-path` | zero or one | Exactly one non-empty path string. Relative paths resolve from the user config file's directory; `~` is expanded. |
| `keybindings` | zero or one | Group containing `editor`, `prompt`, `vi-remap`, `shortcut-nav-remap`, `command`, and `tui`. |
| `keybindings.editor` | zero or one | Group containing `emacs-vi-insert` and `vi-normal`. |
| `keybindings.editor.emacs-vi-insert` | zero or one | Ordered updates for any line-editor action. Applied to the SQL and command editors in Emacs and Vi insert mode. |
| `keybindings.editor.vi-normal` | zero or one | Ordered updates for any line-editor action in Vi normal mode. Vi grammar keys are separate from action bindings. |
| `keybindings.prompt` | zero or one | SQL-prompt actions: `complete`, `cycle-display`, `command-mode`. |
| `keybindings.vi-remap` | at most two non-overlapping mode groups | Remaps plain/Shift keys in Vi `normal`, `visual`, or `normal,visual` modes. Requires a `modes` property and child remap nodes. |
| `keybindings.shortcut-nav-remap` | zero or one | Global editor/TUI remaps whose sources contain Ctrl or Alt. |
| `keybindings.command` | zero or one | Command-only `complete` and `cancel` actions plus every line-editor action. Command line-editor updates apply after shared editor updates in Emacs and Vi insert mode. |
| `keybindings.tui` | zero or one | Full-screen result viewer actions. |

The exact valid action names and defaults are exhaustive in
[Keybindings](../keybindings/#valid-action-names).

## Binding updates

An action node has zero or more key values and an optional `do` property:

```kdl
keybindings {
    prompt {
        complete do=remove ctrl-space
        complete do=add ctrl-y
        cycle-display ctrl-v
    }

    tui {
        quit q esc ctrl-c ctrl-q
    }
}
```

| `do` value | Default? | Operation |
| --- | --- | --- |
| `set` | yes, when `do` is omitted | Replace that action's current bindings with the listed keys. An empty list clears the action. |
| `add` | no | Append keys that are not already present. |
| `remove` | no | Remove the listed keys from that action. For line-editor actions, a key is removed only if it is currently bound to that same action. |

Repeated action nodes are valid and are applied in document order. Keys within
an update are deduplicated. Omitted actions keep their defaults.

The final TUI map has extra validation: `quit` must retain at least one key, and
one key cannot be bound to two TUI actions. Prompt and editor groups do not run
that TUI-specific collision check; a later line-editor binding for a key can
replace the event previously attached to that key.

## Remap operations

The child node name is the source key and its sole argument is the target:

```kdl
keybindings {
    vi-remap modes=normal,visual {
        j do=swap n
    }

    shortcut-nav-remap {
        ctrl-j do=swap ctrl-n
    }
}
```

| `do` value | Default? | Operation |
| --- | --- | --- |
| `set` | yes, when `do` is omitted | One-way source-to-target remap. |
| `swap` | no | Install both source-to-target and target-to-source remaps. |

`vi-remap` requires exactly one untyped string property:
`modes=normal`, `modes=visual`, or `modes=normal,visual`. No two blocks may
overlap a mode. Sources cannot contain Ctrl or Alt; both sides of a swap must
omit them. Shift is allowed. A one-way target may use Ctrl or Alt.

`shortcut-nav-remap` sources must contain Ctrl or Alt. Both sides of a swap must
contain Ctrl or Alt; a one-way target may be unmodified. Later definitions of
the same source replace earlier ones. Remaps are one lookup, not a recursively
chained transformation.

## Key grammar

Runtime key parsing is case-insensitive. The published KDL schema patterns are
lowercase, so use lowercase for compatibility with external schema validators.
Modifiers are prefixes and may be combined: `ctrl-`, `control-`, `alt-`, and
`shift-`.

Named keys are:

```text
left up right down home end
pageup page-up pagedown page-down
esc escape enter return tab backspace delete del space
```

Any single character is also valid. Quote KDL-sensitive tokens such as `":"`.
Unknown names, empty strings, and multi-character names outside the named list
are errors.

## Validation and strictness

The parser rejects the complete file on any invalid setting. Errors include a
KDL parse error or source line where available. In particular:

- unknown top-level, group, or action nodes are rejected;
- singleton top-level and group nodes may not be repeated;
- group nodes require a child block and accept no arguments or properties;
- leaf/action/remap nodes do not accept child blocks;
- KDL type annotations are not accepted on nodes or values;
- unknown or duplicate `do` properties are rejected;
- values and key names must be strings of the required count;
- `named-sql.shared-path` must be non-empty;
- final remap and TUI constraints are validated after parsing.

Settings are not partially applied after an error.

## Minimal example

```kdl
/- kdl-version 2

edit-mode vi

named-sql {
    shared-path "~/sql/shared"
}

keybindings {
    prompt {
        complete do=add ctrl-y
        cycle-display ctrl-v
    }

    editor {
        emacs-vi-insert {
            clear-screen ctrl-l
        }
    }

    tui {
        quit do=add ctrl-q
    }
}
```

## Project configuration

`dbcrab.kdl` is not a user configuration file and does not use the schema above.
It is discovered from the working project and accepts only:

```kdl
context "billing"
named-sql-path "./sql"
```

Both nodes are optional singletons, but `named-sql-path` requires `context`.
Values must be exactly one non-empty untyped string. Unknown nodes, properties,
children, duplicates, and type annotations are errors. It cannot configure edit
mode, keybindings, or the shared named-SQL root.

Discovery, precedence, and path resolution are documented under
[Project configuration](../paths-environment/#project-configuration).
