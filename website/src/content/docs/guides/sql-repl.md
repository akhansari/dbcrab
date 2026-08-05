---
title: Work in the SQL REPL
description: Write, complete, run, and revisit PostgreSQL statements in DBCrab's interactive SQL editor.
---

## Run a statement

The primary prompt is `>`. End a statement with a semicolon and press Enter:

```sql
select * from users limit 5;
```

After a semicolon, you can press **Alt-Enter** to start a new line instead of submitting the input.

Until the input is complete, Enter adds another line. Continuation lines use a two-space prompt:

```sql
select *
from orders
order by 1
limit 10;
```

You can submit several complete statements together. DBCrab runs them in order:

```sql
select count(*) from users;
select count(*) from orders;
```

:::caution
Several statements in one input are not automatically one transaction. A failed statement is reported and later statements are still attempted. Use PostgreSQL transaction statements when the work must be atomic.
:::

### Semicolons inside SQL constructs

DBCrab only treats a semicolon as a statement boundary when it is outside:

- Single-quoted strings and double-quoted identifiers.
- `--` line comments.
- Nested `/* ... */` block comments.
- Dollar-quoted bodies such as `$$...$$` and `$body$...$body$`.

Quoted strings must use doubled quotes for an embedded quote. The input scanner
does not interpret PostgreSQL backslash escapes in `E'...'` strings, so use
doubled quotes or dollar quoting when such a string contains semicolons.

The final semicolon still belongs after the closing quote, comment, or dollar tag:

```sql
select $$a value; still inside the string$$ as example;
```

:::caution
Avoid a backslash-escaped quote followed by a semicolon inside `E'...'`. DBCrab
can treat that semicolon as a statement boundary before PostgreSQL parses the
escape string.
:::

## Use highlighting and completion

The SQL editor highlights keywords, strings, quoted identifiers, comments, numbers, and dollar-quoted bodies while you type.

Press **Tab** or **Ctrl-Space** to open completion. Metadata loaded at connection time provides:

- Relations after contexts such as `FROM`, `JOIN`, `UPDATE`, and `INTO`.
- Columns in contexts such as `SELECT`, `WHERE`, `ON`, `SET`, and `RETURNING`.
- Columns belonging to relations already visible in the statement.
- Schema-qualified relations after `schema.` and columns after `table.`.
- Alias-qualified columns after an alias such as `u.`.

For example, completion after `u.` is scoped to `users`:

```sql
select u.*
from users as u
limit 5;
```

After creating, dropping, or changing database objects, enter [command mode](../command-mode/) and run `refresh` to reload completion metadata.

## Correct an error

PostgreSQL errors include severity, SQLSTATE, and the server message. When available, DBCrab also shows detail, a caret at the query position, the server hint, or a metadata-based suggestion for a misspelled relation or column. Correct the input and submit it again; an error does not exit the REPL.

## Reuse SQL history

Press **Ctrl-R** to search SQL history in either Emacs or Vi mode. Multiline inputs are kept as single history entries, with up to 1,000 entries in the persistent history file.

Start an input with one space when it must not be saved to SQL history:

```sql
 select current_user;
```

Only SQL history persists. Command-mode history lasts for the current process. See [Contexts and history](../contexts-history/) to isolate persistent history by project or database.

## Choose Emacs or Vi editing

Emacs editing is the default. Set `edit-mode vi` in the user KDL configuration to use Vi insert and normal modes in both SQL and command editors. Both modes keep completion and **Ctrl-R** history search.

See [Configure DBCrab](../configuration/) for the setting, [Customize keybindings](../keybindings/) for overrides, and the [keybindings reference](../../reference/keybindings/) for the complete defaults.
