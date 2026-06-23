use std::borrow::Cow;

use reedline::{Prompt, PromptEditMode, PromptHistorySearch};

use crate::render::{DisplayMode, DisplayModeState};

pub struct DbPrompt {
    display_mode: DisplayModeState,
}

pub struct CommandPrompt;

impl DbPrompt {
    pub fn new(display_mode: DisplayModeState) -> Self {
        Self { display_mode }
    }
}

impl Prompt for DbPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        match self.display_mode.get() {
            DisplayMode::Auto => Cow::Borrowed(""),
            mode => Cow::Owned(format!("[{}]", mode.label())),
        }
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("  ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        _history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Borrowed("_ ")
    }
}

impl Prompt for CommandPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed(": ")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("  ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        _history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Borrowed(": ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_prompt_hides_auto_display_mode() {
        // Given
        let prompt = DbPrompt::new(DisplayModeState::new());

        // When
        let right = prompt.render_prompt_right();

        // Then
        assert_eq!(right, "");
    }

    #[test]
    fn right_prompt_shows_non_auto_display_mode() {
        // Given
        let display_mode = DisplayModeState::new();
        display_mode.set(DisplayMode::Full);
        let prompt = DbPrompt::new(display_mode);

        // When
        let right = prompt.render_prompt_right();

        // Then
        assert_eq!(right, "[full]");
    }

    #[test]
    fn command_prompt_uses_colon_indicator() {
        // Given
        let prompt = CommandPrompt;

        // When
        let indicator = prompt.render_prompt_indicator(PromptEditMode::Default);

        // Then
        assert_eq!(indicator, ": ");
    }

    #[test]
    fn command_prompt_hides_display_mode() {
        // Given
        let prompt = CommandPrompt;

        // When
        let right = prompt.render_prompt_right();

        // Then
        assert_eq!(right, "");
    }
}
