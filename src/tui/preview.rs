use ansi_to_tui::IntoText as _;
use ratatui::text::Text;

use crate::render::format_json_value;

#[derive(Debug, Clone)]
pub(super) struct PreviewContent {
    pub(super) text: Text<'static>,
    pub(super) line_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct PreviewCache {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) width: u16,
    pub(super) editing: bool,
    pub(super) content: PreviewContent,
}

pub(super) fn preview_content_for_value(
    value: &str,
    type_name: &str,
    content_width: u16,
) -> PreviewContent {
    let text = preview_text(value, type_name);
    let line_count = wrapped_line_count(&text, content_width.max(1));

    PreviewContent { text, line_count }
}

fn wrapped_line_count(text: &Text<'_>, width: u16) -> usize {
    let width = usize::from(width).max(1);
    text.lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

pub(super) fn preview_text(value: &str, type_name: &str) -> Text<'static> {
    if is_json_type(type_name)
        && let Ok(json) = serde_json::from_str(value)
        && let Ok(text) = format_json_value(json).into_text()
    {
        return text;
    }

    Text::from(value.to_owned())
}

pub(super) fn preview_plain_text(value: &str, type_name: &str) -> String {
    if is_json_type(type_name)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(value)
    {
        return serde_json::to_string_pretty(&json).unwrap_or_else(|_| value.to_owned());
    }

    value.to_owned()
}

fn is_json_type(type_name: &str) -> bool {
    matches!(type_name.to_ascii_lowercase().as_str(), "json" | "jsonb")
}
