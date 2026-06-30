use ratatui::{style::Style, text::Line};
use ratatui_textarea::{DataCursor, TextArea as EditTextArea, WrapMode};

#[derive(Debug, Clone)]
pub(super) struct PreviewEdit {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) textarea: EditTextArea<'static>,
    pub(super) text: String,
}

impl PreviewEdit {
    pub(super) fn new(row: usize, column: usize, text: String) -> Self {
        let mut textarea = EditTextArea::new(text.split('\n').map(ToOwned::to_owned).collect());
        textarea.set_wrap_mode(WrapMode::Glyph);
        textarea.set_cursor_line_style(Style::default());

        Self {
            row,
            column,
            textarea,
            text,
        }
    }

    pub(super) fn sync_text(&mut self) {
        self.text = self.textarea.lines().join("\n");
    }

    pub(super) fn cursor_visual_position(&self, width: u16) -> (usize, usize) {
        textarea_cursor_visual_position(&self.textarea, usize::from(width).max(1))
    }
}

fn textarea_cursor_visual_position(textarea: &EditTextArea<'_>, width: usize) -> (usize, usize) {
    let DataCursor(cursor_row, cursor_col) = textarea.cursor();
    let lines = textarea.lines();
    let visual_y = lines
        .iter()
        .take(cursor_row)
        .map(|line| visual_line_count(line, width))
        .sum::<usize>();
    let line = lines.get(cursor_row).map_or("", String::as_str);
    let prefix_width = line_prefix_width(line, cursor_col);

    (prefix_width % width, visual_y + prefix_width / width)
}

fn visual_line_count(line: &str, width: usize) -> usize {
    Line::from(line).width().max(1).div_ceil(width)
}

fn line_prefix_width(line: &str, char_offset: usize) -> usize {
    let byte_offset = line
        .char_indices()
        .nth(char_offset)
        .map_or(line.len(), |(offset, _)| offset);

    Line::from(&line[..byte_offset]).width()
}
