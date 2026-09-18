//! Case-insensitive ordered-character matching shared by both completion menus.

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Rank(u8, usize, usize, usize);

#[derive(Debug, Clone)]
pub(crate) struct Match {
    pub rank: Rank,
    pub indices: Vec<usize>,
}

pub(crate) fn find(value: &str, query: &str) -> Option<Match> {
    let query = crate::catalog::normalize_typed_identifier(query).to_lowercase();
    let span = ratatui::text::Span::raw(value);
    // Keep display grapheme positions through case expansion and SQL quoting.
    let mut chars = Vec::new();
    let quoted = value.starts_with('"');
    let mut graphemes = span
        .styled_graphemes(ratatui::style::Style::default())
        .enumerate()
        .peekable();
    while let Some((index, grapheme)) = graphemes.next() {
        if quoted && grapheme.symbol == "\"" {
            if index == 0 || graphemes.peek().is_none() {
                continue;
            }
            if graphemes
                .peek()
                .is_some_and(|(_, next)| next.symbol == "\"")
            {
                graphemes.next();
            }
        }
        for ch in grapheme.symbol.chars().flat_map(char::to_lowercase) {
            chars.push((ch, index));
        }
    }
    let query = query.chars().collect::<Vec<_>>();
    if query.is_empty() {
        return Some(Match {
            rank: Rank(0, 0, 0, 0),
            indices: Vec::new(),
        });
    }
    let mut cursor = 0;
    let mut positions = Vec::new();
    for ch in &query {
        let offset = chars[cursor..]
            .iter()
            .position(|(candidate, _)| candidate == ch)?;
        cursor += offset;
        positions.push(cursor);
        cursor += 1;
    }
    let prefix = positions.iter().copied().eq(0..query.len());
    let class = if prefix && chars.len() == query.len() {
        0
    } else if prefix {
        1
    } else {
        2
    };
    let non_boundary = positions
        .iter()
        .filter(|&&position| position != 0 && chars[position - 1].0.is_alphanumeric())
        .count();
    let gaps = positions.windows(2).map(|pair| pair[1] - pair[0] - 1).sum();
    let start = positions.first().copied().unwrap_or_default();
    let mut indices = positions
        .into_iter()
        .map(|position| chars[position].1)
        .collect::<Vec<_>>();
    indices.dedup();
    Some(Match {
        rank: Rank(class, non_boundary, gaps, start),
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::find;

    fn ranked<'a>(values: &[&'a str], query: &str) -> Vec<&'a str> {
        let mut matches = values
            .iter()
            .filter_map(|value| find(value, query).map(|matched| (matched.rank, *value)))
            .collect::<Vec<_>>();
        matches.sort();
        matches.into_iter().map(|(_, value)| value).collect()
    }

    #[test]
    fn exact_and_prefix_matches_precede_abbreviations() {
        // Given
        let values = ["user_accounts", "audit", "ua_extra", "ua"];
        // When
        let matches = ranked(&values, "UA");
        // Then
        assert_eq!(matches, ["ua", "ua_extra", "user_accounts"]);
    }

    #[test]
    fn word_starts_precede_interior_matches() {
        // Given
        let values = ["guard", "user_accounts"];
        // When
        let matches = ranked(&values, "ua");
        // Then
        assert_eq!(matches, ["user_accounts", "guard"]);
    }

    #[test]
    fn consecutive_matches_precede_gapped_matches() {
        // Given
        let values = ["x_abxc", "x_abc"];
        // When
        let matches = ranked(&values, "bc");
        // Then
        assert_eq!(matches, ["x_abc", "x_abxc"]);
    }

    #[test]
    fn highlighting_uses_grapheme_positions() {
        // Given
        let value = "\"e\u{301}_Äpfel\"";
        // When
        let matched = find(value, "äl").map(|matched| matched.indices);
        // Then
        assert_eq!(matched, Some(vec![3, 7]));
    }

    #[test]
    fn escaped_identifier_quotes_remain_literal_matches() {
        // Given
        let value = "\"a\"\"B\"";
        // When
        let matched = find(value, "\"a\"\"b").map(|matched| matched.indices);
        // Then
        assert_eq!(matched, Some(vec![1, 2, 4]));
    }

    #[test]
    fn transpositions_do_not_match() {
        // Given
        let value = "users";
        // When
        let matched = find(value, "uesrs");
        // Then
        assert!(matched.is_none());
    }
}
