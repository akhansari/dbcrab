use std::collections::HashMap;

use nu_ansi_term::{Color, Style};
use reedline::{Completer, Span, Suggestion};
use sqlparser::{
    dialect::PostgreSqlDialect,
    keywords::Keyword,
    tokenizer::{Token, Tokenizer},
};

use crate::{
    catalog::{
        Catalog, ColumnInfo, SharedCatalog, TableInfo, normalize_typed_identifier, quote_identifier,
    },
    sql::SQL_KEYWORDS,
};

#[derive(Clone)]
pub struct SqlCompleter {
    catalog: SharedCatalog,
}

impl SqlCompleter {
    pub fn new(catalog: SharedCatalog) -> Self {
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
        let request = CompletionRequest::new(line.to_owned(), pos);
        let candidates = self.catalog.read().map_or_else(
            |_| keyword_candidates(&request.input),
            |catalog| candidates_for_request(&catalog, &request),
        );

        build_suggestions(candidates, request.input.replacement_span, &request.input)
    }
}

impl Completer for SqlCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.suggestions(line, pos)
    }
}

#[derive(Debug, Clone)]
struct CompletionRequest {
    input: CompletionInput,
    tokens: Vec<Token>,
    before_cursor_tokens: Vec<Token>,
    after_cursor_tokens: Vec<Token>,
}

impl CompletionRequest {
    fn new(line: String, pos: usize) -> Self {
        let pos = clamp_to_char_boundary(&line, pos);
        let input = CompletionInput::new(&line, pos);
        let before_cursor = &line[..input.replacement_span.start];
        let after_cursor = &line[input.replacement_span.end..];

        Self {
            input,
            tokens: significant_tokens(&line),
            before_cursor_tokens: significant_tokens(before_cursor),
            after_cursor_tokens: significant_tokens(after_cursor),
        }
    }
}

#[derive(Debug, Clone)]
struct CompletionInput {
    prefix: String,
    normalized_prefix: String,
    prefix_lower: String,
    prefix_upper: String,
    sort_prefix: String,
    qualifier: Option<String>,
    replacement_span: Span,
}

impl CompletionInput {
    fn new(line: &str, pos: usize) -> Self {
        let pos = clamp_to_char_boundary(line, pos);
        let token_start = current_token_start(line, pos);
        let token = &line[token_start..pos];

        if let Some(dot) = last_unquoted_dot(token) {
            Self::from_parts(
                token[dot + 1..].to_owned(),
                Some(token[..dot].to_owned()),
                Span::new(token_start + dot + 1, pos),
            )
        } else {
            Self::from_parts(token.to_owned(), None, Span::new(token_start, pos))
        }
    }

    fn from_parts(prefix: String, qualifier: Option<String>, replacement_span: Span) -> Self {
        let normalized_prefix = normalize_typed_identifier(&prefix);
        let sort_prefix = normalized_prefix.to_ascii_lowercase();
        let prefix_lower = prefix.to_ascii_lowercase();
        let prefix_upper = prefix.to_ascii_uppercase();

        Self {
            prefix,
            normalized_prefix,
            prefix_lower,
            prefix_upper,
            sort_prefix,
            qualifier,
            replacement_span,
        }
    }

    fn is_empty(&self) -> bool {
        self.prefix.is_empty()
    }

    fn is_quoted(&self) -> bool {
        self.prefix.starts_with('"')
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    value: String,
    sort_value: String,
    description: Option<String>,
    append_whitespace: bool,
    style: Option<Style>,
    kind: CandidateKind,
    sort_priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CandidateKind {
    Keyword,
    Relation,
    Column,
}

const SCOPED_COLUMN_PRIORITY: u8 = 0;
const SCOPED_RELATION_PRIORITY: u8 = 0;
const KEYWORD_PRIORITY: u8 = 10;
const RELATION_PRIORITY: u8 = 20;
const COLUMN_PRIORITY: u8 = 30;

impl Candidate {
    fn new(
        value: String,
        description: Option<String>,
        append_whitespace: bool,
        style: Option<Style>,
        kind: CandidateKind,
        sort_priority: u8,
    ) -> Self {
        let sort_value = normalized_sort_value(&value);

        Self {
            value,
            sort_value,
            description,
            append_whitespace,
            style,
            kind,
            sort_priority,
        }
    }

    fn with_sort_priority(mut self, sort_priority: u8) -> Self {
        self.sort_priority = sort_priority;
        self
    }
}

#[derive(Debug, Clone)]
struct MergedCandidate {
    candidate: Candidate,
    descriptions: Vec<String>,
}

impl MergedCandidate {
    fn new(candidate: Candidate) -> Self {
        let descriptions = candidate.description.iter().cloned().collect();

        Self {
            candidate,
            descriptions,
        }
    }

    fn merge(&mut self, candidate: Candidate) {
        push_description(&mut self.descriptions, candidate.description.clone());

        if candidate_precedence(&candidate) < candidate_precedence(&self.candidate) {
            self.candidate = candidate;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableRef {
    schema: Option<String>,
    table: String,
}

#[derive(Debug, Clone, Default)]
struct TableReferences {
    aliases: HashMap<String, TableRef>,
    visible: Vec<TableRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionContext {
    Relation,
    Column,
    Broad,
}

fn candidates_for_request(catalog: &Catalog, request: &CompletionRequest) -> Vec<Candidate> {
    let table_references = extract_table_references(&request.tokens);

    if let Some(qualifier) = request.input.qualifier.as_deref() {
        qualified_candidates(
            catalog,
            qualifier,
            &request.input,
            &table_references.aliases,
        )
    } else {
        match infer_context(request) {
            CompletionContext::Relation => relation_candidates(catalog, &request.input),
            CompletionContext::Column => {
                column_candidates(catalog, &request.input, &table_references.visible)
            }
            CompletionContext::Broad => {
                broad_candidates(catalog, &request.input, &table_references.visible)
            }
        }
    }
}

fn qualified_candidates(
    catalog: &Catalog,
    qualifier: &str,
    input: &CompletionInput,
    aliases: &HashMap<String, TableRef>,
) -> Vec<Candidate> {
    let normalized = normalize_typed_identifier(qualifier);
    let alias_key = normalized.to_ascii_lowercase();

    if let Some(table_ref) = aliases.get(&alias_key) {
        return catalog
            .columns_for_table(table_ref.schema.as_deref(), &table_ref.table)
            .into_iter()
            .filter(|column| identifier_matches_input_prefix(&column.name, input))
            .map(|column| column_candidate(column).with_sort_priority(SCOPED_COLUMN_PRIORITY))
            .collect();
    }

    if catalog.schema_exists(&normalized) {
        return catalog
            .tables_in_schema(&normalized)
            .into_iter()
            .filter(|table| identifier_matches_input_prefix(&table.name, input))
            .map(|table| table_candidate(table).with_sort_priority(SCOPED_RELATION_PRIORITY))
            .collect();
    }

    if let Some(table) = catalog.find_table(None, &normalized) {
        return catalog
            .columns_for_table(Some(&table.schema), &table.name)
            .into_iter()
            .filter(|column| identifier_matches_input_prefix(&column.name, input))
            .map(|column| column_candidate(column).with_sort_priority(SCOPED_COLUMN_PRIORITY))
            .collect();
    }

    broad_candidates(catalog, input, &[])
}

fn relation_candidates(catalog: &Catalog, input: &CompletionInput) -> Vec<Candidate> {
    let schema_candidates = catalog
        .schemas()
        .iter()
        .filter(|schema| identifier_matches_input_prefix(schema, input))
        .map(|schema| {
            Candidate::new(
                format!("{}.", quote_identifier(schema)),
                Some("schema".into()),
                false,
                Some(Style::new().fg(Color::Cyan)),
                CandidateKind::Relation,
                RELATION_PRIORITY,
            )
        });

    let table_candidates = catalog
        .tables()
        .iter()
        .filter(|table| identifier_matches_input_prefix(&table.name, input))
        .map(table_candidate);

    schema_candidates.chain(table_candidates).collect()
}

fn column_candidates(
    catalog: &Catalog,
    input: &CompletionInput,
    visible_relations: &[TableRef],
) -> Vec<Candidate> {
    let scoped_columns = visible_relations
        .iter()
        .flat_map(|table_ref| {
            catalog.columns_for_table(table_ref.schema.as_deref(), &table_ref.table)
        })
        .collect::<Vec<_>>();

    if scoped_columns.is_empty() {
        catalog
            .columns()
            .iter()
            .filter(|column| identifier_matches_input_prefix(&column.name, input))
            .map(|column| column_candidate(column).with_sort_priority(COLUMN_PRIORITY))
            .collect()
    } else {
        scoped_columns
            .into_iter()
            .filter(|column| identifier_matches_input_prefix(&column.name, input))
            .map(|column| column_candidate(column).with_sort_priority(SCOPED_COLUMN_PRIORITY))
            .collect()
    }
}

fn broad_candidates(
    catalog: &Catalog,
    input: &CompletionInput,
    visible_relations: &[TableRef],
) -> Vec<Candidate> {
    let mut candidates = keyword_candidates(input);
    candidates.extend(relation_candidates(catalog, input));
    candidates.extend(column_candidates(catalog, input, visible_relations));
    candidates
}

fn identifier_matches_input_prefix(identifier: &str, input: &CompletionInput) -> bool {
    if input.is_quoted() {
        identifier.starts_with(&input.normalized_prefix)
    } else {
        starts_with_ascii_case_insensitive(identifier, &input.prefix_lower)
    }
}

fn starts_with_ascii_case_insensitive(value: &str, lowercase_prefix: &str) -> bool {
    value
        .as_bytes()
        .get(..lowercase_prefix.len())
        .is_some_and(|candidate_prefix| {
            candidate_prefix
                .iter()
                .zip(lowercase_prefix.bytes())
                .all(|(candidate, prefix)| candidate.to_ascii_lowercase() == prefix)
        })
}

fn build_suggestions(
    candidates: Vec<Candidate>,
    span: Span,
    input: &CompletionInput,
) -> Vec<Suggestion> {
    let candidate_count = candidates.len();
    let mut indexes: HashMap<String, usize> = HashMap::with_capacity(candidate_count);
    let mut merged_candidates: Vec<MergedCandidate> = Vec::with_capacity(candidate_count);

    for candidate in candidates {
        if let Some(index) = indexes.get(&candidate.value).copied() {
            merged_candidates[index].merge(candidate);
        } else {
            indexes.insert(candidate.value.clone(), merged_candidates.len());
            merged_candidates.push(MergedCandidate::new(candidate));
        }
    }

    merged_candidates
        .sort_by(|left, right| compare_candidates(&left.candidate, &right.candidate, input));

    merged_candidates
        .into_iter()
        .map(|mut merged| {
            merged.candidate.description = merged_description(&merged.descriptions);
            merged.candidate
        })
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
        .collect()
}

fn compare_candidates(
    left: &Candidate,
    right: &Candidate,
    input: &CompletionInput,
) -> std::cmp::Ordering {
    left.sort_priority
        .cmp(&right.sort_priority)
        .then_with(|| {
            match_quality(&left.sort_value, input).cmp(&match_quality(&right.sort_value, input))
        })
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.sort_value.len().cmp(&right.sort_value.len()))
        .then_with(|| left.sort_value.cmp(&right.sort_value))
        .then_with(|| left.value.cmp(&right.value))
}

fn candidate_precedence(candidate: &Candidate) -> (u8, CandidateKind) {
    (candidate.sort_priority, candidate.kind)
}

fn match_quality(value: &str, input: &CompletionInput) -> u8 {
    if input.is_empty() {
        return 0;
    }

    if value == input.sort_prefix {
        0
    } else if value.starts_with(&input.sort_prefix) {
        1
    } else {
        2
    }
}

fn normalized_sort_value(value: &str) -> String {
    normalize_typed_identifier(value).to_ascii_lowercase()
}

fn push_description(descriptions: &mut Vec<String>, description: Option<String>) {
    if let Some(description) = description
        && !descriptions.contains(&description)
    {
        descriptions.push(description);
    }
}

fn merged_description(descriptions: &[String]) -> Option<String> {
    match descriptions {
        [] => None,
        [description] => Some(description.clone()),
        descriptions => Some(format!("multiple matches: {}", descriptions.join("; "))),
    }
}

fn keyword_candidates(input: &CompletionInput) -> Vec<Candidate> {
    SQL_KEYWORDS
        .iter()
        .filter(|keyword| keyword.starts_with(&input.prefix_upper))
        .map(|keyword| {
            Candidate::new(
                keyword.to_ascii_lowercase(),
                Some("keyword".into()),
                true,
                Some(Style::new().fg(Color::Purple)),
                CandidateKind::Keyword,
                KEYWORD_PRIORITY,
            )
        })
        .collect()
}

fn table_candidate(table: &TableInfo) -> Candidate {
    Candidate::new(
        quote_identifier(&table.name),
        Some(format!(
            "relation {}.{} ({})",
            table.schema, table.name, table.kind
        )),
        true,
        Some(Style::new().fg(Color::Green)),
        CandidateKind::Relation,
        RELATION_PRIORITY,
    )
}

fn column_candidate(column: &ColumnInfo) -> Candidate {
    Candidate::new(
        quote_identifier(&column.name),
        Some(format!(
            "column {}.{}.{} ({})",
            column.schema, column.table, column.name, column.data_type
        )),
        false,
        Some(Style::new().fg(Color::Yellow)),
        CandidateKind::Column,
        COLUMN_PRIORITY,
    )
}

fn infer_context(request: &CompletionRequest) -> CompletionContext {
    if is_insert_column_list_context(&request.before_cursor_tokens) {
        return CompletionContext::Column;
    }
    if is_relation_list_context(&request.before_cursor_tokens) {
        return CompletionContext::Relation;
    }

    let words = significant_words(&request.before_cursor_tokens);
    if let Some(context) = infer_context_after_cursor(request, &words) {
        return context;
    }

    let Some(last) = words.last().map(String::as_str) else {
        return CompletionContext::Broad;
    };

    if last == "on" && is_create_index_context(&words) {
        return CompletionContext::Relation;
    }

    if is_relation_keyword(last) {
        return CompletionContext::Relation;
    }

    if is_column_keyword(last) {
        return CompletionContext::Column;
    }

    active_clause_context(&words).unwrap_or(CompletionContext::Broad)
}

fn infer_context_after_cursor(
    request: &CompletionRequest,
    words_before_cursor: &[String],
) -> Option<CompletionContext> {
    let words_after_cursor = significant_words(&request.after_cursor_tokens);
    let first_after_cursor = words_after_cursor.first()?.as_str();

    if first_after_cursor == "from"
        && (words_before_cursor.is_empty() || active_clause_context(words_before_cursor).is_none())
        && !matches_sql_keyword_prefix(&request.input)
    {
        return Some(CompletionContext::Column);
    }

    None
}

fn matches_sql_keyword_prefix(input: &CompletionInput) -> bool {
    !input.is_empty()
        && SQL_KEYWORDS
            .iter()
            .any(|keyword| keyword.starts_with(&input.prefix_upper))
}

fn significant_words(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Word(word) => Some(word.value.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn significant_tokens(sql: &str) -> Vec<Token> {
    let dialect = PostgreSqlDialect {};
    let Ok(tokens) = Tokenizer::new(&dialect, sql).tokenize() else {
        return Vec::new();
    };

    tokens.into_iter().filter(is_significant_token).collect()
}

fn is_relation_keyword(word: &str) -> bool {
    matches!(
        word,
        "from" | "join" | "into" | "update" | "table" | "truncate" | "describe" | "references"
    )
}

fn is_column_keyword(word: &str) -> bool {
    matches!(
        word,
        "select" | "where" | "on" | "by" | "having" | "returning" | "set" | "using"
    )
}

fn active_clause_context(words: &[String]) -> Option<CompletionContext> {
    for word in words.iter().rev() {
        match word.as_str() {
            "select" | "where" | "on" | "by" | "having" | "returning" | "set" | "using" => {
                return Some(CompletionContext::Column);
            }
            "from" | "join" | "into" | "update" | "table" | "truncate" | "values" | "limit"
            | "offset" | "union" | "except" | "intersect" => {
                return Some(CompletionContext::Broad);
            }
            _ => {}
        }
    }

    None
}

fn is_create_index_context(words: &[String]) -> bool {
    words.iter().any(|word| word == "create") && words.iter().any(|word| word == "index")
}

fn is_insert_column_list_context(tokens: &[Token]) -> bool {
    let Some(insert_index) = tokens
        .iter()
        .position(|token| matches!(word_keyword(Some(token)), Some(Keyword::INSERT)))
    else {
        return false;
    };
    let Some(into_index) =
        tokens
            .iter()
            .enumerate()
            .skip(insert_index + 1)
            .find_map(|(index, token)| {
                matches!(word_keyword(Some(token)), Some(Keyword::INTO)).then_some(index)
            })
    else {
        return false;
    };

    if tokens.iter().skip(into_index + 1).any(|token| {
        matches!(
            word_keyword(Some(token)),
            Some(Keyword::SELECT | Keyword::VALUES)
        )
    }) {
        return false;
    }

    let Some(open_index) = tokens
        .iter()
        .enumerate()
        .skip(into_index + 1)
        .find_map(|(index, token)| matches!(token, Token::LParen).then_some(index))
    else {
        return false;
    };

    unmatched_parenthesis_depth(&tokens[open_index..]) > 0
}

fn unmatched_parenthesis_depth(tokens: &[Token]) -> usize {
    tokens.iter().fold(0usize, |depth, token| match token {
        Token::LParen => depth + 1,
        Token::RParen => depth.saturating_sub(1),
        _ => depth,
    })
}

fn is_relation_list_context(tokens: &[Token]) -> bool {
    if !matches!(tokens.last(), Some(Token::Comma)) {
        return false;
    }

    for token in tokens.iter().rev().skip(1) {
        let Token::Word(word) = token else {
            continue;
        };
        let value = word.value.to_ascii_lowercase();

        if matches!(value.as_str(), "from" | "join") {
            return true;
        }
        if is_column_keyword(&value) || matches!(value.as_str(), "select" | "where" | "having") {
            return false;
        }
    }

    false
}

fn extract_table_references(tokens: &[Token]) -> TableReferences {
    let mut table_references = TableReferences::default();
    let mut index = 0;
    let mut depth: usize = 0;
    let mut in_relation_list = false;

    while index < tokens.len() {
        match &tokens[index] {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Word(word) if depth == 0 && starts_table_reference(word.keyword) => {
                if let Some((table_ref, alias, next_index)) =
                    parse_table_reference(tokens, index + 1)
                {
                    insert_table_reference(&mut table_references, table_ref, alias);
                    in_relation_list = true;
                    index = next_index;
                    continue;
                }
            }
            Token::Word(_) if depth == 0 && starts_create_index_table_reference(tokens, index) => {
                if let Some((table_ref, alias, next_index)) =
                    parse_table_reference(tokens, index + 1)
                {
                    insert_table_reference(&mut table_references, table_ref, alias);
                    in_relation_list = false;
                    index = next_index;
                    continue;
                }
            }
            Token::Word(word) if depth == 0 && stops_relation_list(word.keyword) => {
                in_relation_list = false;
            }
            Token::Comma if depth == 0 && in_relation_list => {
                if let Some((table_ref, alias, next_index)) =
                    parse_table_reference(tokens, index + 1)
                {
                    insert_table_reference(&mut table_references, table_ref, alias);
                    index = next_index;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }

    table_references
}

#[cfg(test)]
fn extract_aliases(sql: &str) -> HashMap<String, TableRef> {
    extract_table_references(&significant_tokens(sql)).aliases
}

fn insert_table_reference(
    table_references: &mut TableReferences,
    table_ref: TableRef,
    alias: Option<String>,
) {
    if !table_references.visible.contains(&table_ref) {
        table_references.visible.push(table_ref.clone());
    }

    table_references
        .aliases
        .insert(alias_key(&table_ref.table), table_ref.clone());
    if let Some(alias) = alias {
        table_references
            .aliases
            .insert(alias_key(&alias), table_ref);
    }
}

fn alias_key(identifier: &str) -> String {
    normalize_typed_identifier(identifier).to_ascii_lowercase()
}

fn starts_table_reference(keyword: Keyword) -> bool {
    matches!(
        keyword,
        Keyword::FROM | Keyword::JOIN | Keyword::UPDATE | Keyword::INTO
    )
}

fn starts_create_index_table_reference(tokens: &[Token], index: usize) -> bool {
    if !matches!(word_keyword(tokens.get(index)), Some(Keyword::ON)) {
        return false;
    }

    let mut has_create = false;
    let mut has_index = false;
    for token in &tokens[..index] {
        let Some(value) = word_value(token) else {
            continue;
        };

        match value.to_ascii_lowercase().as_str() {
            "create" => has_create = true,
            "index" => has_index = true,
            _ => {}
        }
    }

    has_create && has_index
}

fn stops_relation_list(keyword: Keyword) -> bool {
    matches!(
        keyword,
        Keyword::ON
            | Keyword::USING
            | Keyword::WHERE
            | Keyword::GROUP
            | Keyword::ORDER
            | Keyword::LIMIT
            | Keyword::OFFSET
            | Keyword::RETURNING
            | Keyword::SET
            | Keyword::VALUES
            | Keyword::SELECT
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

fn word_value(token: &Token) -> Option<&str> {
    match token {
        Token::Word(word) => Some(&word.value),
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

fn clamp_to_char_boundary(line: &str, pos: usize) -> usize {
    let mut pos = pos.min(line.len());
    while !line.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn current_token_start(line: &str, pos: usize) -> usize {
    let pos = clamp_to_char_boundary(line, pos);
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
    use crate::catalog::{Catalog, ColumnInfo, TableInfo, shared_catalog};

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
                ColumnInfo {
                    schema: "public".into(),
                    table: "events".into(),
                    name: "id".into(),
                    data_type: "bigint".into(),
                },
            ],
        )
    }

    fn after_cursor_context_catalog() -> Catalog {
        Catalog::from_parts(
            vec!["public".into()],
            vec![
                TableInfo {
                    schema: "public".into(),
                    name: "users".into(),
                    kind: "r".into(),
                },
                TableInfo {
                    schema: "public".into(),
                    name: "emails".into(),
                    kind: "r".into(),
                },
            ],
            vec![ColumnInfo {
                schema: "public".into(),
                table: "users".into(),
                name: "email".into(),
                data_type: "text".into(),
            }],
        )
    }

    fn schema_qualified_catalog() -> Catalog {
        Catalog::from_parts(
            vec!["public".into(), "Sales Data".into()],
            vec![
                TableInfo {
                    schema: "public".into(),
                    name: "Orders".into(),
                    kind: "r".into(),
                },
                TableInfo {
                    schema: "Sales Data".into(),
                    name: "Orders".into(),
                    kind: "r".into(),
                },
            ],
            vec![
                ColumnInfo {
                    schema: "public".into(),
                    table: "Orders".into(),
                    name: "id".into(),
                    data_type: "integer".into(),
                },
                ColumnInfo {
                    schema: "Sales Data".into(),
                    table: "Orders".into(),
                    name: "id".into(),
                    data_type: "uuid".into(),
                },
            ],
        )
    }

    #[test]
    fn completer_orders_broad_suggestions_by_candidate_kind() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("e", 1);
        let keyword_index = suggestions
            .iter()
            .position(|suggestion| suggestion == "else")
            .expect("else keyword suggestion");
        let relation_index = suggestions
            .iter()
            .position(|suggestion| suggestion == "events")
            .expect("events relation suggestion");
        let column_index = suggestions
            .iter()
            .position(|suggestion| suggestion == "email")
            .expect("email column suggestion");

        // Then
        assert!(keyword_index < relation_index);
        assert!(relation_index < column_index);
    }

    #[test]
    fn completer_merges_duplicate_suggestion_descriptions() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "select i";

        // When
        let suggestions = completer.suggestions(line, line.len());
        let id = suggestions
            .iter()
            .find(|suggestion| suggestion.value == "id")
            .expect("id column suggestion");

        // Then
        assert_eq!(
            id.description.as_deref(),
            Some(
                "multiple matches: column public.users.id (integer); column public.events.id (bigint)"
            )
        );
    }

    #[test]
    fn completer_scopes_column_suggestions_to_visible_relations() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "select * from users where i";

        // When
        let suggestions = completer.suggestions(line, line.len());
        let id = suggestions
            .iter()
            .find(|suggestion| suggestion.value == "id")
            .expect("id column suggestion");

        // Then
        assert_eq!(
            id.description.as_deref(),
            Some("column public.users.id (integer)")
        );
    }

    #[test]
    fn completer_uses_from_after_cursor_to_infer_column_context() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(after_cursor_context_catalog()));
        let line = "em from users";

        // When
        let suggestions = completer.suggestion_values(line, 2);

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"emails".to_owned()));
    }

    #[test]
    fn completer_uses_schema_qualified_relation_after_cursor_for_columns() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(schema_qualified_catalog()));
        let line = "select i from \"Sales Data\".\"Orders\"";

        // When
        let suggestions = completer.suggestions(line, 8);
        let id = suggestions
            .iter()
            .find(|suggestion| suggestion.value == "id")
            .expect("id column suggestion");

        // Then
        assert_eq!(
            id.description.as_deref(),
            Some("column Sales Data.Orders.id (uuid)")
        );
    }

    #[test]
    fn completer_suggests_alias_columns_after_dot() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select u. from users u", 9);

        // Then
        assert!(suggestions.contains(&"id".to_owned()));
        assert!(suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_suggests_alias_columns_before_trailing_semicolon() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestions("select u. from users u;", 9);
        let id = suggestions
            .iter()
            .find(|suggestion| suggestion.value == "id")
            .expect("id column suggestion");

        // Then
        assert_eq!(
            id.description.as_deref(),
            Some("column public.users.id (integer)")
        );
        assert!(
            suggestions
                .iter()
                .any(|suggestion| suggestion.value == "email")
        );
    }

    #[test]
    fn completer_suggests_quoted_schema_relations() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select * from \"Sales Data\".", 27);

        // Then
        assert!(suggestions.contains(&"\"Orders\"".to_owned()));
    }

    #[test]
    fn completer_suggests_relations_after_from() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select * from us", 16);

        // Then
        assert!(suggestions.contains(&"users".to_owned()));
    }

    #[test]
    fn completer_does_not_suggest_columns_while_typing_relation_after_from() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select * from e", 15);

        // Then
        assert!(suggestions.contains(&"events".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_does_not_suggest_columns_while_typing_relation_after_join() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select * from users join e", 26);

        // Then
        assert!(suggestions.contains(&"events".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_suggests_columns_while_typing_column_after_where() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));

        // When
        let suggestions = completer.suggestion_values("select * from users where em", 28);

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"events".to_owned()));
    }

    #[test]
    fn completer_suggests_columns_inside_function_calls_in_column_context() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "select * from users where lower(em";

        // When
        let suggestions = completer.suggestion_values(line, line.len());

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"events".to_owned()));
    }

    #[test]
    fn completer_suggests_insert_target_columns() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "insert into users (em";

        // When
        let suggestions = completer.suggestion_values(line, line.len());

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"events".to_owned()));
    }

    #[test]
    fn completer_suggests_update_target_columns_after_set() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "update users set em";

        // When
        let suggestions = completer.suggestion_values(line, line.len());

        // Then
        assert!(suggestions.contains(&"email".to_owned()));
        assert!(!suggestions.contains(&"events".to_owned()));
    }

    #[test]
    fn completer_suggests_relations_after_create_index_on() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "create index users_email_idx on us";

        // When
        let suggestions = completer.suggestion_values(line, line.len());

        // Then
        assert!(suggestions.contains(&"users".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
    }

    #[test]
    fn completer_suggests_relations_after_from_list_comma() {
        // Given
        let completer = SqlCompleter::new(shared_catalog(test_catalog()));
        let line = "select * from users, e";

        // When
        let suggestions = completer.suggestion_values(line, line.len());

        // Then
        assert!(suggestions.contains(&"events".to_owned()));
        assert!(!suggestions.contains(&"email".to_owned()));
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
