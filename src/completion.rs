use std::collections::{HashMap, HashSet};

use nu_ansi_term::{Color, Style};
use reedline::{Completer, Span, Suggestion};
use sqlparser::{
    dialect::PostgreSqlDialect,
    keywords::Keyword,
    tokenizer::{Token, Tokenizer},
};

use crate::{
    catalog::{
        Catalog, ColumnInfo, TableInfo, identifier_matches_prefix, normalize_typed_identifier,
        quote_identifier,
    },
    sql::SQL_KEYWORDS,
};

#[derive(Clone)]
pub struct SqlCompleter {
    catalog: Catalog,
}

impl SqlCompleter {
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog }
    }

    #[cfg(test)]
    pub fn suggestion_values(&self, line: &str, pos: usize) -> Vec<String> {
        self.suggestions(line, pos)
            .into_iter()
            .map(|suggestion| suggestion.value)
            .collect()
    }

    fn suggestions(&self, line: &str, pos: usize) -> Vec<Suggestion> {
        let input = CompletionInput::new(line, pos);
        let aliases = extract_aliases(line);

        let candidates = if let Some(qualifier) = input.qualifier.as_deref() {
            self.qualified_candidates(qualifier, &input.prefix, &aliases)
        } else {
            match infer_context(line, input.replacement_span.start) {
                CompletionContext::Relation => self.relation_candidates(&input.prefix),
                CompletionContext::Column => self.column_candidates(&input.prefix),
                CompletionContext::Broad => self.broad_candidates(&input.prefix),
            }
        };

        build_suggestions(candidates, input.replacement_span)
    }

    fn qualified_candidates(
        &self,
        qualifier: &str,
        prefix: &str,
        aliases: &HashMap<String, TableRef>,
    ) -> Vec<Candidate> {
        let normalized = normalize_typed_identifier(qualifier);

        if let Some(table_ref) = aliases.get(&normalized) {
            return self
                .catalog
                .columns_for_table(table_ref.schema.as_deref(), &table_ref.table)
                .into_iter()
                .filter(|column| identifier_matches_prefix(&column.name, prefix))
                .map(column_candidate)
                .collect();
        }

        if self.catalog.schema_exists(&normalized) {
            return self
                .catalog
                .tables_in_schema(&normalized)
                .into_iter()
                .filter(|table| identifier_matches_prefix(&table.name, prefix))
                .map(table_candidate)
                .collect();
        }

        if let Some(table) = self.catalog.find_table(None, &normalized) {
            return self
                .catalog
                .columns_for_table(Some(&table.schema), &table.name)
                .into_iter()
                .filter(|column| identifier_matches_prefix(&column.name, prefix))
                .map(column_candidate)
                .collect();
        }

        self.broad_candidates(prefix)
    }

    fn relation_candidates(&self, prefix: &str) -> Vec<Candidate> {
        let schema_candidates = self
            .catalog
            .schemas()
            .iter()
            .filter(|schema| identifier_matches_prefix(schema, prefix))
            .map(|schema| Candidate {
                value: format!("{}.", quote_identifier(schema)),
                description: Some("schema".into()),
                append_whitespace: false,
                style: Some(Style::new().fg(Color::Cyan)),
            });

        let table_candidates = self
            .catalog
            .tables()
            .iter()
            .filter(|table| identifier_matches_prefix(&table.name, prefix))
            .map(table_candidate);

        schema_candidates.chain(table_candidates).collect()
    }

    fn column_candidates(&self, prefix: &str) -> Vec<Candidate> {
        self.catalog
            .columns()
            .iter()
            .filter(|column| identifier_matches_prefix(&column.name, prefix))
            .map(column_candidate)
            .collect()
    }

    fn broad_candidates(&self, prefix: &str) -> Vec<Candidate> {
        let mut candidates = keyword_candidates(prefix);
        candidates.extend(self.relation_candidates(prefix));
        candidates.extend(self.column_candidates(prefix));
        candidates
    }
}

impl Completer for SqlCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.suggestions(line, pos)
    }
}

#[derive(Debug, Clone)]
struct CompletionInput {
    prefix: String,
    qualifier: Option<String>,
    replacement_span: Span,
}

impl CompletionInput {
    fn new(line: &str, pos: usize) -> Self {
        let pos = pos.min(line.len());
        let token_start = current_token_start(line, pos);
        let token = &line[token_start..pos];

        if let Some(dot) = last_unquoted_dot(token) {
            Self {
                qualifier: Some(token[..dot].to_owned()),
                prefix: token[dot + 1..].to_owned(),
                replacement_span: Span::new(token_start + dot + 1, pos),
            }
        } else {
            Self {
                qualifier: None,
                prefix: token.to_owned(),
                replacement_span: Span::new(token_start, pos),
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    value: String,
    description: Option<String>,
    append_whitespace: bool,
    style: Option<Style>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableRef {
    schema: Option<String>,
    table: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionContext {
    Relation,
    Column,
    Broad,
}

fn build_suggestions(candidates: Vec<Candidate>, span: Span) -> Vec<Suggestion> {
    let mut seen = HashSet::new();
    let mut suggestions = candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.value.clone()))
        .map(|candidate| Suggestion {
            value: candidate.value,
            display_override: None,
            description: candidate.description,
            style: candidate.style,
            extra: None,
            span,
            append_whitespace: candidate.append_whitespace,
            match_indices: None,
        })
        .collect::<Vec<_>>();

    suggestions.sort_by(|left, right| left.value.cmp(&right.value));
    suggestions
}

fn keyword_candidates(prefix: &str) -> Vec<Candidate> {
    SQL_KEYWORDS
        .iter()
        .filter(|keyword| keyword.starts_with(&prefix.to_ascii_uppercase()))
        .map(|keyword| Candidate {
            value: keyword.to_ascii_lowercase(),
            description: Some("keyword".into()),
            append_whitespace: true,
            style: Some(Style::new().fg(Color::Purple)),
        })
        .collect()
}

fn table_candidate(table: &TableInfo) -> Candidate {
    Candidate {
        value: quote_identifier(&table.name),
        description: Some(format!("relation {}.{}", table.schema, table.kind)),
        append_whitespace: true,
        style: Some(Style::new().fg(Color::Green)),
    }
}

fn column_candidate(column: &ColumnInfo) -> Candidate {
    Candidate {
        value: quote_identifier(&column.name),
        description: Some(format!(
            "column {}.{} ({})",
            column.table, column.name, column.data_type
        )),
        append_whitespace: false,
        style: Some(Style::new().fg(Color::Yellow)),
    }
}

fn infer_context(line: &str, pos: usize) -> CompletionContext {
    let before_cursor = &line[..pos.min(line.len())];
    let words = significant_words(before_cursor);
    let Some(last) = words.last().map(String::as_str) else {
        return CompletionContext::Broad;
    };

    if matches!(
        last,
        "from" | "join" | "into" | "update" | "table" | "truncate" | "describe"
    ) {
        return CompletionContext::Relation;
    }

    if matches!(
        last,
        "select" | "where" | "on" | "by" | "having" | "returning" | "set"
    ) {
        return CompletionContext::Column;
    }

    CompletionContext::Broad
}

fn significant_words(sql: &str) -> Vec<String> {
    let dialect = PostgreSqlDialect {};
    let Ok(tokens) = Tokenizer::new(&dialect, sql).tokenize() else {
        return Vec::new();
    };

    tokens
        .into_iter()
        .filter_map(|token| match token {
            Token::Word(word) => Some(word.value.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn extract_aliases(sql: &str) -> HashMap<String, TableRef> {
    let dialect = PostgreSqlDialect {};
    let Ok(tokens) = Tokenizer::new(&dialect, sql).tokenize() else {
        return HashMap::new();
    };
    let tokens = tokens
        .into_iter()
        .filter(is_significant_token)
        .collect::<Vec<_>>();
    let mut aliases = HashMap::new();
    let mut index = 0;
    let mut depth: usize = 0;

    while index < tokens.len() {
        match &tokens[index] {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Word(word) if depth == 0 && starts_table_reference(word.keyword) => {
                if let Some((table_ref, alias, next_index)) =
                    parse_table_reference(&tokens, index + 1)
                {
                    aliases.insert(table_ref.table.to_ascii_lowercase(), table_ref.clone());
                    if let Some(alias) = alias {
                        aliases.insert(alias.to_ascii_lowercase(), table_ref);
                    }
                    index = next_index;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }

    aliases
}

fn starts_table_reference(keyword: Keyword) -> bool {
    matches!(
        keyword,
        Keyword::FROM | Keyword::JOIN | Keyword::UPDATE | Keyword::INTO
    )
}

fn parse_table_reference(
    tokens: &[Token],
    mut index: usize,
) -> Option<(TableRef, Option<String>, usize)> {
    while matches!(
        word_keyword(tokens.get(index)),
        Some(Keyword::ONLY | Keyword::LATERAL)
    ) {
        index += 1;
    }

    if matches!(tokens.get(index), Some(Token::LParen)) {
        let next = skip_parenthesized(tokens, index)?;
        let (alias, end) = parse_alias(tokens, next);
        return alias.map(|alias| {
            (
                TableRef {
                    schema: None,
                    table: alias.clone(),
                },
                Some(alias),
                end,
            )
        });
    }

    let (parts, next) = parse_qualified_name(tokens, index)?;
    let table = parts.last()?.to_owned();
    let schema = (parts.len() >= 2).then(|| parts[parts.len() - 2].clone());
    let (alias, end) = parse_alias(tokens, next);

    Some((TableRef { schema, table }, alias, end))
}

fn parse_qualified_name(tokens: &[Token], mut index: usize) -> Option<(Vec<String>, usize)> {
    let mut parts = Vec::new();

    while let Token::Word(word) = tokens.get(index)? {
        parts.push(word.value.clone());
        index += 1;

        if !matches!(tokens.get(index), Some(Token::Period)) {
            break;
        }
        index += 1;
    }

    (!parts.is_empty()).then_some((parts, index))
}

fn parse_alias(tokens: &[Token], mut index: usize) -> (Option<String>, usize) {
    if matches!(word_keyword(tokens.get(index)), Some(Keyword::AS)) {
        index += 1;
    }

    let Some(Token::Word(word)) = tokens.get(index) else {
        return (None, index);
    };

    if stops_alias(word.keyword) {
        return (None, index);
    }

    (Some(word.value.clone()), index + 1)
}

fn skip_parenthesized(tokens: &[Token], index: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, token) in tokens[index..].iter().enumerate() {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn word_keyword(token: Option<&Token>) -> Option<Keyword> {
    match token {
        Some(Token::Word(word)) => Some(word.keyword),
        _ => None,
    }
}

fn stops_alias(keyword: Keyword) -> bool {
    matches!(
        keyword,
        Keyword::ON
            | Keyword::USING
            | Keyword::WHERE
            | Keyword::JOIN
            | Keyword::INNER
            | Keyword::LEFT
            | Keyword::RIGHT
            | Keyword::FULL
            | Keyword::CROSS
            | Keyword::GROUP
            | Keyword::ORDER
            | Keyword::LIMIT
            | Keyword::OFFSET
            | Keyword::RETURNING
            | Keyword::SET
            | Keyword::VALUES
    )
}

fn is_significant_token(token: &Token) -> bool {
    !matches!(token, Token::Whitespace(_))
}

fn current_token_start(line: &str, pos: usize) -> usize {
    let mut start = 0;
    let mut in_double_quote = false;
    let mut chars = line[..pos].char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if in_double_quote {
            if ch == '"' {
                if matches!(chars.peek(), Some((_, '"'))) {
                    chars.next();
                } else {
                    in_double_quote = false;
                }
            }
            continue;
        }

        match ch {
            '"' => in_double_quote = true,
            '.' => {}
            ch if is_token_separator(ch) => start = idx + ch.len_utf8(),
            _ => {}
        }
    }

    start
}

fn last_unquoted_dot(token: &str) -> Option<usize> {
    let mut last_dot = None;
    let mut in_double_quote = false;
    let mut chars = token.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if in_double_quote {
            if ch == '"' {
                if matches!(chars.peek(), Some((_, '"'))) {
                    chars.next();
                } else {
                    in_double_quote = false;
                }
            }
            continue;
        }

        match ch {
            '"' => in_double_quote = true,
            '.' => last_dot = Some(idx),
            _ => {}
        }
    }

    last_dot
}

fn is_token_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '(' | ')' | ';' | '+' | '-' | '*' | '/' | '=' | '<' | '>' | '!' | '?' | ':'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{ColumnInfo, TableInfo};

    fn test_catalog() -> Catalog {
        Catalog::from_parts(
            vec!["public".into(), "Sales Data".into()],
            vec![
                TableInfo {
                    schema: "public".into(),
                    name: "users".into(),
                    kind: "r".into(),
                },
                TableInfo {
                    schema: "Sales Data".into(),
                    name: "Orders".into(),
                    kind: "r".into(),
                },
                TableInfo {
                    schema: "public".into(),
                    name: "events".into(),
                    kind: "r".into(),
                },
            ],
            vec![
                ColumnInfo {
                    schema: "public".into(),
                    table: "users".into(),
                    name: "id".into(),
                    data_type: "integer".into(),
                },
                ColumnInfo {
                    schema: "public".into(),
                    table: "users".into(),
                    name: "email".into(),
                    data_type: "text".into(),
                },
            ],
        )
    }

    #[test]
    fn completer_suggests_alias_columns_after_dot() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select u. from users u", 9);

        // Then
        assert!(suggestions.contains(&"id".to_owned()));
        assert!(suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_suggests_quoted_schema_relations() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select * from \"Sales Data\".", 27);

        // Then
        assert!(suggestions.contains(&"\"Orders\"".to_owned()));
    }

    #[test]
    fn completer_suggests_relations_after_from() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select * from us", 16);

        // Then
        assert!(suggestions.contains(&"users".to_owned()));
    }

    #[test]
    fn completer_does_not_suggest_columns_while_typing_relation_after_from() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select * from e", 15);

        // Then
        assert!(suggestions.contains(&"events".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_does_not_suggest_columns_while_typing_relation_after_join() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select * from users join e", 26);

        // Then
        assert!(suggestions.contains(&"events".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_suggests_columns_while_typing_column_after_where() {
        // Given
        let completer = SqlCompleter::new(test_catalog());

        // When
        let suggestions = completer.suggestion_values("select * from users where em", 28);

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"events".to_owned()));
    }

    #[test]
    fn extract_aliases_maps_join_aliases_to_tables() {
        // Given
        let sql = "select * from public.users u join accounts a on a.user_id = u.id";

        // When
        let aliases = extract_aliases(sql);

        // Then
        assert_eq!(
            aliases.get("u"),
            Some(&TableRef {
                schema: Some("public".into()),
                table: "users".into()
            })
        );
        assert_eq!(
            aliases.get("a"),
            Some(&TableRef {
                schema: None,
                table: "accounts".into()
            })
        );
    }
}
