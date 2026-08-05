---
title: Customize keybindings
description: Patch editor, SQL prompt, command mode, Vi remap, and TUI keys in KDL.
---

Keybindings live in the user KDL file described in [Configure DBCrab](../configuration/). Start from the current defaults when you need to discover an action name:

```sh
dbcrab --default-config
```

## Set, add, or remove keys

Each action node is an ordered update:

```kdl
keybindings {
    prompt {
        complete do=add ctrl-y
        complete do=remove ctrl-space
        cycle-display ctrl-v
    }
}
```

- Omitted `do` means `do=set`: replace all bindings for that action.
- `do=add` appends keys that are not already present.
- `do=remove` removes only the listed keys.
- Repeated nodes for the same action are applied from top to bottom.

In this example, completion keeps **Tab**, removes **Ctrl-Space**, and adds **Ctrl-Y**. `cycle-display ctrl-v` replaces the default **Alt-V** binding.

Editor updates also run in document order on top of Reedline's selected Emacs or Vi defaults. Removing a key from an action leaves it alone if that key is currently bound to a different action.

## Spell keys correctly

Key names are case-insensitive in configuration. Combine modifier prefixes with `-`, for example `ctrl-space`, `alt-v`, or `ctrl-shift-left`. `control-` is an alias for `ctrl-`.

Named keys include `left`, `up`, `right`, `down`, `home`, `end`, `pageup`, `pagedown`, `esc`, `enter`, `tab`, `backspace`, `delete`, and `space`, plus any single character. Aliases include `page-up`, `page-down`, `escape`, `return`, and `del`.

Most keys can be bare KDL values. Quote punctuation when KDL requires it, especially the command-mode key:

```kdl
keybindings {
    prompt {
        command-mode ":"
    }
}
```

## Choose the binding scope

Shared line-editor overrides belong under `editor`:

```kdl
keybindings {
    editor {
        emacs-vi-insert {
            clear-screen ctrl-l
            history-menu ctrl-r
        }

        vi-normal {
            undo u
        }
    }
}
```

`emacs-vi-insert` applies to both SQL and command editors in Emacs mode and Vi insert mode. `vi-normal` applies to their Vi normal mode.

Use the other groups for narrower behavior:

- `prompt`: SQL-only `complete`, `cycle-display`, and `command-mode` actions.
- `command`: command-mode `complete` and `cancel`, plus command-editor line actions. These line overrides apply after the shared `editor` group.
- `tui`: full-screen result actions such as movement, preview, editing, yanking, and quitting.

For example:

```kdl
keybindings {
    prompt {
        command-mode ctrl-g
    }

    command {
        complete do=add ctrl-y
        cancel esc ctrl-d ctrl-g
        clear-screen ctrl-l
    }

    tui {
        quit do=add ctrl-q
    }
}
```

TUI keys must be unique across TUI actions, and `quit` cannot be left empty.

## Remap Vi navigation

Use `vi-remap` for unmodified or Shift-modified keys in non-insert Vi modes:

```kdl
keybindings {
    vi-remap modes=normal,visual {
        j do=swap n
    }
}
```

`modes` must be `normal`, `visual`, or `normal,visual`. At most two blocks may be used, and their modes cannot overlap. Source keys cannot contain Ctrl or Alt. `do=set` is a one-way remap; `do=swap` installs both directions, and both sides of a swap must omit Ctrl and Alt. These remaps do not change typed characters in Emacs or Vi insert mode.

## Remap modified shortcuts globally

Use `shortcut-nav-remap` for Ctrl/Alt shortcuts across SQL, command, and TUI handling:

```kdl
keybindings {
    shortcut-nav-remap {
        ctrl-j do=swap ctrl-n
    }
}
```

Every source must contain Ctrl or Alt, optionally with Shift. A one-way target may be any valid key; for `do=swap`, both sides must contain Ctrl or Alt. Remaps are applied once rather than chained.

:::caution
A one-way remap or `do=set` binding can shadow an existing shortcut. Prefer `do=swap` when exchanging navigation keys, and check `dbcrab --default-config` before replacing an action's complete binding list.
:::

See the [keybindings reference](../../reference/keybindings/) for exhaustive action names and defaults, or print the release's validation rules with `dbcrab --config-schema`.
