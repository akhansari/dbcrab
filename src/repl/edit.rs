use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crossterm::event::{Event, KeyCode, KeyModifiers};
use reedline::{
    EditCommand, EditMode, Emacs, PromptEditMode, PromptViMode, ReedlineEvent, ReedlineRawEvent,
    Vi, default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
};

use crate::{
    config::{
        CommandKeybindings, ConfigEditMode, KeyBinding, KeyRemaps, KeybindingsConfig,
        history_menu_event,
    },
    render::DisplayModeState,
};

use super::{
    COMMAND_CANCEL_HOST_COMMAND, COMMAND_COMPLETION_MENU, COMMAND_MODE_HOST_COMMAND,
    COMPLETION_MENU,
};

pub(super) struct SqlEditMode {
    inner: EditorInnerEditMode,
    key_remaps: KeyRemaps,
    cycle_display: Vec<KeyBinding>,
    command_mode: Vec<KeyBinding>,
    display_mode: DisplayModeState,
    command_mode_ready: Arc<AtomicBool>,
    tracked_sql_input: TrackedSqlInput,
}

pub(super) enum EditorInnerEditMode {
    Emacs(Emacs),
    Vi(Vi),
}

impl EditorInnerEditMode {
    fn parse_event(&mut self, event: Event) -> ReedlineEvent {
        let Ok(event) = ReedlineRawEvent::try_from(event) else {
            return ReedlineEvent::None;
        };

        match self {
            Self::Emacs(edit_mode) => edit_mode.parse_event(event),
            Self::Vi(edit_mode) => edit_mode.parse_event(event),
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        match self {
            Self::Emacs(edit_mode) => edit_mode.edit_mode(),
            Self::Vi(edit_mode) => edit_mode.edit_mode(),
        }
    }
}

impl SqlEditMode {
    pub(super) fn new(
        inner: EditorInnerEditMode,
        key_remaps: KeyRemaps,
        cycle_display: Vec<KeyBinding>,
        command_mode: Vec<KeyBinding>,
        display_mode: DisplayModeState,
        command_mode_ready: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner,
            key_remaps,
            cycle_display,
            command_mode,
            display_mode,
            command_mode_ready,
            tracked_sql_input: TrackedSqlInput::default(),
        }
    }

    fn remap_event_for_current_mode(&self, event: Event) -> Event {
        match self.inner.edit_mode() {
            PromptEditMode::Emacs | PromptEditMode::Vi(PromptViMode::Insert) => {
                self.key_remaps.remap_text_input_event(event)
            }
            PromptEditMode::Default
            | PromptEditMode::Vi(PromptViMode::Normal)
            | PromptEditMode::Vi(PromptViMode::Visual)
            | PromptEditMode::Custom(_) => self.key_remaps.remap_event(event),
        }
    }
}

impl EditMode for SqlEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = Event::from(event);
        let event = self.remap_event_for_current_mode(event);

        if self.command_mode_ready.load(Ordering::Relaxed) {
            self.tracked_sql_input.reset();
        }
        if is_keybinding_event(&event, &self.command_mode)
            && self.command_mode_ready.load(Ordering::Relaxed)
        {
            self.command_mode_ready.store(false, Ordering::Relaxed);
            return ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned());
        }

        if is_keybinding_event(&event, &self.cycle_display) {
            self.display_mode.cycle();
            return ReedlineEvent::Repaint;
        }

        let reedline_event = self.inner.parse_event(event);
        self.tracked_sql_input.apply_event(&reedline_event);
        self.command_mode_ready
            .store(self.tracked_sql_input.is_known_empty(), Ordering::Relaxed);

        reedline_event
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

#[derive(Default)]
struct TrackedSqlInput {
    char_len: Option<usize>,
}

impl TrackedSqlInput {
    fn reset(&mut self) {
        self.char_len = Some(0);
    }

    fn is_known_empty(&self) -> bool {
        self.char_len == Some(0)
    }

    fn apply_event(&mut self, event: &ReedlineEvent) {
        match event {
            ReedlineEvent::Edit(commands) => {
                for command in commands {
                    self.apply_command(command);
                }
            }
            ReedlineEvent::Multiple(events) | ReedlineEvent::UntilFound(events) => {
                for event in events {
                    self.apply_event(event);
                }
            }
            _ => {}
        }
    }

    fn apply_command(&mut self, command: &EditCommand) {
        let Some(char_len) = self.char_len.as_mut() else {
            return;
        };

        match command {
            EditCommand::InsertChar(_) | EditCommand::InsertNewline => *char_len += 1,
            EditCommand::InsertString(value) => *char_len += value.chars().count(),
            EditCommand::Backspace | EditCommand::Delete | EditCommand::CutChar => {
                *char_len = char_len.saturating_sub(1);
            }
            EditCommand::Clear => *char_len = 0,
            EditCommand::ReplaceChars(count, value) => {
                *char_len = char_len.saturating_sub(*count) + value.chars().count();
            }
            EditCommand::ReplaceChar(_)
            | EditCommand::Complete
            | EditCommand::MoveToStart { .. }
            | EditCommand::MoveToLineStart { .. }
            | EditCommand::MoveToLineNonBlankStart { .. }
            | EditCommand::MoveToEnd { .. }
            | EditCommand::MoveToLineEnd { .. }
            | EditCommand::MoveLineUp { .. }
            | EditCommand::MoveLineDown { .. }
            | EditCommand::MoveLeft { .. }
            | EditCommand::MoveRight { .. }
            | EditCommand::MoveWordLeft { .. }
            | EditCommand::MoveBigWordLeft { .. }
            | EditCommand::MoveWordRight { .. }
            | EditCommand::MoveWordRightStart { .. }
            | EditCommand::MoveBigWordRightStart { .. }
            | EditCommand::MoveWordRightEnd { .. }
            | EditCommand::MoveBigWordRightEnd { .. }
            | EditCommand::MoveToPosition { .. }
            | EditCommand::SelectAll
            | EditCommand::CopySelection
            | EditCommand::CopyFromStart
            | EditCommand::CopyFromStartLinewise
            | EditCommand::CopyFromLineStart
            | EditCommand::CopyFromLineNonBlankStart
            | EditCommand::CopyToEnd
            | EditCommand::CopyToEndLinewise
            | EditCommand::CopyToLineEnd
            | EditCommand::CopyCurrentLine
            | EditCommand::CopyWordLeft
            | EditCommand::CopyBigWordLeft
            | EditCommand::CopyWordRight
            | EditCommand::CopyBigWordRight => {}
            _ => self.char_len = None,
        }
    }
}

pub(super) struct CommandEditMode {
    inner: EditorInnerEditMode,
    cancel: Vec<KeyBinding>,
    key_remaps: KeyRemaps,
}

impl CommandEditMode {
    pub(super) fn new(
        inner: EditorInnerEditMode,
        keybindings_config: CommandKeybindings,
        key_remaps: KeyRemaps,
    ) -> Self {
        Self {
            inner,
            cancel: keybindings_config.cancel,
            key_remaps,
        }
    }

    fn should_cancel(&self, event: &Event) -> bool {
        if !is_keybinding_event(event, &self.cancel) {
            return false;
        }

        !(matches!(
            self.inner.edit_mode(),
            PromptEditMode::Vi(PromptViMode::Insert)
        ) && is_unmodified_escape(event))
    }
}

impl EditMode for CommandEditMode {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let event = self.key_remaps.remap_text_input_event(Event::from(event));

        if self.should_cancel(&event) {
            return ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned());
        }

        self.inner.parse_event(event)
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

pub(super) fn sql_inner_edit_mode(
    edit_mode: ConfigEditMode,
    keybindings: &KeybindingsConfig,
) -> EditorInnerEditMode {
    match edit_mode {
        ConfigEditMode::Emacs => {
            EditorInnerEditMode::Emacs(Emacs::new(sql_emacs_keybindings(keybindings)))
        }
        ConfigEditMode::Vi => {
            let (insert, normal) = sql_vi_keybindings(keybindings);
            EditorInnerEditMode::Vi(Vi::new(insert, normal))
        }
    }
}

fn sql_emacs_keybindings(config: &KeybindingsConfig) -> reedline::Keybindings {
    let mut keybindings = default_emacs_keybindings();
    add_history_menu_keybinding(&mut keybindings);
    config.prompt.insert.apply_to(&mut keybindings);
    config.prompt.emacs.apply_to(&mut keybindings);
    add_completion_keybindings(&mut keybindings, &config.prompt.complete, COMPLETION_MENU);

    keybindings
}

fn sql_vi_keybindings(
    config: &KeybindingsConfig,
) -> (reedline::Keybindings, reedline::Keybindings) {
    let insert = sql_vi_insert_keybindings(config);
    let normal = sql_vi_normal_keybindings(config);

    (insert, normal)
}

fn sql_vi_insert_keybindings(config: &KeybindingsConfig) -> reedline::Keybindings {
    let mut insert = default_vi_insert_keybindings();
    add_history_menu_keybinding(&mut insert);
    config.prompt.insert.apply_to(&mut insert);
    config.prompt.vi_insert.apply_to(&mut insert);
    add_completion_keybindings(&mut insert, &config.prompt.complete, COMPLETION_MENU);

    insert
}

fn sql_vi_normal_keybindings(config: &KeybindingsConfig) -> reedline::Keybindings {
    let mut normal = default_vi_normal_keybindings();
    add_history_menu_keybinding(&mut normal);
    config.prompt.vi_normal.apply_to(&mut normal);

    normal
}

pub(super) fn command_inner_edit_mode(
    edit_mode: ConfigEditMode,
    keybindings: &KeybindingsConfig,
) -> EditorInnerEditMode {
    match edit_mode {
        ConfigEditMode::Emacs => {
            EditorInnerEditMode::Emacs(Emacs::new(command_emacs_keybindings(keybindings)))
        }
        ConfigEditMode::Vi => {
            let (insert, normal) = command_vi_keybindings(keybindings);
            EditorInnerEditMode::Vi(Vi::new(insert, normal))
        }
    }
}

fn command_emacs_keybindings(keybindings: &KeybindingsConfig) -> reedline::Keybindings {
    let mut editor_keybindings = default_emacs_keybindings();
    add_history_menu_keybinding(&mut editor_keybindings);
    keybindings.prompt.insert.apply_to(&mut editor_keybindings);
    keybindings.prompt.emacs.apply_to(&mut editor_keybindings);
    keybindings.command.editor.apply_to(&mut editor_keybindings);
    add_completion_keybindings(
        &mut editor_keybindings,
        &keybindings.command.complete,
        COMMAND_COMPLETION_MENU,
    );

    editor_keybindings
}

fn command_vi_keybindings(
    keybindings: &KeybindingsConfig,
) -> (reedline::Keybindings, reedline::Keybindings) {
    let insert = command_vi_insert_keybindings(keybindings);

    let mut normal = default_vi_normal_keybindings();
    add_history_menu_keybinding(&mut normal);
    keybindings.prompt.vi_normal.apply_to(&mut normal);

    (insert, normal)
}

fn command_vi_insert_keybindings(keybindings: &KeybindingsConfig) -> reedline::Keybindings {
    let mut insert = default_vi_insert_keybindings();
    add_history_menu_keybinding(&mut insert);
    keybindings.prompt.insert.apply_to(&mut insert);
    keybindings.prompt.vi_insert.apply_to(&mut insert);
    keybindings.command.editor.apply_to(&mut insert);
    add_completion_keybindings(
        &mut insert,
        &keybindings.command.complete,
        COMMAND_COMPLETION_MENU,
    );

    insert
}

fn add_history_menu_keybinding(keybindings: &mut reedline::Keybindings) {
    keybindings.remove_binding(KeyModifiers::CONTROL, KeyCode::Char('r'));
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('r'),
        history_menu_event(),
    );
}

fn add_completion_keybindings(
    keybindings: &mut reedline::Keybindings,
    bindings: &[KeyBinding],
    menu_name: &str,
) {
    let completion_event = completion_event(menu_name);
    for binding in bindings {
        keybindings.add_binding(binding.modifiers, binding.code, completion_event.clone());
    }
}

fn completion_event(menu_name: &str) -> ReedlineEvent {
    ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu(menu_name.to_owned()),
        ReedlineEvent::MenuNext,
    ])
}

fn is_keybinding_event(event: &Event, bindings: &[KeyBinding]) -> bool {
    matches!(event, Event::Key(key) if bindings.iter().any(|binding| binding.matches(*key)))
}

fn is_unmodified_escape(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{DisplayMode, DisplayModeState};
    use crossterm::event::KeyEvent;

    fn completion_keybindings() -> reedline::Keybindings {
        sql_emacs_keybindings(&KeybindingsConfig::default())
    }

    fn default_sql_edit_mode(display_mode: DisplayModeState) -> SqlEditMode {
        sql_edit_mode_with_remaps(ConfigEditMode::Emacs, KeyRemaps::default(), display_mode)
    }

    fn sql_edit_mode_with_remaps(
        edit_mode: ConfigEditMode,
        key_remaps: KeyRemaps,
        display_mode: DisplayModeState,
    ) -> SqlEditMode {
        let keybindings = KeybindingsConfig::default();
        SqlEditMode::new(
            sql_inner_edit_mode(edit_mode, &keybindings),
            key_remaps,
            keybindings.prompt.cycle_display,
            keybindings.prompt.command_mode,
            display_mode,
            Arc::new(AtomicBool::new(true)),
        )
    }

    fn default_command_edit_mode() -> CommandEditMode {
        command_edit_mode(ConfigEditMode::Emacs)
    }

    fn command_edit_mode(edit_mode: ConfigEditMode) -> CommandEditMode {
        command_edit_mode_with_remaps(edit_mode, KeyRemaps::default())
    }

    fn command_edit_mode_with_remaps(
        edit_mode: ConfigEditMode,
        key_remaps: KeyRemaps,
    ) -> CommandEditMode {
        let keybindings = KeybindingsConfig::default();
        CommandEditMode::new(
            command_inner_edit_mode(edit_mode, &keybindings),
            keybindings.command,
            key_remaps,
        )
    }

    fn swapped_plain_key_remaps(from: char, to: char) -> KeyRemaps {
        let mut key_remaps = KeyRemaps::default();
        key_remaps.set(
            key_binding(KeyCode::Char(from), KeyModifiers::NONE),
            key_binding(KeyCode::Char(to), KeyModifiers::NONE),
        );
        key_remaps.set(
            key_binding(KeyCode::Char(to), KeyModifiers::NONE),
            key_binding(KeyCode::Char(from), KeyModifiers::NONE),
        );
        key_remaps
    }

    fn key_remap(
        from_code: KeyCode,
        from_modifiers: KeyModifiers,
        to_code: KeyCode,
        to_modifiers: KeyModifiers,
    ) -> KeyRemaps {
        let mut key_remaps = KeyRemaps::default();
        key_remaps.set(
            key_binding(from_code, from_modifiers),
            key_binding(to_code, to_modifiers),
        );
        key_remaps
    }

    fn key_binding(code: KeyCode, modifiers: KeyModifiers) -> KeyBinding {
        KeyBinding { code, modifiers }
    }

    #[test]
    fn tab_opens_completion_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::NONE, KeyCode::Tab);

        // Then
        assert_eq!(event, Some(completion_event(COMPLETION_MENU)));
    }

    #[test]
    fn ctrl_space_opens_completion_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char(' '));

        // Then
        assert_eq!(event, Some(completion_event(COMPLETION_MENU)));
    }

    #[test]
    fn ctrl_r_opens_history_menu() {
        // Given
        let keybindings = completion_keybindings();

        // When
        let event = keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char('r'));

        // Then
        assert_eq!(event, Some(history_menu_event()));
    }

    #[test]
    fn alt_v_toggles_display_mode_in_place() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode.clone());
        let event = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::ALT,
        )))
        .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, ReedlineEvent::Repaint);
        assert_eq!(display_mode.get(), DisplayMode::Tui);
    }

    #[test]
    fn first_colon_enters_command_mode() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode);
        let event = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
        )))
        .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn colon_after_sql_text_stays_in_sql_mode() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode);

        // When
        let _ = edit_mode.parse_event(raw_char_event('s'));
        let colon_event = edit_mode.parse_event(raw_char_event(':'));

        // Then
        assert_eq!(
            colon_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar(':')])
        );
    }

    #[test]
    fn colon_enters_command_mode_after_erasing_sql_text() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode);

        // When
        for ch in "select *".chars() {
            let _ = edit_mode.parse_event(raw_char_event(ch));
        }
        for _ in 0.."select *".chars().count() {
            let _ = edit_mode.parse_event(raw_key_event(KeyCode::Backspace, KeyModifiers::NONE));
        }
        let colon_event = edit_mode.parse_event(raw_char_event(':'));

        // Then
        assert_eq!(
            colon_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn ctrl_l_does_not_block_command_mode_trigger() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode);
        let clear = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        )))
        .expect("key event should be valid");
        let colon = ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(
            KeyCode::Char(':'),
            KeyModifiers::SHIFT,
        )))
        .expect("key event should be valid");

        // When
        let clear_event = edit_mode.parse_event(clear);
        let colon_event = edit_mode.parse_event(colon);

        // Then
        assert_eq!(clear_event, ReedlineEvent::ClearScreen);
        assert_eq!(
            colon_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_MODE_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn prompt_ctrl_r_reaches_history_menu() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = default_sql_edit_mode(display_mode);
        let event = raw_key_event(KeyCode::Char('r'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, history_menu_event());
    }

    #[test]
    fn vi_normal_prompt_ctrl_r_reaches_history_menu() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode =
            sql_edit_mode_with_remaps(ConfigEditMode::Vi, KeyRemaps::default(), display_mode);
        let _ = edit_mode.parse_event(raw_key_event(KeyCode::Esc, KeyModifiers::NONE));
        let event = raw_key_event(KeyCode::Char('r'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, history_menu_event());
    }

    #[test]
    fn emacs_prompt_keeps_plain_swapped_characters_while_typing() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = sql_edit_mode_with_remaps(
            ConfigEditMode::Emacs,
            swapped_plain_key_remaps('h', 'i'),
            display_mode,
        );

        // When
        let reedline_event = edit_mode.parse_event(raw_char_event('h'));

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('h')])
        );
    }

    #[test]
    fn emacs_prompt_keeps_shifted_plain_swapped_characters_while_typing() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = sql_edit_mode_with_remaps(
            ConfigEditMode::Emacs,
            swapped_plain_key_remaps('h', 'i'),
            display_mode,
        );

        // When
        let reedline_event =
            edit_mode.parse_event(raw_key_event(KeyCode::Char('H'), KeyModifiers::SHIFT));

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('H')])
        );
    }

    #[test]
    fn vi_insert_prompt_keeps_plain_swapped_characters_while_typing() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = sql_edit_mode_with_remaps(
            ConfigEditMode::Vi,
            swapped_plain_key_remaps('h', 'i'),
            display_mode,
        );

        // When
        let reedline_event = edit_mode.parse_event(raw_char_event('h'));

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('h')])
        );
    }

    #[test]
    fn prompt_still_applies_modified_remaps_while_typing() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = sql_edit_mode_with_remaps(
            ConfigEditMode::Emacs,
            key_remap(
                KeyCode::Char('h'),
                KeyModifiers::CONTROL,
                KeyCode::Char('l'),
                KeyModifiers::CONTROL,
            ),
            display_mode,
        );

        // When
        let reedline_event =
            edit_mode.parse_event(raw_key_event(KeyCode::Char('h'), KeyModifiers::CONTROL));

        // Then
        assert_eq!(reedline_event, ReedlineEvent::ClearScreen);
    }

    #[test]
    fn vi_normal_prompt_applies_plain_swapped_characters() {
        // Given
        let display_mode = DisplayModeState::new();
        let mut edit_mode = sql_edit_mode_with_remaps(
            ConfigEditMode::Vi,
            swapped_plain_key_remaps('h', 'i'),
            display_mode,
        );
        let _ = edit_mode.parse_event(raw_key_event(KeyCode::Esc, KeyModifiers::NONE));

        // When
        let reedline_event = edit_mode.parse_event(raw_char_event('h'));

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::Multiple(vec![ReedlineEvent::Repaint])
        );
    }

    fn raw_char_event(ch: char) -> ReedlineRawEvent {
        raw_key_event(
            KeyCode::Char(ch),
            if ch == ':' {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            },
        )
    }

    fn raw_key_event(code: KeyCode, modifiers: KeyModifiers) -> ReedlineRawEvent {
        ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(code, modifiers)))
            .expect("key event should be valid")
    }

    #[test]
    fn emacs_command_mode_esc_cancels_command_mode() {
        // Given
        let mut edit_mode = default_command_edit_mode();
        let event =
            ReedlineRawEvent::try_from(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
                .expect("key event should be valid");

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn vi_command_insert_esc_enters_normal_mode() {
        // Given
        let mut edit_mode = command_edit_mode(ConfigEditMode::Vi);
        let event = raw_key_event(KeyCode::Esc, KeyModifiers::NONE);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_ne!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
        assert!(matches!(
            edit_mode.edit_mode(),
            PromptEditMode::Vi(PromptViMode::Normal)
        ));
    }

    #[test]
    fn vi_command_normal_esc_cancels_command_mode() {
        // Given
        let mut edit_mode = command_edit_mode(ConfigEditMode::Vi);
        let _ = edit_mode.parse_event(raw_key_event(KeyCode::Esc, KeyModifiers::NONE));
        let event = raw_key_event(KeyCode::Esc, KeyModifiers::NONE);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn command_mode_ctrl_d_cancels_command_mode() {
        // Given
        let mut edit_mode = default_command_edit_mode();
        let event = raw_key_event(KeyCode::Char('d'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn command_mode_ctrl_r_reaches_history_menu() {
        // Given
        let mut edit_mode = default_command_edit_mode();
        let event = raw_key_event(KeyCode::Char('r'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, history_menu_event());
    }

    #[test]
    fn vi_normal_command_ctrl_r_reaches_history_menu() {
        // Given
        let mut edit_mode = command_edit_mode(ConfigEditMode::Vi);
        let _ = edit_mode.parse_event(raw_key_event(KeyCode::Esc, KeyModifiers::NONE));
        let event = raw_key_event(KeyCode::Char('r'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, history_menu_event());
    }

    #[test]
    fn vi_command_insert_ctrl_d_cancels_command_mode() {
        // Given
        let mut edit_mode = command_edit_mode(ConfigEditMode::Vi);
        let event = raw_key_event(KeyCode::Char('d'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::ExecuteHostCommand(COMMAND_CANCEL_HOST_COMMAND.to_owned())
        );
    }

    #[test]
    fn command_mode_ctrl_c_does_not_cancel_command_mode() {
        // Given
        let mut edit_mode = default_command_edit_mode();
        let event = raw_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL);

        // When
        let reedline_event = edit_mode.parse_event(event);

        // Then
        assert_eq!(reedline_event, ReedlineEvent::CtrlC);
    }

    #[test]
    fn command_mode_keeps_plain_swapped_characters_while_typing() {
        // Given
        let mut edit_mode = command_edit_mode_with_remaps(
            ConfigEditMode::Emacs,
            swapped_plain_key_remaps('h', 'i'),
        );

        // When
        let reedline_event = edit_mode.parse_event(raw_char_event('h'));

        // Then
        assert_eq!(
            reedline_event,
            ReedlineEvent::Edit(vec![EditCommand::InsertChar('h')])
        );
    }
}
