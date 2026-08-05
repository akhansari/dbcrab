---
title: Configure DBCrab
description: Load strict KDL user settings for editing, shared named SQL, and keybindings.
---

## Find the user configuration

DBCrab supports KDL configuration only. It looks for `dbcrab/config.kdl` under:

1. `$XDG_CONFIG_HOME`, when set.
2. `$HOME/.config` on Unix-like systems.
3. `%APPDATA%` on Windows, with `%USERPROFILE%\AppData\Roaming` and then
   `$HOME/.config` as fallbacks.

If the default path does not exist, DBCrab uses built-in defaults. Select one file explicitly with `--config`:

```sh
dbcrab "$PG_URL" --config ./team-dbcrab.kdl
```

When runtime configuration is loaded, an explicit missing path is an error.
`--config` replaces default-path lookup; DBCrab does not merge several user
configuration files.

Runtime configuration is loaded for the interactive REPL and for unattended `session`, `run`, and `named` commands. Plain `-e` and unrelated `-:` commands do not need editor, history, or named SQL configuration.

## Start with minimal KDL

Add only settings you want to change:

```kdl
/- kdl-version 2

edit-mode vi
```

`edit-mode` accepts `emacs` or `vi` and applies to both SQL and command editors.

To override shared named SQL storage:

```kdl
named-sql {
    shared-path "~/dbcrab-sql/shared"
}
```

`~` is expanded. A relative `shared-path` resolves from the user configuration file's directory, including when `--config` itself is relative.

## Keep project settings separate

A project `dbcrab.kdl` is not a user configuration layer. It accepts only a project context and its optional local named SQL root:

```kdl
context "billing"
named-sql-path "./sql"
```

DBCrab checks the current directory first and then the Git root; those project files do not merge. User `config.kdl` still supplies editing, shared named SQL, and keybindings. See [Contexts and history](../contexts-history/) for project lookup and `--context` precedence.

## Understand defaults and validation

DBCrab starts from built-in defaults, then applies the selected user file:

- Scalar settings replace their defaults.
- Omitted settings keep their defaults.
- Keybinding `set`, `add`, and `remove` updates are applied in document order.
- Unknown nodes, unknown properties, duplicate singleton groups, invalid values, and malformed KDL reject the complete file with a line-numbered error.
- TUI bindings are validated for conflicting keys and must leave at least one `quit` key.

:::caution
Configuration is strict: a misspelled setting is not ignored. Run DBCrab after editing the file and correct any validation error before relying on the new settings.
:::

Print a complete, commented configuration generated from current defaults:

```sh
dbcrab --default-config
```

Print the KDL schema used by this release:

```sh
dbcrab --config-schema
```

Continue with [Customize keybindings](../keybindings/). See the [configuration reference](../../reference/configuration/) for every accepted node and validation rule.
