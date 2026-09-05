use nu_ansi_term::{Color, Style};
use reedline::{AbbrExpandContext, Highlighter, StyledText};

use crate::sql::is_sql_keyword;

pub struct SqlHighlighter;

impl Highlighter for SqlHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        highlight_sql(line)
    }

    fn should_expand_abbr(&self, line: &str, cursor: usize, _context: AbbrExpandContext) -> bool {
        !is_inside_string(line, cursor.min(line.len()))
    }
}

fn highlight_sql(line: &str) -> StyledText {
    let mut styled = StyledText::new();
    let mut cursor = 0;
    let mut chars = line.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if idx > cursor {
            push(&mut styled, Style::new(), &line[cursor..idx]);
        }

        match ch {
            '\'' => {
                let end = consume_single_quote(line, idx, &mut chars);
                push(&mut styled, Style::new().fg(Color::Yellow), &line[idx..end]);
                cursor = end;
            }
            '"' => {
                let end = consume_double_quote(line, idx, &mut chars);
                push(&mut styled, Style::new().fg(Color::Cyan), &line[idx..end]);
                cursor = end;
            }
            '-' if peek_char(&mut chars) == Some('-') => {
                let end = line[idx..]
                    .find('\n')
                    .map_or(line.len(), |offset| idx + offset);
                push(&mut styled, Style::new().fg(Color::Green), &line[idx..end]);
                skip_until(&mut chars, end);
                cursor = end;
            }
            '/' if peek_char(&mut chars) == Some('*') => {
                chars.next();
                let end = consume_block_comment(line, idx, &mut chars);
                push(&mut styled, Style::new().fg(Color::Green), &line[idx..end]);
                cursor = end;
            }
            '$' if dollar_quote_tag(line, idx).is_some() => {
                let tag = dollar_quote_tag(line, idx).expect("tag was checked");
                let end = consume_dollar_quote(line, idx, &tag);
                push(&mut styled, Style::new().fg(Color::Yellow), &line[idx..end]);
                skip_until(&mut chars, end);
                cursor = end;
            }
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let end = consume_word(line, idx, ch, &mut chars);
                let word = &line[idx..end];
                let style = if is_sql_keyword(word) {
                    Style::new().bold().fg(Color::Purple)
                } else {
                    Style::new()
                };
                push(&mut styled, style, word);
                cursor = end;
            }
            ch if ch.is_ascii_digit() => {
                let end = consume_number(line, idx, ch, &mut chars);
                push(&mut styled, Style::new().fg(Color::Blue), &line[idx..end]);
                cursor = end;
            }
            _ => {
                let end = idx + ch.len_utf8();
                push(&mut styled, Style::new(), &line[idx..end]);
                cursor = end;
            }
        }
    }

    if cursor < line.len() {
        push(&mut styled, Style::new(), &line[cursor..]);
    }

    styled
}

fn push(styled: &mut StyledText, style: Style, text: &str) {
    if !text.is_empty() {
        styled.push((style, text.to_owned()));
    }
}

fn is_inside_string(line: &str, cursor: usize) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = line[..cursor].char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        match ch {
            '\'' if !in_double => {
                if in_single && peek_char(&mut chars) == Some('\'') {
                    chars.next();
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single => {
                if in_double && peek_char(&mut chars) == Some('"') {
                    chars.next();
                } else {
                    in_double = !in_double;
                }
            }
            _ => {}
        }
    }

    in_single || in_double
}

fn consume_single_quote(
    line: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    while let Some((idx, ch)) = chars.next() {
        if ch == '\'' {
            if peek_char(chars) == Some('\'') {
                chars.next();
            } else {
                return idx + ch.len_utf8();
            }
        }
    }
    line.len().max(start + 1)
}

fn consume_double_quote(
    line: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    while let Some((idx, ch)) = chars.next() {
        if ch == '"' {
            if peek_char(chars) == Some('"') {
                chars.next();
            } else {
                return idx + ch.len_utf8();
            }
        }
    }
    line.len().max(start + 1)
}

fn consume_block_comment(
    line: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    while let Some((idx, ch)) = chars.next() {
        if ch == '*' && peek_char(chars) == Some('/') {
            chars.next();
            return idx + 2;
        }
    }
    line.len().max(start + 2)
}

fn consume_word(
    line: &str,
    start: usize,
    first: char,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut end = start + first.len_utf8();
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            chars.next();
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    end.min(line.len())
}

fn consume_number(
    line: &str,
    start: usize,
    first: char,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut end = start + first.len_utf8();
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() || ch == '.' {
            chars.next();
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    end.min(line.len())
}

fn consume_dollar_quote(line: &str, start: usize, tag: &str) -> usize {
    let content_start = start + tag.len();
    line[content_start..]
        .find(tag)
        .map_or(line.len(), |offset| content_start + offset + tag.len())
}

fn dollar_quote_tag(line: &str, start: usize) -> Option<String> {
    let rest = &line[start..];
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

fn skip_until(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, end: usize) {
    while matches!(chars.peek(), Some((idx, _)) if *idx < end) {
        chars.next();
    }
}

fn peek_char(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<char> {
    chars.peek().map(|(_, ch)| *ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_highlighting(styled: &StyledText, expected: &[(Style, &str)]) {
        let actual: Vec<_> = styled
            .buffer
            .iter()
            .flat_map(|(style, text)| text.chars().map(move |ch| (*style, ch)))
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .flat_map(|(style, text)| text.chars().map(move |ch| (*style, ch)))
            .collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn sql_highlighting_resumes_after_a_line_comment() {
        // Given
        let sql = "-- a comment\nselect 1;";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().fg(Color::Green), "-- a comment"),
                (Style::new(), "\n"),
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (Style::new().fg(Color::Blue), "1"),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn sql_highlighting_resumes_after_an_inline_comment() {
        // Given
        let sql = "select 1; -- a comment\nselect 2;";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (Style::new().fg(Color::Blue), "1"),
                (Style::new(), "; "),
                (Style::new().fg(Color::Green), "-- a comment"),
                (Style::new(), "\n"),
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (Style::new().fg(Color::Blue), "2"),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn sql_highlighting_resumes_after_consecutive_line_comments() {
        // Given
        let sql = "-- first\n-- second\nselect 1;";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().fg(Color::Green), "-- first"),
                (Style::new(), "\n"),
                (Style::new().fg(Color::Green), "-- second"),
                (Style::new(), "\n"),
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (Style::new().fg(Color::Blue), "1"),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn a_line_comment_without_a_newline_extends_to_the_end() {
        // Given
        let sql = "-- select 'unfinished";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[(Style::new().fg(Color::Green), "-- select 'unfinished")],
        );
    }

    #[test]
    fn unicode_comments_and_crlf_line_endings_are_preserved() {
        // Given
        let sql = "-- café 🦀\r\nselect 1;";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().fg(Color::Green), "-- café 🦀\r"),
                (Style::new(), "\n"),
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (Style::new().fg(Color::Blue), "1"),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn comment_markers_inside_a_string_keep_string_highlighting() {
        // Given
        let sql = "select '-- a comment\nstill a string';";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (
                    Style::new().fg(Color::Yellow),
                    "'-- a comment\nstill a string'",
                ),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn comment_markers_inside_a_quoted_identifier_keep_identifier_highlighting() {
        // Given
        let sql = "select \"-- a comment\nstill an identifier\";";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (
                    Style::new().fg(Color::Cyan),
                    "\"-- a comment\nstill an identifier\"",
                ),
                (Style::new(), ";"),
            ],
        );
    }

    #[test]
    fn comment_markers_inside_a_dollar_quoted_string_keep_string_highlighting() {
        // Given
        let sql = "select $tag$-- a comment\nstill a string$tag$;";

        // When
        let styled = SqlHighlighter.highlight(sql, sql.len());

        // Then
        assert_highlighting(
            &styled,
            &[
                (Style::new().bold().fg(Color::Purple), "select"),
                (Style::new(), " "),
                (
                    Style::new().fg(Color::Yellow),
                    "$tag$-- a comment\nstill a string$tag$",
                ),
                (Style::new(), ";"),
            ],
        );
    }
}
