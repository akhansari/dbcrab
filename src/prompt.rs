use std::borrow::Cow;

use reedline::{Prompt, PromptEditMode, PromptHistorySearch};

pub struct DbPrompt {
    _private: (),
}

impl DbPrompt {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Prompt for DbPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
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
