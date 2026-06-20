use reedline::{ValidationResult, Validator};

use crate::sql::split_complete_statements;

pub struct SqlValidator;

impl Validator for SqlValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if is_complete_sql_input(line) {
            ValidationResult::Complete
        } else {
            ValidationResult::Incomplete
        }
    }
}

pub fn is_complete_sql_input(input: &str) -> bool {
    if input.trim().is_empty() {
        return true;
    }

    let (statements, rest) = split_complete_statements(input);
    !statements.is_empty() && rest.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_complete() {
        // Given
        let input = "   ";

        // When
        let complete = is_complete_sql_input(input);

        // Then
        assert!(complete);
    }

    #[test]
    fn input_without_semicolon_is_incomplete() {
        // Given
        let input = "select 1";

        // When
        let complete = is_complete_sql_input(input);

        // Then
        assert!(!complete);
    }

    #[test]
    fn input_with_trailing_incomplete_statement_is_incomplete() {
        // Given
        let input = "select 1;\nselect 2";

        // When
        let complete = is_complete_sql_input(input);

        // Then
        assert!(!complete);
    }

    #[test]
    fn semicolon_inside_string_does_not_complete_input() {
        // Given
        let input = "select ';'";

        // When
        let complete = is_complete_sql_input(input);

        // Then
        assert!(!complete);
    }

    #[test]
    fn semicolon_terminated_multiline_statement_is_complete() {
        // Given
        let input = "select *\nfrom users\nwhere id = 1;";

        // When
        let complete = is_complete_sql_input(input);

        // Then
        assert!(complete);
    }
}
