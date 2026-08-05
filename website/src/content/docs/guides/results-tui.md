---
title: Explore query results
description: Choose inline or full-screen result display and navigate DBCrab's table viewer.
---

## Understand inline output

Run a small query first:

```sql
select * from users order by id limit 1;
```

When output stays inline:

- No rows produce a row count only.
- One row uses an expanded field/value layout.
- Two or more rows use a table with column headings.
- Cells wrap to the terminal width; table cells taller than six lines end in `...`.

Use a larger limit to see the normal multirow table:

```sql
select * from orders order by 1 limit 10;
```

For `json` and `jsonb`, the one-row expanded layout pretty-prints and highlights valid JSON:

```sql
select '{"name":"Crab","active":true}'::jsonb as profile;
```

Multirow JSON table cells remain compact. The TUI preview also pretty-prints and highlights valid JSON.

## Let Auto choose

`auto` is the initial display mode. It opens the TUI only when standard output is an interactive terminal and at least one of these conditions applies:

- More than one column would average less than 12 terminal columns of width.
- The result body plus table overhead would exceed the terminal height. For a one-row expanded result, each field counts as a body row.
- The rendered, wrapped inline output would exceed the terminal height.

Small results and empty results stay inline. Redirected output never opens the TUI automatically.

## Select a display mode

At the SQL prompt, press **Alt-V** to cycle exactly through:

1. `auto`
2. `tui`
3. `inline`
4. `inline-blank`
5. Back to `auto`

The right side of the SQL prompt shows `[tui]`, `[inline]`, or `[inline-blank]`; `auto` has no label. `tui` opens the viewer for every nonempty row result. `inline-blank` prints the inline layout without table borders.

## Navigate the TUI

Use the status line at the bottom to see the selected row, column, visible column range, and available actions.

| Task | Keys |
| --- | --- |
| Move one cell | Arrow keys or `h`, `j`, `k`, `l` |
| Move half a page | `H`, `J`, `K`, `L`; **Ctrl-D** also moves down |
| Move a full page | **Ctrl-H**, **Ctrl-J**, **Ctrl-K**, **Ctrl-L**, **PageUp**, **PageDown** |
| Open or close cell preview | **Enter** |
| Switch table/preview focus | **Tab** |
| Yank the selected cell | `y` |
| Close the viewer | `q`, **Esc**, or **Ctrl-C** |

With preview focus, navigation keys scroll the preview instead of moving the selected table cell. Yanking uses the terminal clipboard protocol and copies the displayed value, including a staged value.

After the viewer closes, DBCrab prints the result row count and returns to the prompt.

:::caution
DBCrab fetches the complete result before displaying it. The TUI makes large results easier to navigate, but it is not server-side pagination and does not reduce query cost or memory use. Add `WHERE` and `LIMIT` clauses whenever practical. Inline output warns only after more than 1,000 rows.
:::

To change data from an eligible result, continue with [Edit result rows](../editing-results/). See the [display and output reference](../../reference/display-output/) and [keybindings guide](../keybindings/) for all display and navigation actions.
