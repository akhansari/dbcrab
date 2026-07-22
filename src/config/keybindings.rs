use std::fmt;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use reedline::{
    EditCommand, Keybindings as ReedlineKeybindings, ReedlineEvent, default_emacs_keybindings,
    default_vi_insert_keybindings, default_vi_normal_keybindings,
};

pub(crate) const HISTORY_MENU: &str = "history_menu";

pub(crate) fn history_menu_event() -> ReedlineEvent {
    ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu(HISTORY_MENU.to_owned()),
        ReedlineEvent::MenuPageNext,
    ])
}

pub(crate) fn default_emacs_editor_keybindings() -> ReedlineKeybindings {
    with_history_menu(default_emacs_keybindings())
}

pub(crate) fn default_vi_insert_editor_keybindings() -> ReedlineKeybindings {
    with_history_menu(default_vi_insert_keybindings())
}

pub(crate) fn default_vi_normal_editor_keybindings() -> ReedlineKeybindings {
    with_history_menu(default_vi_normal_keybindings())
}

fn with_history_menu(mut keybindings: ReedlineKeybindings) -> ReedlineKeybindings {
    keybindings.remove_binding(KeyModifiers::CONTROL, KeyCode::Char('r'));
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('r'),
        history_menu_event(),
    );
    keybindings
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub edit_mode: ConfigEditMode,
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct KeybindingsConfig {
    pub remaps: KeyRemaps,
    pub editor: EditorKeybindings,
    pub prompt: PromptKeybindings,
    pub command: CommandKeybindings,
    pub tui: TuiKeybindings,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct KeyRemaps {
    normal: Vec<KeyRemap>,
    visual: Vec<KeyRemap>,
    shortcut_nav: Vec<KeyRemap>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct KeyRemap {
    from: KeyBinding,
    to: KeyBinding,
}

impl KeyRemaps {
    fn remaps_mut(&mut self, scope: KeyRemapScope) -> &mut Vec<KeyRemap> {
        match scope {
            KeyRemapScope::Normal => &mut self.normal,
            KeyRemapScope::Visual => &mut self.visual,
            KeyRemapScope::ShortcutNav => &mut self.shortcut_nav,
        }
    }

    fn remaps(&self, scope: KeyRemapScope) -> &[KeyRemap] {
        match scope {
            KeyRemapScope::Normal => &self.normal,
            KeyRemapScope::Visual => &self.visual,
            KeyRemapScope::ShortcutNav => &self.shortcut_nav,
        }
    }

    pub(super) fn set(&mut self, scope: KeyRemapScope, from: KeyBinding, to: KeyBinding) {
        let remaps = self.remaps_mut(scope);
        remaps.retain(|remap| remap.from != from);
        remaps.push(KeyRemap { from, to });
    }

    #[cfg(test)]
    pub(crate) fn set_vi_remap(&mut self, mode: ViRemapMode, from: KeyBinding, to: KeyBinding) {
        let scope = match mode {
            ViRemapMode::Normal => KeyRemapScope::Normal,
            ViRemapMode::Visual => KeyRemapScope::Visual,
        };
        self.set(scope, from, to);
    }

    #[cfg(test)]
    pub(crate) fn set_shortcut_nav_remap(&mut self, from: KeyBinding, to: KeyBinding) {
        self.set(KeyRemapScope::ShortcutNav, from, to);
    }

    pub(crate) fn remap_editor_event(&self, vi_mode: Option<ViRemapMode>, event: Event) -> Event {
        let Event::Key(key) = event else {
            return event;
        };
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Event::Key(self.remap_key_event(KeyRemapScope::ShortcutNav, key));
        }
        match vi_mode {
            Some(ViRemapMode::Normal) => {
                Event::Key(self.remap_key_event(KeyRemapScope::Normal, key))
            }
            Some(ViRemapMode::Visual) => {
                Event::Key(self.remap_key_event(KeyRemapScope::Visual, key))
            }
            None => Event::Key(key),
        }
    }

    pub(crate) fn remap_shortcut_key_event(&self, key: KeyEvent) -> KeyEvent {
        self.remap_key_event(KeyRemapScope::ShortcutNav, key)
    }

    fn remap_key_event(&self, scope: KeyRemapScope, key: KeyEvent) -> KeyEvent {
        let remaps = self.remaps(scope);
        if let Some(remap) = remaps
            .iter()
            .find(|remap| !remap.from.is_plain_char() && remap.from.matches(key))
        {
            return key_event_with_binding(key, remap.to);
        }

        if let Some(remap) = remaps.iter().find(|remap| remap.from.matches(key)) {
            return key_event_with_binding(key, remap.to);
        }

        Self::remap_shifted_plain_char(remaps, key).unwrap_or(key)
    }

    fn remap_shifted_plain_char(remaps: &[KeyRemap], key: KeyEvent) -> Option<KeyEvent> {
        let KeyCode::Char(ch) = key.code else {
            return None;
        };
        let mut modifiers = key.modifiers;
        modifiers.remove(KeyModifiers::SHIFT);
        if !modifiers.is_empty() {
            return None;
        }

        let shifted = key.modifiers.contains(KeyModifiers::SHIFT) || ch.is_ascii_uppercase();
        let ch = ch.to_ascii_lowercase();
        let remap = remaps.iter().find(|remap| {
            matches!(remap.from.code, KeyCode::Char(from) if remap.from.modifiers.is_empty() && from == ch)
        })?;

        Some(match remap.to.code {
            KeyCode::Char(target) if remap.to.modifiers.is_empty() && shifted => {
                let mut remapped = key_event_with_binding(key, remap.to);
                remapped.code = KeyCode::Char(target.to_ascii_lowercase());
                remapped.modifiers.insert(KeyModifiers::SHIFT);
                remapped
            }
            _ => key_event_with_binding(key, remap.to),
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum KeyRemapScope {
    Normal,
    Visual,
    ShortcutNav,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ViRemapMode {
    Normal,
    Visual,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum ConfigEditMode {
    #[default]
    Emacs,
    Vi,
}

impl ConfigEditMode {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Emacs => "emacs",
            Self::Vi => "vi",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct LineEditorKeybindings {
    updates: Vec<LineEditorKeybindingUpdate>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct EditorKeybindings {
    pub emacs_vi_insert: LineEditorKeybindings,
    pub vi_normal: LineEditorKeybindings,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct LineEditorKeybindingUpdate {
    action: LineEditorAction,
    operation: KeyBindingOperation,
    bindings: Vec<KeyBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PromptKeybindings {
    pub complete: Vec<KeyBinding>,
    pub cycle_display: Vec<KeyBinding>,
    pub command_mode: Vec<KeyBinding>,
}

impl Default for PromptKeybindings {
    fn default() -> Self {
        Self {
            complete: key_bindings(PromptAction::Complete.default_bindings()),
            cycle_display: key_bindings(PromptAction::CycleDisplay.default_bindings()),
            command_mode: key_bindings(PromptAction::CommandMode.default_bindings()),
        }
    }
}

impl PromptKeybindings {
    pub(super) fn bindings_for(&self, action: PromptAction) -> &[KeyBinding] {
        match action {
            PromptAction::Complete => &self.complete,
            PromptAction::CycleDisplay => &self.cycle_display,
            PromptAction::CommandMode => &self.command_mode,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandKeybindings {
    pub complete: Vec<KeyBinding>,
    pub cancel: Vec<KeyBinding>,
    pub editor: LineEditorKeybindings,
}

impl Default for CommandKeybindings {
    fn default() -> Self {
        Self {
            complete: key_bindings(CommandAction::Complete.default_bindings()),
            cancel: key_bindings(CommandAction::Cancel.default_bindings()),
            editor: LineEditorKeybindings::default(),
        }
    }
}

impl CommandKeybindings {
    pub(super) fn bindings_for(&self, action: CommandAction) -> &[KeyBinding] {
        match action {
            CommandAction::Complete => &self.complete,
            CommandAction::Cancel => &self.cancel,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TuiKeybindings {
    left: Vec<KeyBinding>,
    up: Vec<KeyBinding>,
    right: Vec<KeyBinding>,
    down: Vec<KeyBinding>,
    half_page_left: Vec<KeyBinding>,
    half_page_up: Vec<KeyBinding>,
    half_page_right: Vec<KeyBinding>,
    half_page_down: Vec<KeyBinding>,
    full_page_left: Vec<KeyBinding>,
    full_page_up: Vec<KeyBinding>,
    full_page_right: Vec<KeyBinding>,
    full_page_down: Vec<KeyBinding>,
    toggle_preview: Vec<KeyBinding>,
    focus_next: Vec<KeyBinding>,
    edit_cell: Vec<KeyBinding>,
    stage_change: Vec<KeyBinding>,
    stage_null: Vec<KeyBinding>,
    update_row: Vec<KeyBinding>,
    yank_cell: Vec<KeyBinding>,
    quit: Vec<KeyBinding>,
}

impl Default for TuiKeybindings {
    fn default() -> Self {
        Self {
            left: key_bindings(TuiAction::Left.default_bindings()),
            up: key_bindings(TuiAction::Up.default_bindings()),
            right: key_bindings(TuiAction::Right.default_bindings()),
            down: key_bindings(TuiAction::Down.default_bindings()),
            half_page_left: key_bindings(TuiAction::HalfPageLeft.default_bindings()),
            half_page_up: key_bindings(TuiAction::HalfPageUp.default_bindings()),
            half_page_right: key_bindings(TuiAction::HalfPageRight.default_bindings()),
            half_page_down: key_bindings(TuiAction::HalfPageDown.default_bindings()),
            full_page_left: key_bindings(TuiAction::FullPageLeft.default_bindings()),
            full_page_up: key_bindings(TuiAction::FullPageUp.default_bindings()),
            full_page_right: key_bindings(TuiAction::FullPageRight.default_bindings()),
            full_page_down: key_bindings(TuiAction::FullPageDown.default_bindings()),
            toggle_preview: key_bindings(TuiAction::TogglePreview.default_bindings()),
            focus_next: key_bindings(TuiAction::FocusNext.default_bindings()),
            edit_cell: key_bindings(TuiAction::EditCell.default_bindings()),
            stage_change: key_bindings(TuiAction::StageChange.default_bindings()),
            stage_null: key_bindings(TuiAction::StageNull.default_bindings()),
            update_row: key_bindings(TuiAction::UpdateRow.default_bindings()),
            yank_cell: key_bindings(TuiAction::YankCell.default_bindings()),
            quit: key_bindings(TuiAction::Quit.default_bindings()),
        }
    }
}

impl TuiKeybindings {
    pub fn action_for(&self, key: KeyEvent) -> Option<TuiAction> {
        TuiAction::ALL
            .iter()
            .copied()
            .find(|action| self.matches_action(*action, key))
    }

    pub(crate) fn matches_action(&self, action: TuiAction, key: KeyEvent) -> bool {
        self.bindings_for(action)
            .iter()
            .any(|binding| binding.matches(key))
    }

    pub(crate) fn display_binding_for(&self, action: TuiAction) -> Option<KeyBinding> {
        self.bindings_for(action).first().copied()
    }

    pub(super) fn bindings_for(&self, action: TuiAction) -> &[KeyBinding] {
        match action {
            TuiAction::Left => &self.left,
            TuiAction::Up => &self.up,
            TuiAction::Right => &self.right,
            TuiAction::Down => &self.down,
            TuiAction::HalfPageLeft => &self.half_page_left,
            TuiAction::HalfPageUp => &self.half_page_up,
            TuiAction::HalfPageRight => &self.half_page_right,
            TuiAction::HalfPageDown => &self.half_page_down,
            TuiAction::FullPageLeft => &self.full_page_left,
            TuiAction::FullPageUp => &self.full_page_up,
            TuiAction::FullPageRight => &self.full_page_right,
            TuiAction::FullPageDown => &self.full_page_down,
            TuiAction::TogglePreview => &self.toggle_preview,
            TuiAction::FocusNext => &self.focus_next,
            TuiAction::EditCell => &self.edit_cell,
            TuiAction::StageChange => &self.stage_change,
            TuiAction::StageNull => &self.stage_null,
            TuiAction::UpdateRow => &self.update_row,
            TuiAction::YankCell => &self.yank_cell,
            TuiAction::Quit => &self.quit,
        }
    }

    fn bindings_for_mut(&mut self, action: TuiAction) -> &mut Vec<KeyBinding> {
        match action {
            TuiAction::Left => &mut self.left,
            TuiAction::Up => &mut self.up,
            TuiAction::Right => &mut self.right,
            TuiAction::Down => &mut self.down,
            TuiAction::HalfPageLeft => &mut self.half_page_left,
            TuiAction::HalfPageUp => &mut self.half_page_up,
            TuiAction::HalfPageRight => &mut self.half_page_right,
            TuiAction::HalfPageDown => &mut self.half_page_down,
            TuiAction::FullPageLeft => &mut self.full_page_left,
            TuiAction::FullPageUp => &mut self.full_page_up,
            TuiAction::FullPageRight => &mut self.full_page_right,
            TuiAction::FullPageDown => &mut self.full_page_down,
            TuiAction::TogglePreview => &mut self.toggle_preview,
            TuiAction::FocusNext => &mut self.focus_next,
            TuiAction::EditCell => &mut self.edit_cell,
            TuiAction::StageChange => &mut self.stage_change,
            TuiAction::StageNull => &mut self.stage_null,
            TuiAction::UpdateRow => &mut self.update_row,
            TuiAction::YankCell => &mut self.yank_cell,
            TuiAction::Quit => &mut self.quit,
        }
    }

    pub(super) fn apply_update(
        &mut self,
        action: TuiAction,
        operation: KeyBindingOperation,
        bindings: Vec<KeyBinding>,
    ) {
        apply_bindings_update(self.bindings_for_mut(action), operation, bindings);
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.quit.is_empty() {
            return Err("`quit` must have at least one key binding".to_owned());
        }

        let mut seen = Vec::new();
        for &action in TuiAction::ALL {
            for binding in self.bindings_for(action) {
                if let Some((existing, existing_action)) = seen
                    .iter()
                    .find(|(existing, _): &&(KeyBinding, TuiAction)| existing == binding)
                {
                    return Err(format!(
                        "key `{existing}` is bound to both `{}` and `{}`",
                        existing_action.name(),
                        action.name()
                    ));
                }
                seen.push((*binding, action));
            }
        }

        Ok(())
    }
}

impl LineEditorKeybindings {
    pub(super) fn push_update(
        &mut self,
        action: LineEditorAction,
        operation: KeyBindingOperation,
        bindings: Vec<KeyBinding>,
    ) {
        self.updates.push(LineEditorKeybindingUpdate {
            action,
            operation,
            bindings: dedup_bindings(bindings),
        });
    }

    pub fn apply_to(&self, keybindings: &mut reedline::Keybindings) {
        for update in &self.updates {
            let event = update.action.event();
            match update.operation {
                KeyBindingOperation::Set => {
                    remove_event_bindings(keybindings, &event);
                    add_event_bindings(keybindings, &update.bindings, event);
                }
                KeyBindingOperation::Add => {
                    add_event_bindings(keybindings, &update.bindings, event)
                }
                KeyBindingOperation::Remove => {
                    for binding in &update.bindings {
                        if keybindings
                            .find_binding(binding.modifiers, binding.code)
                            .is_some_and(|existing| existing == event)
                        {
                            keybindings.remove_binding(binding.modifiers, binding.code);
                        }
                    }
                }
            }
        }
    }
}

macro_rules! named_action_enum {
    (
        $(#[$meta:meta])*
        $visibility:vis enum $name:ident {
            $($variant:ident => $config_name:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $visibility enum $name {
            $($variant),+
        }

        impl $name {
            pub(super) const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub(super) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $config_name),+
                }
            }

            pub(super) fn from_name(name: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|action| action.name() == name)
            }
        }
    };
}

named_action_enum! {
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum TuiAction {
        Left => "left",
        Up => "up",
        Right => "right",
        Down => "down",
        HalfPageLeft => "half-page-left",
        HalfPageUp => "half-page-up",
        HalfPageRight => "half-page-right",
        HalfPageDown => "half-page-down",
        FullPageLeft => "full-page-left",
        FullPageUp => "full-page-up",
        FullPageRight => "full-page-right",
        FullPageDown => "full-page-down",
        TogglePreview => "toggle-preview",
        FocusNext => "focus-next",
        EditCell => "edit-cell",
        StageChange => "stage-change",
        StageNull => "stage-null",
        UpdateRow => "update-row",
        YankCell => "yank-cell",
        Quit => "quit",
    }
}

impl TuiAction {
    fn default_bindings(self) -> &'static [&'static str] {
        match self {
            Self::Left => &["left", "h"],
            Self::Up => &["up", "k"],
            Self::Right => &["right", "l"],
            Self::Down => &["down", "j"],
            Self::HalfPageLeft => &["shift-h"],
            Self::HalfPageUp => &["shift-k"],
            Self::HalfPageRight => &["shift-l"],
            Self::HalfPageDown => &["shift-j", "ctrl-d"],
            Self::FullPageLeft => &["ctrl-h"],
            Self::FullPageUp => &["ctrl-k", "pageup"],
            Self::FullPageRight => &["ctrl-l"],
            Self::FullPageDown => &["ctrl-j", "pagedown"],
            Self::TogglePreview => &["enter"],
            Self::FocusNext => &["tab"],
            Self::EditCell => &["c"],
            Self::StageChange => &["ctrl-s"],
            Self::StageNull => &["ctrl-x"],
            Self::UpdateRow => &["ctrl-u"],
            Self::YankCell => &["y"],
            Self::Quit => &["q", "esc", "ctrl-c"],
        }
    }

    pub(super) fn ends_template_group(self) -> bool {
        matches!(
            self,
            Self::Down | Self::HalfPageDown | Self::FullPageDown | Self::YankCell
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct KeyBinding {
    pub(crate) code: KeyCode,
    pub(crate) modifiers: KeyModifiers,
}

impl KeyBinding {
    fn is_plain_char(self) -> bool {
        matches!(self.code, KeyCode::Char(_)) && self.modifiers.is_empty()
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        match (self.code, key.code) {
            (KeyCode::Char(expected), KeyCode::Char(actual)) => {
                modifiers_without_shift(self.modifiers) == modifiers_without_shift(key.modifiers)
                    && if self.modifiers.contains(KeyModifiers::SHIFT) {
                        actual.eq_ignore_ascii_case(&expected)
                            && (key.modifiers.contains(KeyModifiers::SHIFT)
                                || actual.is_ascii_uppercase())
                    } else {
                        actual == expected
                            && (!key.modifiers.contains(KeyModifiers::SHIFT)
                                || !expected.is_ascii_alphabetic())
                    }
            }
            _ => self.code == key.code && self.modifiers == key.modifiers,
        }
    }
}

fn key_event_with_binding(mut key: KeyEvent, binding: KeyBinding) -> KeyEvent {
    key.code = binding.code;
    key.modifiers = binding.modifiers;
    key
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum KeyBindingOperation {
    Set,
    Add,
    Remove,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum KeyRemapOperation {
    Set,
    Swap,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ViModeSelection {
    pub(super) normal: bool,
    pub(super) visual: bool,
}

named_action_enum! {
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub(super) enum PromptAction {
        Complete => "complete",
        CycleDisplay => "cycle-display",
        CommandMode => "command-mode",
    }
}

impl PromptAction {
    fn default_bindings(self) -> &'static [&'static str] {
        match self {
            Self::Complete => &["tab", "ctrl-space"],
            Self::CycleDisplay => &["alt-v"],
            Self::CommandMode => &[":"],
        }
    }
}

named_action_enum! {
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub(super) enum CommandAction {
        Complete => "complete",
        Cancel => "cancel",
    }
}

impl CommandAction {
    fn default_bindings(self) -> &'static [&'static str] {
        match self {
            Self::Complete => &["tab", "ctrl-space"],
            Self::Cancel => &["esc", "ctrl-d"],
        }
    }
}

named_action_enum! {
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub(super) enum LineEditorAction {
        Esc => "esc",
        CtrlC => "ctrl-c",
        CtrlD => "ctrl-d",
        ClearScreen => "clear-screen",
        HistoryMenu => "history-menu",
        OpenEditor => "open-editor",
        Enter => "enter",
        InsertNewline => "insert-newline",
        Up => "up",
        Down => "down",
        Left => "left",
        Right => "right",
        ToStart => "to-start",
        ToEnd => "to-end",
        MoveToStart => "move-to-start",
        MoveToLineStart => "move-to-line-start",
        MoveToLineNonBlankStart => "move-to-line-non-blank-start",
        MoveToEnd => "move-to-end",
        MoveToLineEnd => "move-to-line-end",
        MoveLineUp => "move-line-up",
        MoveLineDown => "move-line-down",
        MoveLeft => "move-left",
        MoveRight => "move-right",
        MoveWordLeft => "move-word-left",
        MoveWordRight => "move-word-right",
        MoveBigWordLeft => "move-big-word-left",
        MoveBigWordRight => "move-big-word-right",
        MoveWordRightStart => "move-word-right-start",
        MoveWordRightEnd => "move-word-right-end",
        MoveBigWordRightStart => "move-big-word-right-start",
        MoveBigWordRightEnd => "move-big-word-right-end",
        Backspace => "backspace",
        Delete => "delete",
        BackspaceWord => "backspace-word",
        DeleteWord => "delete-word",
        CutChar => "cut-char",
        Clear => "clear",
        ClearToLineEnd => "clear-to-line-end",
        CutCurrentLine => "cut-current-line",
        CutFromStart => "cut-from-start",
        CutFromLineStart => "cut-from-line-start",
        CutFromLineNonBlankStart => "cut-from-line-non-blank-start",
        CutToEnd => "cut-to-end",
        CutToLineEnd => "cut-to-line-end",
        KillLine => "kill-line",
        CutWordLeft => "cut-word-left",
        CutWordRight => "cut-word-right",
        CutBigWordLeft => "cut-big-word-left",
        CutBigWordRight => "cut-big-word-right",
        PasteCutBufferBefore => "paste-cut-buffer-before",
        PasteCutBufferAfter => "paste-cut-buffer-after",
        Paste => "paste",
        Undo => "undo",
        Redo => "redo",
        UppercaseWord => "uppercase-word",
        LowercaseWord => "lowercase-word",
        CapitalizeChar => "capitalize-char",
        SwitchcaseChar => "switchcase-char",
        SwapWords => "swap-words",
        SwapGraphemes => "swap-graphemes",
        SelectAll => "select-all",
        CopySelection => "copy-selection",
        CutSelection => "cut-selection",
        CopyFromStart => "copy-from-start",
        CopyFromLineStart => "copy-from-line-start",
        CopyFromLineNonBlankStart => "copy-from-line-non-blank-start",
        CopyToEnd => "copy-to-end",
        CopyToLineEnd => "copy-to-line-end",
        CopyCurrentLine => "copy-current-line",
        CopyWordLeft => "copy-word-left",
        CopyWordRight => "copy-word-right",
        CopyBigWordLeft => "copy-big-word-left",
        CopyBigWordRight => "copy-big-word-right",
        MoveLineUpSelect => "move-line-up-select",
        MoveLineDownSelect => "move-line-down-select",
        MoveLeftSelect => "move-left-select",
        MoveRightSelect => "move-right-select",
        MoveWordLeftSelect => "move-word-left-select",
        MoveWordRightSelect => "move-word-right-select",
        MoveToLineStartSelect => "move-to-line-start-select",
        MoveToLineEndSelect => "move-to-line-end-select",
        MoveToStartSelect => "move-to-start-select",
        MoveToEndSelect => "move-to-end-select",
    }
}

impl LineEditorAction {
    pub(super) fn event(self) -> ReedlineEvent {
        use EditCommand as EC;
        use LineEditorAction as Action;
        use ReedlineEvent as RE;

        fn edit(command: EditCommand) -> ReedlineEvent {
            ReedlineEvent::Edit(vec![command])
        }

        match self {
            Action::Esc => RE::Esc,
            Action::CtrlC => RE::CtrlC,
            Action::CtrlD => RE::CtrlD,
            Action::ClearScreen => RE::ClearScreen,
            Action::HistoryMenu => history_menu_event(),
            Action::OpenEditor => RE::OpenEditor,
            Action::Enter => RE::Enter,
            Action::InsertNewline => edit(EC::InsertNewline),
            Action::Up => RE::UntilFound(vec![RE::MenuUp, RE::Up]),
            Action::Down => RE::UntilFound(vec![RE::MenuDown, RE::Down]),
            Action::Left => RE::UntilFound(vec![RE::MenuLeft, RE::Left]),
            Action::Right => {
                RE::UntilFound(vec![RE::HistoryHintComplete, RE::MenuRight, RE::Right])
            }
            Action::ToStart => RE::ToStart,
            Action::ToEnd => RE::ToEnd,
            Action::MoveToStart => edit(EC::MoveToStart { select: false }),
            Action::MoveToLineStart => edit(EC::MoveToLineStart { select: false }),
            Action::MoveToLineNonBlankStart => edit(EC::MoveToLineNonBlankStart { select: false }),
            Action::MoveToEnd => edit(EC::MoveToEnd { select: false }),
            Action::MoveToLineEnd => RE::UntilFound(vec![
                RE::HistoryHintComplete,
                edit(EC::MoveToLineEnd { select: false }),
            ]),
            Action::MoveLineUp => edit(EC::MoveLineUp { select: false }),
            Action::MoveLineDown => edit(EC::MoveLineDown { select: false }),
            Action::MoveLeft => edit(EC::MoveLeft { select: false }),
            Action::MoveRight => edit(EC::MoveRight { select: false }),
            Action::MoveWordLeft => edit(EC::MoveWordLeft { select: false }),
            Action::MoveWordRight => RE::UntilFound(vec![
                RE::HistoryHintWordComplete,
                edit(EC::MoveWordRight { select: false }),
            ]),
            Action::MoveBigWordLeft => edit(EC::MoveBigWordLeft { select: false }),
            Action::MoveBigWordRight => edit(EC::MoveBigWordRightStart { select: false }),
            Action::MoveWordRightStart => edit(EC::MoveWordRightStart { select: false }),
            Action::MoveWordRightEnd => edit(EC::MoveWordRightEnd { select: false }),
            Action::MoveBigWordRightStart => edit(EC::MoveBigWordRightStart { select: false }),
            Action::MoveBigWordRightEnd => edit(EC::MoveBigWordRightEnd { select: false }),
            Action::Backspace => edit(EC::Backspace),
            Action::Delete => edit(EC::Delete),
            Action::BackspaceWord => edit(EC::BackspaceWord),
            Action::DeleteWord => edit(EC::DeleteWord),
            Action::CutChar => edit(EC::CutChar),
            Action::Clear => edit(EC::Clear),
            Action::ClearToLineEnd => edit(EC::ClearToLineEnd),
            Action::CutCurrentLine => edit(EC::CutCurrentLine),
            Action::CutFromStart => edit(EC::CutFromStart),
            Action::CutFromLineStart => edit(EC::CutFromLineStart),
            Action::CutFromLineNonBlankStart => edit(EC::CutFromLineNonBlankStart),
            Action::CutToEnd => edit(EC::CutToEnd),
            Action::CutToLineEnd => edit(EC::CutToLineEnd),
            Action::KillLine => edit(EC::KillLine),
            Action::CutWordLeft => edit(EC::CutWordLeft),
            Action::CutWordRight => edit(EC::CutWordRight),
            Action::CutBigWordLeft => edit(EC::CutBigWordLeft),
            Action::CutBigWordRight => edit(EC::CutBigWordRight),
            Action::PasteCutBufferBefore => edit(EC::PasteCutBufferBefore),
            Action::PasteCutBufferAfter => edit(EC::PasteCutBufferAfter),
            Action::Paste => edit(EC::Paste),
            Action::Undo => edit(EC::Undo),
            Action::Redo => edit(EC::Redo),
            Action::UppercaseWord => edit(EC::UppercaseWord),
            Action::LowercaseWord => edit(EC::LowercaseWord),
            Action::CapitalizeChar => edit(EC::CapitalizeChar),
            Action::SwitchcaseChar => edit(EC::SwitchcaseChar),
            Action::SwapWords => edit(EC::SwapWords),
            Action::SwapGraphemes => edit(EC::SwapGraphemes),
            Action::SelectAll => edit(EC::SelectAll),
            Action::CopySelection => edit(EC::CopySelection),
            Action::CutSelection => edit(EC::CutSelection),
            Action::CopyFromStart => edit(EC::CopyFromStart),
            Action::CopyFromLineStart => edit(EC::CopyFromLineStart),
            Action::CopyFromLineNonBlankStart => edit(EC::CopyFromLineNonBlankStart),
            Action::CopyToEnd => edit(EC::CopyToEnd),
            Action::CopyToLineEnd => edit(EC::CopyToLineEnd),
            Action::CopyCurrentLine => edit(EC::CopyCurrentLine),
            Action::CopyWordLeft => edit(EC::CopyWordLeft),
            Action::CopyWordRight => edit(EC::CopyWordRight),
            Action::CopyBigWordLeft => edit(EC::CopyBigWordLeft),
            Action::CopyBigWordRight => edit(EC::CopyBigWordRight),
            Action::MoveLineUpSelect => edit(EC::MoveLineUp { select: true }),
            Action::MoveLineDownSelect => edit(EC::MoveLineDown { select: true }),
            Action::MoveLeftSelect => edit(EC::MoveLeft { select: true }),
            Action::MoveRightSelect => edit(EC::MoveRight { select: true }),
            Action::MoveWordLeftSelect => edit(EC::MoveWordLeft { select: true }),
            Action::MoveWordRightSelect => edit(EC::MoveWordRight { select: true }),
            Action::MoveToLineStartSelect => edit(EC::MoveToLineStart { select: true }),
            Action::MoveToLineEndSelect => edit(EC::MoveToLineEnd { select: true }),
            Action::MoveToStartSelect => edit(EC::MoveToStart { select: true }),
            Action::MoveToEndSelect => edit(EC::MoveToEnd { select: true }),
        }
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("alt".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("shift".to_owned());
        }
        parts.push(key_code_name(self.code));
        f.write_str(&parts.join("-"))
    }
}

fn add_event_bindings(
    keybindings: &mut reedline::Keybindings,
    bindings: &[KeyBinding],
    event: ReedlineEvent,
) {
    for binding in bindings {
        keybindings.add_binding(binding.modifiers, binding.code, event.clone());
    }
}

fn remove_event_bindings(keybindings: &mut reedline::Keybindings, event: &ReedlineEvent) {
    let keys = keybindings
        .get_keybindings()
        .iter()
        .filter_map(|(key, existing_event)| {
            (existing_event == event).then_some((key.modifier, key.key_code))
        })
        .collect::<Vec<_>>();

    for (modifiers, code) in keys {
        keybindings.remove_binding(modifiers, code);
    }
}

pub(super) fn apply_bindings_update(
    target: &mut Vec<KeyBinding>,
    operation: KeyBindingOperation,
    bindings: Vec<KeyBinding>,
) {
    match operation {
        KeyBindingOperation::Set => *target = dedup_bindings(bindings),
        KeyBindingOperation::Add => add_bindings(target, bindings),
        KeyBindingOperation::Remove => remove_bindings(target, &bindings),
    }
}

fn dedup_bindings(bindings: Vec<KeyBinding>) -> Vec<KeyBinding> {
    let mut unique = Vec::new();
    add_bindings(&mut unique, bindings);
    unique
}

fn add_bindings(target: &mut Vec<KeyBinding>, bindings: Vec<KeyBinding>) {
    for binding in bindings {
        if !target.contains(&binding) {
            target.push(binding);
        }
    }
}

fn remove_bindings(target: &mut Vec<KeyBinding>, bindings: &[KeyBinding]) {
    target.retain(|binding| !bindings.contains(binding));
}

fn key_bindings(names: &[&str]) -> Vec<KeyBinding> {
    names
        .iter()
        .map(|name| parse_key_binding(name).expect("default key binding is valid"))
        .collect()
}

pub(super) fn parse_key_binding(name: &str) -> Result<KeyBinding, String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("key binding cannot be empty".to_owned());
    }

    let mut modifiers = KeyModifiers::NONE;
    let mut key_name = normalized.as_str();
    loop {
        if let Some(rest) = key_name.strip_prefix("ctrl-") {
            modifiers.insert(KeyModifiers::CONTROL);
            key_name = rest;
        } else if let Some(rest) = key_name.strip_prefix("control-") {
            modifiers.insert(KeyModifiers::CONTROL);
            key_name = rest;
        } else if let Some(rest) = key_name.strip_prefix("alt-") {
            modifiers.insert(KeyModifiers::ALT);
            key_name = rest;
        } else if let Some(rest) = key_name.strip_prefix("shift-") {
            modifiers.insert(KeyModifiers::SHIFT);
            key_name = rest;
        } else {
            break;
        }
    }

    let code = match key_name {
        "left" => KeyCode::Left,
        "up" => KeyCode::Up,
        "right" => KeyCode::Right,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "page-up" => KeyCode::PageUp,
        "pagedown" | "page-down" => KeyCode::PageDown,
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        key if key.chars().count() == 1 => KeyCode::Char(
            key.chars()
                .next()
                .expect("single-character key has one character"),
        ),
        _ => return Err(format!("unknown key binding `{name}`")),
    };

    Ok(KeyBinding { code, modifiers })
}

fn modifiers_without_shift(mut modifiers: KeyModifiers) -> KeyModifiers {
    modifiers.remove(KeyModifiers::SHIFT);
    modifiers
}

fn key_code_name(code: KeyCode) -> String {
    match code {
        KeyCode::Left => "left".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Char(' ') => "space".to_owned(),
        KeyCode::Char(ch) => ch.to_string(),
        _ => format!("{code:?}").to_ascii_lowercase(),
    }
}
