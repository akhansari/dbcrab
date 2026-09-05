---
title: Display and Output
description: Interactive display modes, inline rendering, TUI controls and editing, plus compact and column-JSON schemas.
---

DBCrab has interactive terminal displays and separate deterministic
non-interactive encodings.

## Interactive display modes

Press `Alt-V` at the SQL prompt to cycle the exact labels in this order:

```text
auto -> tui -> inline -> inline-blank -> auto
```

| Label | Behavior |
| --- | --- |
| `auto` | Keep small results inline and open the full-screen TUI when terminal dimensions make inline output unsuitable. This is the default. |
| `tui` | Open every non-empty SQL row result in the full-screen table viewer. Empty results remain inline. |
| `inline` | Render a rounded-border table in normal terminal output. |
| `inline-blank` | Render the same data and wrapping without table borders. |

The right side of the SQL prompt shows `[tui]`, `[inline]`, or
`[inline-blank]`; the default `auto` label is hidden.

The selected SQL display mode does not govern command-mode results. A
single-section command result uses Auto; multi-section results such as
`describe` are always printed inline with section titles.

## Inline rendering

| Result shape | Rendering |
| --- | --- |
| No rows | Only `(0 rows)`. The TUI is never opened. |
| One row | Expanded vertical field/value layout with no generic `field`/`value` header. |
| Two or more rows | Horizontal table with column headers. |

Every row result ends with `(1 row)` or `(<N> rows)`. Non-row interactive SQL
prints `(<N> rows affected)` for `COPY`, `DELETE`, `FETCH`, `INSERT`, `MERGE`,
`MOVE`, and `UPDATE`, or whenever PostgreSQL reports a positive count. Other
successful statements with zero affected rows print `OK`.

Tables wrap to terminal width. Multi-row cells are limited to six display lines;
the final line becomes `...` when content is taller. One-row expanded values are
not height-limited.

### Values and types

| PostgreSQL type family | Interactive value |
| --- | --- |
| Boolean, integer, and floating point | PostgreSQL value decoded and printed as text. |
| Text, varchar, character, and `name` | Text value. |
| Date, time, timestamp, timestamptz | Chronological text; timestamptz uses RFC 3339. |
| UUID | Canonical UUID text. |
| JSON/JSONB | Compact JSON in multi-row tables. In a one-row SQL result and TUI preview, valid JSON is pretty-printed and syntax-highlighted. |
| `bytea` | Lowercase PostgreSQL hex form beginning `\x`. |
| SQL null | `(null)`, distinct from an empty string. |
| Other types | Decoded as text when SQLx permits it; otherwise shown as `<type-name>`. |

Color highlights headers, JSON tokens, and borders when terminal output supports
ANSI display.

When more than 1,000 SQL rows are rendered inline, DBCrab warns:
`warning: rendering N rows; narrow the statement's result set when possible`.
Metadata grids use `add a filter for large metadata results` instead. The
warning threshold is strictly greater than 1,000.

## Auto selection

Auto can open the TUI only when standard output is a terminal and the result is
non-empty. It selects the TUI when any condition is true:

- there is more than one column and `terminal width / column count < 12`;
- estimated body rows plus four table-overhead rows exceed terminal height;
- the actually wrapped inline rendering has more lines than terminal height.

For the height estimate, a one-row expanded result has one body row per column;
other results use their row count. If terminal dimensions cannot be read,
DBCrab uses `120 x 24`. Redirected standard output never opens Auto TUI.

## TUI controls

Default controls are:

| Action | Keys |
| --- | --- |
| Move one cell | Arrow keys or `h`, `j`, `k`, `l` |
| Move half a visible page | `Shift-H`, `Shift-J`, `Shift-K`, `Shift-L`; `Ctrl-D` also moves half-page down |
| Move one visible page | `Ctrl-H`, `Ctrl-J`, `Ctrl-K`, `Ctrl-L`; `PageUp`/`PageDown` also move vertically |
| Toggle selected-cell preview | `Enter` |
| Switch table/preview focus | `Tab` |
| Yank selected displayed value to clipboard | `y` |
| Edit an eligible cell | `c` |
| Stage edited text | `Ctrl-S` |
| Stage SQL null | `Ctrl-X` |
| Write selected row's staged changes | `Ctrl-U` |
| Cancel active cell edit | `Esc` (fixed, not configurable) |
| Close viewer | `q`, `Esc`, `Ctrl-C` when not editing |

When preview has focus, movement actions scroll it: left/up scroll toward the
start and right/down toward the end; half/full page actions use preview height.
The preview pane expands when focused. JSON/JSONB preview is pretty-printed.

After the TUI closes, DBCrab prints the result row count in the normal terminal.
Yank uses the terminal clipboard operation and reports success or failure in a
temporary message.

Actions other than edit cancellation can be changed by action name. While a cell
editor is active, keys are handled by the editor first, so viewer quit bindings
do not close the TUI until the edit is staged or cancelled. See
[Keybindings](../keybindings/#tui-defaults).

## Editing result rows

Editing is available only for interactive SQL results shown in the TUI. A
selected column is eligible when all of these are true:

- SQLx traces it to a real PostgreSQL ordinary or partitioned table column;
- it is not a primary-key column;
- the result includes every primary-key column for that table.

Views, foreign tables, expressions without origin metadata, tables without a
primary key, and results missing any primary-key column are not editable.

Press `c` to open a text editor in the preview. JSON starts as pretty text.
`Ctrl-S` stages the text; `Ctrl-X` stages SQL null only for a nullable column;
`Esc` cancels the edit buffer. Staging the original value again clears that
cell's staged change. Dirty cells and rows are highlighted.

`Ctrl-U` updates only the selected row's staged columns. DBCrab builds a quoted
`UPDATE` against the traced table, casts text through the actual PostgreSQL
column type, identifies the row by all displayed primary-key values, and
refreshes changed cells from `RETURNING`. A missing row, cast/constraint error,
non-nullable null, or lost edit metadata leaves the changes staged and reports
an error. Quitting discards remaining unwritten staged state.

## Non-interactive output

`--execute` and `--command` never use interactive tables or the TUI. They use
`--format compact` by default or `--format column-json`. After connection and
mode setup, they write successful output and structured execution errors to
standard output and exit `0` on success or `1` on execution failure. Earlier
CLI, configuration, runtime-startup, and connection errors use the ordinary
standard-error path.

`--max-rows` defaults to 1000 and limits each emitted grid independently. DBCrab
fetches the complete database result first, so `row_count`/`rows` is the full
count and `truncated` reports output truncation, not query execution truncation.

### Compact

A row result has this shape:

```text
ok rows=<FULL_COUNT> returned=<EMITTED_COUNT> truncated=<true|false> [elapsed_ms=<MS>]
cols: <name>:<postgres-type> ...
---
<tab-separated row>
```

`elapsed_ms` is included for SQL/named-SQL statement grids when nonzero. Column
names or type labels outside `[A-Za-z0-9_.-]` are JSON-quoted. Cells escape `\`
as `\\`, tab as `\t`, newline as `\n`, and carriage return as `\r`. Null is
`\N`.

Rows-affected and status records are:

```text
ok rows_affected=<N> elapsed_ms=<MS>
ok status=<STATUS> elapsed_ms=<MS>
```

A meta-command result begins with:

```text
ok sections=<N> elapsed_ms=<MS>
section <TITLE>
ok rows=<FULL_COUNT> returned=<EMITTED_COUNT> truncated=<true|false>
...
```

Sections are separated by a blank line. Header/title values containing unsafe
characters are JSON-quoted.

### Column JSON

Each output record is compact JSON followed by one newline. A row result is:

```json
{"ok":true,"kind":"rows","columns":[["id","int8"],["active","bool"]],"rows":[[1,true]],"row_count":1,"returned_rows":1,"truncated":false,"elapsed_ms":8}
```

`columns` preserves order as `[name, PostgreSQL type]` pairs; each row is a
same-order array. Null becomes JSON `null`. Boolean, integer, float, JSON, and
JSONB values become native JSON when conversion succeeds; all other values and
failed conversions remain strings.

Other success schemas are:

```json
{"ok":true,"kind":"rows_affected","rows_affected":3,"elapsed_ms":4}
{"ok":true,"kind":"status","status":"committed","elapsed_ms":4}
{"ok":true,"kind":"meta","sections":[{"title":"Tables","result":{"ok":true,"kind":"rows","columns":[],"rows":[],"row_count":0,"returned_rows":0,"truncated":false,"elapsed_ms":0}}],"elapsed_ms":4}
```

### Errors and named-run status

Compact non-SQL errors use:

```text
error message=<VALUE>
```

PostgreSQL errors use `sqlstate`, `severity`, and `message`, followed when
available by `position=<line>:<column>`, `detail`, `hint`, and `friendly_hint`.
Column JSON errors use `{"ok":false,"error":{...}}`; unavailable optional SQL
fields are JSON null.

A failed named run emits the error and then a second status record. The status
is `rolled_back` when DBCrab successfully rolled back its transaction, otherwise
`failed`:

```text
error message=...
error status=rolled_back
```

Column JSON represents that second record as
`{"ok":false,"kind":"status","status":"rolled_back"}`. A successful
read-only named run ends with `completed`; a successful write-enabled run ends
with `committed`.
