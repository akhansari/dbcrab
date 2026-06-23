pub const SQL_KEYWORDS: &[&str] = &[
    "ABORT",
    "ALTER",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "COMMIT",
    "CONFLICT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "RIGHT",
    "ROLLBACK",
    "SELECT",
    "SET",
    "SHOW",
    "TABLE",
    "THEN",
    "TRUE",
    "TRUNCATE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WITH",
];

pub fn is_sql_keyword(word: &str) -> bool {
    let uppercase = word.to_ascii_uppercase();
    SQL_KEYWORDS
        .binary_search_by(|candidate| candidate.as_bytes().cmp(uppercase.as_bytes()))
        .is_ok()
}

pub fn split_complete_statements(input: &str) -> (Vec<String>, String) {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut state = ScanState::Normal;
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match &mut state {
            ScanState::Normal => match ch {
                ';' => {
                    let statement = input[start..=idx].trim();
                    if !statement.is_empty() {
                        statements.push(statement.to_owned());
                    }
                    start = idx + ch.len_utf8();
                }
                '\'' => state = ScanState::SingleQuoted,
                '"' => state = ScanState::DoubleQuoted,
                '-' if next_char(&mut chars) == Some('-') => {
                    chars.next();
                    state = ScanState::LineComment;
                }
                '/' if next_char(&mut chars) == Some('*') => {
                    chars.next();
                    state = ScanState::BlockComment { depth: 1 };
                }
                '$' => {
                    if let Some(tag) = dollar_quote_tag(input, idx) {
                        for _ in 0..tag.len().saturating_sub(1) {
                            chars.next();
                        }
                        state = ScanState::DollarQuoted { tag };
                    }
                }
                _ => {}
            },
            ScanState::SingleQuoted => {
                if ch == '\'' {
                    if next_char(&mut chars) == Some('\'') {
                        chars.next();
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::DoubleQuoted => {
                if ch == '"' {
                    if next_char(&mut chars) == Some('"') {
                        chars.next();
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::LineComment => {
                if ch == '\n' {
                    state = ScanState::Normal;
                }
            }
            ScanState::BlockComment { depth } => match ch {
                '/' if next_char(&mut chars) == Some('*') => {
                    chars.next();
                    *depth += 1;
                }
                '*' if next_char(&mut chars) == Some('/') => {
                    chars.next();
                    *depth -= 1;
                    if *depth == 0 {
                        state = ScanState::Normal;
                    }
                }
                _ => {}
            },
            ScanState::DollarQuoted { tag } => {
                if input[idx..].starts_with(tag.as_str()) {
                    for _ in 0..tag.len().saturating_sub(1) {
                        chars.next();
                    }
                    state = ScanState::Normal;
                }
            }
        }
    }

    (statements, input[start..].to_owned())
}

pub fn likely_returns_rows(statement: &str) -> bool {
    let Some(keyword) = first_keyword(statement) else {
        return false;
    };

    matches!(
        keyword.as_str(),
        "select" | "with" | "show" | "values" | "table" | "explain"
    ) || contains_returning(statement)
}

pub fn first_keyword(statement: &str) -> Option<String> {
    let mut chars = statement.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }

        if ch == '-' && next_char(&mut chars) == Some('-') {
            for (_, comment_ch) in chars.by_ref() {
                if comment_ch == '\n' {
                    break;
                }
            }
            continue;
        }

        if ch == '/' && next_char(&mut chars) == Some('*') {
            chars.next();
            let mut depth = 1;
            while let Some((_, comment_ch)) = chars.next() {
                match comment_ch {
                    '/' if next_char(&mut chars) == Some('*') => {
                        chars.next();
                        depth += 1;
                    }
                    '*' if next_char(&mut chars) == Some('/') => {
                        chars.next();
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        if is_identifier_start(ch) {
            let end = statement[idx..]
                .char_indices()
                .take_while(|(_, c)| is_identifier_continue(*c))
                .last()
                .map_or(idx + ch.len_utf8(), |(offset, c)| {
                    idx + offset + c.len_utf8()
                });
            return Some(statement[idx..end].to_ascii_lowercase());
        }

        return None;
    }

    None
}

pub fn command_status(statement: &str, rows_affected: u64) -> String {
    match first_keyword(statement).as_deref() {
        Some("insert") => format!("INSERT {rows_affected}"),
        Some("update") => format!("UPDATE {rows_affected}"),
        Some("delete") => format!("DELETE {rows_affected}"),
        Some("copy") => format!("COPY {rows_affected}"),
        Some("move") => format!("MOVE {rows_affected}"),
        Some("fetch") => format!("FETCH {rows_affected}"),
        Some("select") => format!("SELECT {rows_affected}"),
        _ if rows_affected > 0 => format!("OK {rows_affected}"),
        _ => "OK".to_owned(),
    }
}

fn contains_returning(statement: &str) -> bool {
    let mut word = String::new();
    let mut state = ScanState::Normal;
    let mut chars = statement.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match &mut state {
            ScanState::Normal => match ch {
                '\'' => {
                    word.clear();
                    state = ScanState::SingleQuoted;
                }
                '"' => {
                    word.clear();
                    state = ScanState::DoubleQuoted;
                }
                '-' if next_char(&mut chars) == Some('-') => {
                    word.clear();
                    chars.next();
                    state = ScanState::LineComment;
                }
                '/' if next_char(&mut chars) == Some('*') => {
                    word.clear();
                    chars.next();
                    state = ScanState::BlockComment { depth: 1 };
                }
                '$' => {
                    if let Some(tag) = dollar_quote_tag(statement, idx) {
                        word.clear();
                        for _ in 0..tag.len().saturating_sub(1) {
                            chars.next();
                        }
                        state = ScanState::DollarQuoted { tag };
                    }
                }
                _ if is_identifier_continue(ch) => word.push(ch.to_ascii_lowercase()),
                _ => {
                    if word == "returning" {
                        return true;
                    }
                    word.clear();
                }
            },
            ScanState::SingleQuoted => {
                if ch == '\'' {
                    if next_char(&mut chars) == Some('\'') {
                        chars.next();
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::DoubleQuoted => {
                if ch == '"' {
                    if next_char(&mut chars) == Some('"') {
                        chars.next();
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::LineComment => {
                if ch == '\n' {
                    state = ScanState::Normal;
                }
            }
            ScanState::BlockComment { depth } => match ch {
                '/' if next_char(&mut chars) == Some('*') => {
                    chars.next();
                    *depth += 1;
                }
                '*' if next_char(&mut chars) == Some('/') => {
                    chars.next();
                    *depth -= 1;
                    if *depth == 0 {
                        state = ScanState::Normal;
                    }
                }
                _ => {}
            },
            ScanState::DollarQuoted { tag } => {
                if statement[idx..].starts_with(tag.as_str()) {
                    for _ in 0..tag.len().saturating_sub(1) {
                        chars.next();
                    }
                    state = ScanState::Normal;
                }
            }
        }
    }

    word == "returning"
}

#[derive(Debug, Clone)]
enum ScanState {
    Normal,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment { depth: usize },
    DollarQuoted { tag: String },
}

fn next_char(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<char> {
    chars.peek().map(|(_, ch)| *ch)
}

fn dollar_quote_tag(input: &str, start: usize) -> Option<String> {
    let rest = &input[start..];
    if !rest.starts_with('$') {
        return None;
    }

    let mut end = 1;
    for ch in rest[1..].chars() {
        if ch == '$' {
            return Some(rest[..=end].to_owned());
        }
        if !(ch.is_ascii_alphanumeric() || ch == '_') {
            return None;
        }
        end += ch.len_utf8();
    }

    None
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_complete_statements_ignores_semicolons_inside_strings() {
        // Given
        let sql = "select ';'; select 2";

        // When
        let (statements, rest) = split_complete_statements(sql);

        // Then
        assert_eq!(statements, vec!["select ';';"]);
        assert_eq!(rest, " select 2");
    }

    #[test]
    fn split_complete_statements_ignores_semicolons_inside_dollar_quotes() {
        // Given
        let sql = "select $$;$$; select 2;";

        // When
        let (statements, rest) = split_complete_statements(sql);

        // Then
        assert_eq!(statements, vec!["select $$;$$;", "select 2;"]);
        assert_eq!(rest, "");
    }

    #[test]
    fn split_complete_statements_keeps_incomplete_trailing_statement() {
        // Given
        let sql = "select 1;\nselect 2";

        // When
        let (statements, rest) = split_complete_statements(sql);

        // Then
        assert_eq!(statements, vec!["select 1;"]);
        assert_eq!(rest, "\nselect 2");
    }

    #[test]
    fn likely_returns_rows_detects_returning_clause() {
        // Given
        let statement = "insert into users(name) values ('a') returning id;";

        // When
        let returns_rows = likely_returns_rows(statement);

        // Then
        assert!(returns_rows);
    }

    #[test]
    fn command_status_formats_dml_counts() {
        // Given
        let statement = "update users set active = true;";

        // When
        let status = command_status(statement, 3);

        // Then
        assert_eq!(status, "UPDATE 3");
    }
}
