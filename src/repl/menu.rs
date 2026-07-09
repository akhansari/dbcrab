use reedline::{Completer, Editor, Menu, MenuEvent, Painter, Suggestion};

use crate::completion::SharedCompletionLineSnapshot;

pub(super) struct FullBufferCompletionMenu<M> {
    inner: M,
    completion_line: SharedCompletionLineSnapshot,
}

impl<M> FullBufferCompletionMenu<M> {
    pub(super) fn new(inner: M, completion_line: SharedCompletionLineSnapshot) -> Self {
        Self {
            inner,
            completion_line,
        }
    }

    // Reedline passes completers only `buffer[..cursor]`. SQL completion needs the
    // whole line to resolve relations and aliases that appear after the cursor.
    fn capture_editor_buffer(&self, editor: &Editor) {
        if let Ok(mut snapshot) = self.completion_line.write() {
            snapshot.update(editor.get_buffer());
        }
    }
}

impl<M: Menu> Menu for FullBufferCompletionMenu<M> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn indicator(&self) -> &str {
        self.inner.indicator()
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn menu_event(&mut self, event: MenuEvent) {
        self.inner.menu_event(event);
    }

    fn can_quick_complete(&self) -> bool {
        self.inner.can_quick_complete()
    }

    fn can_partially_complete(
        &mut self,
        values_updated: bool,
        editor: &mut Editor,
        completer: &mut dyn Completer,
    ) -> bool {
        self.capture_editor_buffer(editor);
        self.inner
            .can_partially_complete(values_updated, editor, completer)
    }

    fn update_values(&mut self, editor: &mut Editor, completer: &mut dyn Completer) {
        self.capture_editor_buffer(editor);
        self.inner.update_values(editor, completer);
    }

    fn update_working_details(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        painter: &Painter,
    ) {
        self.capture_editor_buffer(editor);
        self.inner
            .update_working_details(editor, completer, painter);
    }

    fn replace_in_buffer(&self, editor: &mut Editor) {
        self.inner.replace_in_buffer(editor);
    }

    fn menu_required_lines(&self, terminal_columns: u16) -> u16 {
        self.inner.menu_required_lines(terminal_columns)
    }

    fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String {
        self.inner.menu_string(available_lines, use_ansi_coloring)
    }

    fn min_rows(&self) -> u16 {
        self.inner.min_rows()
    }

    fn get_values(&self) -> &[Suggestion] {
        self.inner.get_values()
    }

    fn set_cursor_pos(&mut self, pos: (u16, u16)) {
        self.inner.set_cursor_pos(pos);
    }
}
