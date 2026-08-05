---
title: Edit result rows in the TUI
description: Stage and apply guarded PostgreSQL row updates from an eligible full-screen query result.
---

## Return an editable result

Force `tui` with **Alt-V**, or let `auto` choose it, then select source columns directly from a table:

```sql
select *
from users
order by id;
```

A selected cell is editable only when all of these are true:

- The result is open in the TUI. Inline results do not load update metadata.
- PostgreSQL traces the selected column directly to an ordinary table or partitioned table.
- The result includes every column in that same table's primary key, including all parts of a composite key.
- The selected target is a non-primary-key column from that table.
- The result contains at least one row.

Views, materialized views, foreign tables, expressions, and columns without origin metadata are not editable. Primary key cells identify the row but cannot themselves be edited. The preview title adds `[editable]` when the current cell qualifies.

## Stage a change

1. Move to a non-primary-key cell.
2. Press `c`. DBCrab opens the preview, focuses it, and starts a text editor.
3. Edit the value.
4. Press **Ctrl-S** to stage the text, or **Ctrl-X** to stage SQL `NULL` for a nullable column.
5. Press **Esc** instead to cancel the current edit buffer.

You can also press **Ctrl-X** from a focused preview without first pressing `c`. A non-nullable column rejects the request.

Staged cells are bold yellow. A row with staged changes is yellow and its first visible cell starts with `*`. Stage the original value again to clear that cell's staged change. You may stage several non-primary-key cells from the same source relation and row before updating.

## Update the selected row

Return focus to the table if needed, keep the dirty row selected, and press **Ctrl-U**. DBCrab issues one `UPDATE` for that row and refreshes the changed cells from `RETURNING`.

The generated update:

- Quotes the recorded schema, relation, and column identifiers.
- Binds edited text as parameters and casts it through each source column's PostgreSQL type.
- Uses every selected primary key value as a bound `WHERE` parameter.
- Rejects mixed staged changes from different source relations, duplicate source attributes, primary-key targets, and invalid `NULL` changes.
- Reports `row not found` if the selected primary key no longer matches a row.

:::caution
Staging does not write to PostgreSQL, and closing the TUI discards staged changes. **Ctrl-U** writes immediately without a confirmation prompt or an undo operation.
:::

:::caution
Primary-key matching prevents a broad update, but this is not optimistic concurrency control. DBCrab does not compare the other original values or a row version. If another session changes the row after your query, **Ctrl-U** updates the row currently matching that primary key and can overwrite the same columns.
:::

See the [display and output reference](../../reference/display-output/#editing-result-rows) for eligibility and failure details, or [customize the TUI keys](../keybindings/).
