---
title: Keybindings
description: Exhaustive DBCrab 0.6 editor, prompt, command, Vi grammar, and TUI key defaults and action names.
---

DBCrab has two Reedline editors: SQL and command. Both use the configured
`edit-mode` (`emacs` by default), reverse history search, shared editor updates,
and navigation remaps. The command editor adds command-only actions and updates.

Key names below are the exact configuration spellings printed by
`dbcrab --default-config`.

## Shared editor defaults

These action bindings are common to Emacs and Vi insert mode in both editors.

| Action name | Default keys |
| --- | --- |
| `esc` | `esc` |
| `ctrl-c` | `ctrl-c` |
| `ctrl-d` | `ctrl-d` |
| `clear-screen` | `ctrl-l` |
| `history-menu` | `ctrl-r` |
| `open-editor` | `ctrl-o` |
| `enter` | `ctrl-j` |
| `insert-newline` | `alt-enter`, `shift-enter` |
| `up` | `ctrl-p`, `up` |
| `down` | `ctrl-n`, `down` |
| `left` | `left` |
| `right` | `right` |
| `to-start` | `alt-<`, `alt-shift-,` |
| `to-end` | `alt->`, `alt-shift-.` |
| `move-to-start` | `ctrl-home` |
| `move-to-line-start` | `ctrl-a`, `home` |
| `move-to-end` | `ctrl-end` |
| `move-to-line-end` | `ctrl-e`, `end` |
| `move-word-left` | `ctrl-left` |
| `move-word-right` | `ctrl-right` |
| `backspace` | `backspace`, `ctrl-h` |
| `delete` | `delete` |
| `backspace-word` | `ctrl-backspace` |
| `delete-word` | `ctrl-delete` |
| `select-all` | `ctrl-shift-a` |
| `move-line-up-select` | `shift-up` |
| `move-line-down-select` | `shift-down` |
| `move-left-select` | `shift-left` |
| `move-right-select` | `shift-right` |
| `move-word-left-select` | `ctrl-shift-left` |
| `move-word-right-select` | `ctrl-shift-right` |
| `move-to-line-start-select` | `shift-home` |
| `move-to-line-end-select` | `shift-end` |
| `move-to-start-select` | `ctrl-shift-home` |
| `move-to-end-select` | `ctrl-shift-end` |

Right movement first accepts a history hint or moves within an open menu. Up,
down, and left likewise operate an open menu before normal editor navigation.

## Emacs additions

Plain characters insert text. Plain `Enter` submits a complete input or lets the
SQL validator continue a multiline statement.

| Action name | Additional default keys |
| --- | --- |
| `enter` | `enter` |
| `left` | `ctrl-b` |
| `right` | `ctrl-f` |
| `move-word-left` | `alt-b`, `alt-left` |
| `move-word-right` | `alt-f`, `alt-right` |
| `backspace-word` | `alt-backspace`, `alt-m` |
| `delete-word` | `alt-delete` |
| `cut-from-start` | `ctrl-u` |
| `kill-line` | `ctrl-k` |
| `cut-word-left` | `ctrl-w` |
| `cut-word-right` | `alt-d` |
| `paste-cut-buffer-before` | `ctrl-y` |
| `undo` | `ctrl-z` |
| `redo` | `ctrl-g` |
| `uppercase-word` | `alt-u` |
| `lowercase-word` | `alt-l` |
| `capitalize-char` | `alt-c` |
| `swap-graphemes` | `ctrl-t` |

## Vi insert defaults

Vi starts in insert mode. It has every shared editor binding plus:

| Action name | Additional default key |
| --- | --- |
| `backspace-word` | `ctrl-w` |

Plain characters insert text. Plain `Enter` submits or continues multiline SQL.
`Esc` enters Vi normal mode and steps the caret back one grapheme, following
Reedline's Vi cursor policy.

## Vi normal direct bindings

These direct event bindings coexist with the Vi grammar in the next section.

| Action name | Default keys |
| --- | --- |
| `esc` | `esc` |
| `ctrl-c` | `ctrl-c` |
| `ctrl-d` | `ctrl-d` |
| `clear-screen` | `ctrl-l` |
| `history-menu` | `ctrl-r` |
| `open-editor` | `ctrl-o` |
| `up` | `ctrl-p`, `up` |
| `down` | `ctrl-n`, `down` |
| `left` | `left` |
| `right` | `right` |
| `to-start` | `alt-<`, `alt-shift-,` |
| `to-end` | `alt->`, `alt-shift-.` |
| `move-to-start` | `ctrl-home` |
| `move-to-line-start` | `ctrl-a`, `home` |
| `move-to-end` | `ctrl-end` |
| `move-to-line-end` | `ctrl-e`, `end` |
| `move-left` | `backspace` |
| `move-word-left` | `ctrl-left` |
| `move-word-right` | `ctrl-right` |
| `delete` | `delete` |
| `select-all` | `ctrl-shift-a` |
| `move-line-up-select` | `shift-up` |
| `move-line-down-select` | `shift-down` |
| `move-left-select` | `shift-left` |
| `move-right-select` | `shift-right` |
| `move-word-left-select` | `ctrl-shift-left` |
| `move-word-right-select` | `ctrl-shift-right` |
| `move-to-line-start-select` | `shift-home` |
| `move-to-line-end-select` | `shift-end` |
| `move-to-start-select` | `ctrl-shift-home` |
| `move-to-end-select` | `ctrl-shift-end` |

Plain `Enter` submits and returns to insert mode. `Esc` cancels an incomplete Vi
sequence and stays in normal mode.

## Vi normal and visual grammar

The following keys are parsed as Vi grammar, not as `vi-normal` action-node
bindings. Use `vi-remap`, not an editor action update, to change their input
keys.

### Motions

| Keys | Motion |
| --- | --- |
| `h`, `j`, `k`, `l` | Left, down, up, right. Bare `j`/`k` navigate menu/history; in visual mode they extend by display line. |
| `b`, `B` | Previous word or whitespace-delimited big word. |
| `w`, `W` | Next word or big word start. |
| `e`, `E` | Next word or big word end. |
| `0`, `^`, `$` | Line start, first nonblank, line end. |
| `f<char>`, `t<char>` | Forward to the character, or just before it. |
| `F<char>`, `T<char>` | Backward to the character, or just before it. |
| `;`, `,` | Repeat the last character search in the same or reverse direction. |
| `gg`, `G` | Buffer start or end. |

A positive decimal count whose first digit is `1` through `9` may precede a
motion. Counts may contain following digits, including zero. Operators also
accept a count before the operator and another before the motion; Reedline
repeats by the product.

### Commands and operators

| Keys | Behavior |
| --- | --- |
| `i`, `a` | Enter insert mode at the cursor, or after it. |
| `I`, `A` | Move to line start/end and enter insert mode. |
| `o`, `O` | Insert a new line below/above and enter insert mode. In visual mode either swaps cursor and anchor. |
| `d<motion>`, `c<motion>`, `y<motion>` | Delete, change, or yank through a motion. `dd`, `cc`, and `yy` operate linewise. `cw`/`cW` change through word end. |
| `x` | Delete/cut the current character; in visual mode cut the selection. |
| `s` | Delete/cut the current character or visual selection and enter insert mode. |
| `r<char>` | Replace the current character; visual replacement returns to normal mode. |
| `C`, `D`, `S` | Change to line end, delete to line end, or rewrite the current line. Change forms enter insert mode. |
| `p`, `P` | Paste the cut buffer after or before. |
| `u` | Undo. |
| `~` | Switch the current character's case. |
| `.` | Repeat the last completed action. |
| `?` | Start Reedline history search. |
| `v` | Enter visual mode. Motions extend selection; `Esc` returns to normal. |

`d`, `c`, and `y` also support text objects. `i` means inner and `a` means
around. Object selectors are `w` (word), `W` (big word), `b` (brackets), and
`q` (quote). Inner pair selectors for all three operators are `(`, `)`, `[`,
`]`, `{`, `}`, `<`, `>`, `"`, `'`, `` ` ``, and `$`. Around-pair selectors are
implemented for `d` and `y`; `c` around accepts the four object selectors.

In visual mode, bare `d`, `c`, and `y` operate on the selection without a
motion. Delete and yank return to normal mode; change enters insert mode.

## SQL prompt actions

Prompt actions are checked around the selected editor mode.

| Action name | Default keys | Behavior |
| --- | --- | --- |
| `complete` | `tab`, `ctrl-space` | Open or advance schema-aware completion. These bindings are installed into Emacs and Vi insert keymaps. |
| `cycle-display` | `alt-v` | Cycle `auto -> tui -> inline -> inline-blank -> auto` and repaint the right prompt label. Works with SQL in the buffer. |
| `command-mode` | `":"` | Enter the `: ` command editor only when the SQL buffer is known empty. Otherwise the character is inserted or handled by Vi grammar. |

`Ctrl-R` opens reverse history search in either editor mode. SQL history is
persistent when available; command history is in-memory for the current process.

## Command editor

The command editor inherits the selected Emacs/Vi defaults and shared
`keybindings.editor` updates. In Emacs and Vi insert mode, command line-editor
updates are then applied, followed by command completion bindings. Vi normal
mode uses `editor.vi-normal` and Vi grammar; command line-editor updates are not
applied there.

| Command action | Default keys | Behavior |
| --- | --- | --- |
| `complete` | `tab`, `ctrl-space` | Open or advance command/name/flag completion. |
| `cancel` | `esc`, `ctrl-d` | Return to SQL mode. In Vi insert mode the first unmodified `Esc` enters normal mode; a second `Esc` cancels. `Ctrl-D` cancels directly. |

Plain `Enter` runs the command. An empty submitted command returns to SQL mode.
`Ctrl-C` clears/interrupts the current command input but does not leave command
mode. Commands are always considered complete; they do not wait for semicolons.

## TUI defaults

| Action name | Default keys | Behavior |
| --- | --- | --- |
| `left` | `left`, `h` | Move one column, or scroll preview up one line when preview has focus. |
| `up` | `up`, `k` | Move one row, or scroll preview up one line. |
| `right` | `right`, `l` | Move one column, or scroll preview down one line. |
| `down` | `down`, `j` | Move one row, or scroll preview down one line. |
| `half-page-left` | `shift-h` | Move left by half the visible columns, or preview up half a page. |
| `half-page-up` | `shift-k` | Move up half a page, or preview up half a page. |
| `half-page-right` | `shift-l` | Move right half a page, or preview down half a page. |
| `half-page-down` | `shift-j`, `ctrl-d` | Move down half a page, or preview down half a page. |
| `full-page-left` | `ctrl-h` | Move left one visible page, or preview up one page. |
| `full-page-up` | `ctrl-k`, `pageup` | Move up one visible page, or preview up one page. |
| `full-page-right` | `ctrl-l` | Move right one visible page, or preview down one page. |
| `full-page-down` | `ctrl-j`, `pagedown` | Move down one visible page, or preview down one page. |
| `toggle-preview` | `enter` | Open or close selected-cell preview. |
| `focus-next` | `tab` | Switch table/preview focus while preview is open. |
| `edit-cell` | `c` | Edit an eligible selected cell in the preview pane. |
| `stage-change` | `ctrl-s` | Stage the edit buffer. Active only while editing. |
| `stage-null` | `ctrl-x` | Stage SQL null for an eligible nullable cell while editing or with preview focused. |
| `update-row` | `ctrl-u` | Write the selected row's staged changes. |
| `yank-cell` | `y` | Copy the selected displayed value to the terminal clipboard. |
| `quit` | `q`, `esc`, `ctrl-c` | Close the TUI. At least one quit key is required. |

TUI action keys must be unique across actions. `shortcut-nav-remap` is applied
to Ctrl/Alt TUI keys before action lookup.

## Valid action names

The valid line-editor action names for `editor.emacs-vi-insert`,
`editor.vi-normal`, and the line-editor portion of `command` are exactly:

```text
esc ctrl-c ctrl-d clear-screen history-menu open-editor enter insert-newline
up down left right to-start to-end
move-to-start move-to-line-start move-to-line-non-blank-start
move-to-end move-to-line-end move-line-up move-line-down move-left move-right
move-word-left move-word-right move-big-word-left move-big-word-right
move-word-right-start move-word-right-end
move-big-word-right-start move-big-word-right-end
backspace delete backspace-word delete-word cut-char clear clear-to-line-end
cut-current-line cut-from-start cut-from-line-start
cut-from-line-non-blank-start cut-to-end cut-to-line-end kill-line
cut-word-left cut-word-right cut-big-word-left cut-big-word-right
paste-cut-buffer-before paste-cut-buffer-after paste undo redo
uppercase-word lowercase-word capitalize-char switchcase-char
swap-words swap-graphemes select-all copy-selection cut-selection
copy-from-start copy-from-line-start copy-from-line-non-blank-start
copy-to-end copy-to-line-end copy-current-line
copy-word-left copy-word-right copy-big-word-left copy-big-word-right
move-line-up-select move-line-down-select move-left-select move-right-select
move-word-left-select move-word-right-select
move-to-line-start-select move-to-line-end-select
move-to-start-select move-to-end-select
```

Additional group-specific names are:

| Group | Exact action names |
| --- | --- |
| `prompt` | `complete`, `cycle-display`, `command-mode` |
| `command` | `complete`, `cancel`, plus every line-editor action above |
| `tui` | `left`, `up`, `right`, `down`, `half-page-left`, `half-page-up`, `half-page-right`, `half-page-down`, `full-page-left`, `full-page-up`, `full-page-right`, `full-page-down`, `toggle-preview`, `focus-next`, `edit-cell`, `stage-change`, `stage-null`, `update-row`, `yank-cell`, `quit` |

## Updating and remapping

Action updates use `do=set` by default; `do=add` and `do=remove` patch the
current list in order. Vi grammar is not represented by action names and is
changed through `vi-remap`. Ctrl/Alt navigation is changed globally through
`shortcut-nav-remap`.

```kdl
keybindings {
    editor {
        emacs-vi-insert {
            open-editor do=remove ctrl-o
        }
    }

    prompt {
        complete do=add ctrl-y
    }

    vi-remap modes=normal,visual {
        j do=swap n
    }

    shortcut-nav-remap {
        ctrl-j do=swap ctrl-n
    }
}
```

For operation semantics, key-name grammar, modifier restrictions, ordering, and
validation, see [Configuration](../configuration/#binding-updates).
