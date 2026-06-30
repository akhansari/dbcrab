use std::{env, fmt, fs, path::PathBuf};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use reedline::{EditCommand, ReedlineEvent};

use crate::errors::{AppError, AppResult};

pub const DEFAULT_KEYBINDINGS_TOML: &str = r#"# emacs | vi
edit_mode = "emacs"

[keybindings.prompt]
complete = ["tab", "ctrl-space"]
cycle_display = ["alt-v"]
command_mode = [":"]

[keybindings.remap]
# h.swap = "i"

[keybindings.prompt.insert]
Esc = ["esc"]
CtrlC = ["ctrl-c"]
CtrlD = ["ctrl-d"]
ClearScreen = ["ctrl-l"]
HistoryMenu = ["ctrl-r"]
OpenEditor = ["ctrl-o"]

Enter = ["enter", "ctrl-j"]
InsertNewline = ["alt-enter", "shift-enter"]

MoveWordLeft = ["ctrl-left"]
MoveWordRight = ["ctrl-right"]
MoveToLineStart = ["home", "ctrl-a"]
MoveToLineEnd = ["end", "ctrl-e"]
MoveToStart = ["ctrl-home"]
MoveToEnd = ["ctrl-end"]
ToStart = ["alt-<", "shift-alt-,"]
ToEnd = ["alt->", "shift-alt-."]

Backspace = ["backspace", "ctrl-h"]
Delete = ["delete"]
BackspaceWord = ["ctrl-backspace", "ctrl-w"]
DeleteWord = ["ctrl-delete"]

MoveLineUpSelect = ["shift-up"]
MoveLineDownSelect = ["shift-down"]
MoveLeftSelect = ["shift-left"]
MoveRightSelect = ["shift-right"]
MoveWordLeftSelect = ["shift-ctrl-left"]
MoveWordRightSelect = ["shift-ctrl-right"]
MoveToLineStartSelect = ["shift-home"]
MoveToLineEndSelect = ["shift-end"]
MoveToStartSelect = ["shift-ctrl-home"]
MoveToEndSelect = ["shift-ctrl-end"]
SelectAll = ["shift-ctrl-a"]

[keybindings.prompt.emacs]
MoveWordLeft.add = ["alt-left", "alt-b"]
MoveWordRight.add = ["alt-right", "alt-f"]
BackspaceWord.add = ["alt-backspace", "alt-m"]
DeleteWord.add = ["alt-delete"]

Redo = ["ctrl-g"]
Undo = ["ctrl-z"]
PasteCutBufferBefore = ["ctrl-y"]
CutWordLeft = ["ctrl-w"]
KillLine = ["ctrl-k"]
CutFromStart = ["ctrl-u"]
CutWordRight = ["alt-d"]
SwapGraphemes = ["ctrl-t"]
UppercaseWord = ["alt-u"]
LowercaseWord = ["alt-l"]
CapitalizeChar = ["alt-c"]

[keybindings.prompt.vi_insert]
# Inherits keybindings.prompt.insert.

[keybindings.prompt.vi_normal]
# Reedline's built-in vi grammar handles h/j/k/l, w, b, d, c, y, etc.

[keybindings.command]
complete = ["tab", "ctrl-space"]
cancel = ["esc", "ctrl-d"]

[keybindings.tui]
left = ["left", "h"]
up = ["up", "k"]
right = ["right", "l"]
down = ["down", "j"]

half_page_left = ["shift-h"]
half_page_up = ["shift-k"]
half_page_right = ["shift-l"]
half_page_down = ["shift-j", "ctrl-d"]

full_page_left = ["ctrl-h"]
full_page_up = ["ctrl-k", "pageup"]
full_page_right = ["ctrl-l"]
full_page_down = ["ctrl-j", "pagedown"]

toggle_preview = ["enter"]
focus_next = ["tab"]
edit_preview = ["c"]
stage_preview = ["ctrl-s"]
set_null = ["ctrl-x"]
update_row = ["ctrl-u"]

quit = ["q", "esc", "ctrl-c"]
"#;

pub(crate) const HISTORY_MENU: &str = "history_menu";

pub(crate) fn history_menu_event() -> ReedlineEvent {
    ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu(HISTORY_MENU.to_owned()),
        ReedlineEvent::MenuPageNext,
    ])
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub edit_mode: ConfigEditMode,
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct KeybindingsConfig {
    pub remaps: KeyRemaps,
    pub prompt: PromptKeybindings,
    pub command: CommandKeybindings,
    pub tui: TuiKeybindings,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct KeyRemaps {
    remaps: Vec<KeyRemap>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct KeyRemap {
    from: KeyBinding,
    to: KeyBinding,
}

impl KeyRemaps {
    pub(crate) fn set(&mut self, from: KeyBinding, to: KeyBinding) {
        self.remaps.retain(|remap| remap.from != from);
        self.remaps.push(KeyRemap { from, to });
    }

    pub fn remap_event(&self, event: Event) -> Event {
        match event {
            Event::Key(key) => Event::Key(self.remap_key_event(key)),
            event => event,
        }
    }

    pub fn remap_key_event(&self, key: KeyEvent) -> KeyEvent {
        if let Some(remap) = self
            .remaps
            .iter()
            .find(|remap| !remap.from.is_plain_char() && remap.from.matches(key))
        {
            return key_event_with_binding(key, remap.to);
        }

        if let Some(remap) = self.remaps.iter().find(|remap| remap.from.matches(key)) {
            return key_event_with_binding(key, remap.to);
        }

        self.remap_shifted_plain_char(key).unwrap_or(key)
    }

    pub(crate) fn remap_text_input_event(&self, event: Event) -> Event {
        match event {
            Event::Key(key) => Event::Key(self.remap_text_input_key_event(key)),
            event => event,
        }
    }

    pub(crate) fn remap_text_input_key_event(&self, key: KeyEvent) -> KeyEvent {
        if is_text_input_char_key(key) {
            return key;
        }

        self.remap_key_event(key)
    }

    fn remap_shifted_plain_char(&self, key: KeyEvent) -> Option<KeyEvent> {
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
        let remap = self.remaps.iter().find(|remap| {
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum ConfigEditMode {
    #[default]
    Emacs,
    Vi,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct LineEditorKeybindings {
    updates: Vec<LineEditorKeybindingUpdate>,
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
    pub insert: LineEditorKeybindings,
    pub emacs: LineEditorKeybindings,
    pub vi_insert: LineEditorKeybindings,
    pub vi_normal: LineEditorKeybindings,
}

impl Default for PromptKeybindings {
    fn default() -> Self {
        Self {
            complete: key_bindings(["tab", "ctrl-space"]),
            cycle_display: key_bindings(["alt-v"]),
            command_mode: key_bindings([":"]),
            insert: LineEditorKeybindings::default(),
            emacs: LineEditorKeybindings::default(),
            vi_insert: LineEditorKeybindings::default(),
            vi_normal: LineEditorKeybindings::default(),
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
            complete: key_bindings(["tab", "ctrl-space"]),
            cancel: key_bindings(["esc", "ctrl-d"]),
            editor: LineEditorKeybindings::default(),
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
    edit_preview: Vec<KeyBinding>,
    stage_preview: Vec<KeyBinding>,
    set_null: Vec<KeyBinding>,
    update_row: Vec<KeyBinding>,
    quit: Vec<KeyBinding>,
}

impl Default for TuiKeybindings {
    fn default() -> Self {
        Self {
            left: key_bindings(["left", "h"]),
            up: key_bindings(["up", "k"]),
            right: key_bindings(["right", "l"]),
            down: key_bindings(["down", "j"]),
            half_page_left: key_bindings(["shift-h"]),
            half_page_up: key_bindings(["shift-k"]),
            half_page_right: key_bindings(["shift-l"]),
            half_page_down: key_bindings(["shift-j", "ctrl-d"]),
            full_page_left: key_bindings(["ctrl-h"]),
            full_page_up: key_bindings(["ctrl-k", "pageup"]),
            full_page_right: key_bindings(["ctrl-l"]),
            full_page_down: key_bindings(["ctrl-j", "pagedown"]),
            toggle_preview: key_bindings(["enter"]),
            focus_next: key_bindings(["tab"]),
            edit_preview: key_bindings(["c"]),
            stage_preview: key_bindings(["ctrl-s"]),
            set_null: key_bindings(["ctrl-x"]),
            update_row: key_bindings(["ctrl-u"]),
            quit: key_bindings(["q", "esc", "ctrl-c"]),
        }
    }
}

impl TuiKeybindings {
    pub fn action_for(&self, key: KeyEvent) -> Option<TuiAction> {
        TuiAction::ALL
            .into_iter()
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

    fn bindings_for(&self, action: TuiAction) -> &[KeyBinding] {
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
            TuiAction::EditPreview => &self.edit_preview,
            TuiAction::StagePreview => &self.stage_preview,
            TuiAction::SetNull => &self.set_null,
            TuiAction::UpdateRow => &self.update_row,
            TuiAction::Quit => &self.quit,
        }
    }

    fn set_bindings(&mut self, action: TuiAction, bindings: Vec<KeyBinding>) {
        match action {
            TuiAction::Left => self.left = bindings,
            TuiAction::Up => self.up = bindings,
            TuiAction::Right => self.right = bindings,
            TuiAction::Down => self.down = bindings,
            TuiAction::HalfPageLeft => self.half_page_left = bindings,
            TuiAction::HalfPageUp => self.half_page_up = bindings,
            TuiAction::HalfPageRight => self.half_page_right = bindings,
            TuiAction::HalfPageDown => self.half_page_down = bindings,
            TuiAction::FullPageLeft => self.full_page_left = bindings,
            TuiAction::FullPageUp => self.full_page_up = bindings,
            TuiAction::FullPageRight => self.full_page_right = bindings,
            TuiAction::FullPageDown => self.full_page_down = bindings,
            TuiAction::TogglePreview => self.toggle_preview = bindings,
            TuiAction::FocusNext => self.focus_next = bindings,
            TuiAction::EditPreview => self.edit_preview = bindings,
            TuiAction::StagePreview => self.stage_preview = bindings,
            TuiAction::SetNull => self.set_null = bindings,
            TuiAction::UpdateRow => self.update_row = bindings,
            TuiAction::Quit => self.quit = bindings,
        }
    }

    fn apply_update(
        &mut self,
        action: TuiAction,
        operation: KeyBindingOperation,
        bindings: Vec<KeyBinding>,
    ) {
        match operation {
            KeyBindingOperation::Set => self.set_bindings(action, dedup_bindings(bindings)),
            KeyBindingOperation::Add => {
                let mut updated = self.bindings_for(action).to_vec();
                add_bindings(&mut updated, bindings);
                self.set_bindings(action, updated);
            }
            KeyBindingOperation::Remove => {
                let mut updated = self.bindings_for(action).to_vec();
                remove_bindings(&mut updated, &bindings);
                self.set_bindings(action, updated);
            }
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.quit.is_empty() {
            return Err("`quit` must have at least one key binding".to_owned());
        }

        let mut seen = Vec::new();
        for action in TuiAction::ALL {
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
    fn push_update(
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
                        keybindings.remove_binding(binding.modifiers, binding.code);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TuiAction {
    Left,
    Up,
    Right,
    Down,
    HalfPageLeft,
    HalfPageUp,
    HalfPageRight,
    HalfPageDown,
    FullPageLeft,
    FullPageUp,
    FullPageRight,
    FullPageDown,
    TogglePreview,
    FocusNext,
    EditPreview,
    StagePreview,
    SetNull,
    UpdateRow,
    Quit,
}

impl TuiAction {
    const ALL: [Self; 19] = [
        Self::Left,
        Self::Up,
        Self::Right,
        Self::Down,
        Self::HalfPageLeft,
        Self::HalfPageUp,
        Self::HalfPageRight,
        Self::HalfPageDown,
        Self::FullPageLeft,
        Self::FullPageUp,
        Self::FullPageRight,
        Self::FullPageDown,
        Self::TogglePreview,
        Self::FocusNext,
        Self::EditPreview,
        Self::StagePreview,
        Self::SetNull,
        Self::UpdateRow,
        Self::Quit,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Up => "up",
            Self::Right => "right",
            Self::Down => "down",
            Self::HalfPageLeft => "half_page_left",
            Self::HalfPageUp => "half_page_up",
            Self::HalfPageRight => "half_page_right",
            Self::HalfPageDown => "half_page_down",
            Self::FullPageLeft => "full_page_left",
            Self::FullPageUp => "full_page_up",
            Self::FullPageRight => "full_page_right",
            Self::FullPageDown => "full_page_down",
            Self::TogglePreview => "toggle_preview",
            Self::FocusNext => "focus_next",
            Self::EditPreview => "edit_preview",
            Self::StagePreview => "stage_preview",
            Self::SetNull => "set_null",
            Self::UpdateRow => "update_row",
            Self::Quit => "quit",
        }
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
enum KeyBindingOperation {
    Set,
    Add,
    Remove,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum KeyRemapOperation {
    Set,
    Swap,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ConfigSection {
    Root,
    Remap,
    Prompt,
    PromptInsert,
    PromptEmacs,
    PromptViInsert,
    PromptViNormal,
    Command,
    Tui,
    Ignored,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PromptAction {
    Complete,
    CycleDisplay,
    CommandMode,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CommandAction {
    Complete,
    Cancel,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LineEditorAction {
    Esc,
    CtrlC,
    CtrlD,
    ClearScreen,
    HistoryMenu,
    OpenEditor,
    Enter,
    InsertNewline,
    Up,
    Down,
    Left,
    Right,
    ToStart,
    ToEnd,
    MoveToStart,
    MoveToLineStart,
    MoveToLineNonBlankStart,
    MoveToEnd,
    MoveToLineEnd,
    MoveLineUp,
    MoveLineDown,
    MoveLeft,
    MoveRight,
    MoveWordLeft,
    MoveWordRight,
    MoveBigWordLeft,
    MoveBigWordRight,
    MoveWordRightStart,
    MoveWordRightEnd,
    MoveBigWordRightStart,
    MoveBigWordRightEnd,
    Backspace,
    Delete,
    BackspaceWord,
    DeleteWord,
    CutChar,
    Clear,
    ClearToLineEnd,
    CutCurrentLine,
    CutFromStart,
    CutFromLineStart,
    CutFromLineNonBlankStart,
    CutToEnd,
    CutToLineEnd,
    KillLine,
    CutWordLeft,
    CutWordRight,
    CutBigWordLeft,
    CutBigWordRight,
    PasteCutBufferBefore,
    PasteCutBufferAfter,
    Paste,
    Undo,
    Redo,
    UppercaseWord,
    LowercaseWord,
    CapitalizeChar,
    SwitchcaseChar,
    SwapWords,
    SwapGraphemes,
    SelectAll,
    CopySelection,
    CutSelection,
    CopyFromStart,
    CopyFromLineStart,
    CopyFromLineNonBlankStart,
    CopyToEnd,
    CopyToLineEnd,
    CopyCurrentLine,
    CopyWordLeft,
    CopyWordRight,
    CopyBigWordLeft,
    CopyBigWordRight,
    MoveLineUpSelect,
    MoveLineDownSelect,
    MoveLeftSelect,
    MoveRightSelect,
    MoveWordLeftSelect,
    MoveWordRightSelect,
    MoveToLineStartSelect,
    MoveToLineEndSelect,
    MoveToStartSelect,
    MoveToEndSelect,
}

impl LineEditorAction {
    fn event(self) -> ReedlineEvent {
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

pub fn load(path: Option<PathBuf>) -> AppResult<AppConfig> {
    let explicit_path = path.is_some();
    let Some(path) = path.or_else(default_config_path) else {
        return Ok(AppConfig::default());
    };

    if !path.exists() {
        return if explicit_path {
            Err(AppError::message(format!(
                "config file `{}` was not found",
                path.display()
            )))
        } else {
            Ok(AppConfig::default())
        };
    }

    let text = fs::read_to_string(&path).map_err(|err| {
        AppError::message(format!("failed to read config `{}`: {err}", path.display()))
    })?;

    parse_config(&text)
        .map_err(|err| AppError::message(format!("invalid config `{}`: {err}", path.display())))
}

fn default_config_path() -> Option<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME")
        && !config_home.is_empty()
    {
        return Some(PathBuf::from(config_home).join("dbcrab/config.toml"));
    }

    env::var_os("HOME").and_then(|home| {
        if home.is_empty() {
            None
        } else {
            Some(PathBuf::from(home).join(".config/dbcrab/config.toml"))
        }
    })
}

fn parse_config(text: &str) -> Result<AppConfig, String> {
    let mut config = AppConfig::default();
    let mut section = ConfigSection::Root;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = strip_comment(raw_line).trim().to_owned();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            section = config_section(&line);
            continue;
        }

        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {line_number}: expected `name = value`"))?;

        match section {
            ConfigSection::Root => parse_root_setting(&mut config, line_number, name, value)?,
            ConfigSection::Remap => {
                parse_key_remap(&mut config.keybindings.remaps, line_number, name, value)?
            }
            ConfigSection::Prompt => {
                parse_prompt_keybinding(&mut config.keybindings.prompt, line_number, name, value)?
            }
            ConfigSection::PromptInsert => parse_line_editor_keybinding(
                &mut config.keybindings.prompt.insert,
                line_number,
                name,
                value,
                "keybindings.prompt.insert",
            )?,
            ConfigSection::PromptEmacs => parse_line_editor_keybinding(
                &mut config.keybindings.prompt.emacs,
                line_number,
                name,
                value,
                "keybindings.prompt.emacs",
            )?,
            ConfigSection::PromptViInsert => parse_line_editor_keybinding(
                &mut config.keybindings.prompt.vi_insert,
                line_number,
                name,
                value,
                "keybindings.prompt.vi_insert",
            )?,
            ConfigSection::PromptViNormal => parse_line_editor_keybinding(
                &mut config.keybindings.prompt.vi_normal,
                line_number,
                name,
                value,
                "keybindings.prompt.vi_normal",
            )?,
            ConfigSection::Command => {
                parse_command_keybinding(&mut config.keybindings.command, line_number, name, value)?
            }
            ConfigSection::Tui => {
                let (action, operation, bindings) = parse_tui_keybinding(line_number, name, value)?;
                config
                    .keybindings
                    .tui
                    .apply_update(action, operation, bindings);
            }
            ConfigSection::Ignored => {}
        }
    }

    config.keybindings.tui.validate()?;
    Ok(config)
}

fn config_section(line: &str) -> ConfigSection {
    match line {
        "[keybindings.remap]" => ConfigSection::Remap,
        "[keybindings.prompt]" => ConfigSection::Prompt,
        "[keybindings.prompt.insert]" => ConfigSection::PromptInsert,
        "[keybindings.prompt.emacs]" => ConfigSection::PromptEmacs,
        "[keybindings.prompt.vi_insert]" => ConfigSection::PromptViInsert,
        "[keybindings.prompt.vi_normal]" => ConfigSection::PromptViNormal,
        "[keybindings.command]" => ConfigSection::Command,
        "[keybindings.tui]" => ConfigSection::Tui,
        _ => ConfigSection::Ignored,
    }
}

fn parse_root_setting(
    config: &mut AppConfig,
    line_number: usize,
    name: &str,
    value: &str,
) -> Result<(), String> {
    match normalize_name(name).as_str() {
        "editmode" => {
            let value = parse_string_value(value.trim())
                .map_err(|err| format!("line {line_number}: {err}"))?;
            config.edit_mode = parse_edit_mode(&value)
                .ok_or_else(|| format!("line {line_number}: edit_mode must be `emacs` or `vi`"))?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_edit_mode(value: &str) -> Option<ConfigEditMode> {
    match normalize_name(value).as_str() {
        "emacs" => Some(ConfigEditMode::Emacs),
        "vi" => Some(ConfigEditMode::Vi),
        _ => None,
    }
}

fn parse_key_remap(
    remaps: &mut KeyRemaps,
    line_number: usize,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let (name, operation) = parse_key_remap_operation(name);
    let from = parse_key_name(name)
        .and_then(|name| parse_key_binding(&name))
        .map_err(|err| format!("line {line_number}: {err}"))?;
    let to = parse_string_value(value.trim())
        .and_then(|name| parse_key_binding(&name))
        .map_err(|err| format!("line {line_number}: {err}"))?;
    remaps.set(from, to);
    if operation == KeyRemapOperation::Swap {
        remaps.set(to, from);
    }
    Ok(())
}

fn parse_key_remap_operation(name: &str) -> (&str, KeyRemapOperation) {
    let name = name.trim();
    if let Some(key) = name.strip_suffix(".swap") {
        (key.trim(), KeyRemapOperation::Swap)
    } else {
        (name, KeyRemapOperation::Set)
    }
}

fn parse_prompt_keybinding(
    keybindings: &mut PromptKeybindings,
    line_number: usize,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let (action_name, operation) = parse_keybinding_operation(name);
    let action = prompt_action_from_name(action_name).ok_or_else(|| {
        format!(
            "line {line_number}: unknown [keybindings.prompt] key `{}`",
            action_name
        )
    })?;
    let bindings = parse_key_bindings_value(line_number, value)?;
    let target = match action {
        PromptAction::Complete => &mut keybindings.complete,
        PromptAction::CycleDisplay => &mut keybindings.cycle_display,
        PromptAction::CommandMode => &mut keybindings.command_mode,
    };
    apply_bindings_update(target, operation, bindings);
    Ok(())
}

fn parse_line_editor_keybinding(
    keybindings: &mut LineEditorKeybindings,
    line_number: usize,
    name: &str,
    value: &str,
    section: &str,
) -> Result<(), String> {
    let (action_name, operation) = parse_keybinding_operation(name);
    let action = line_editor_action_from_name(action_name).ok_or_else(|| {
        format!(
            "line {line_number}: unknown [{section}] key `{}`",
            action_name
        )
    })?;
    let bindings = parse_key_bindings_value(line_number, value)?;
    keybindings.push_update(action, operation, bindings);
    Ok(())
}

fn parse_command_keybinding(
    keybindings: &mut CommandKeybindings,
    line_number: usize,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let (action_name, operation) = parse_keybinding_operation(name);
    let bindings = parse_key_bindings_value(line_number, value)?;

    if let Some(action) = command_action_from_name(action_name) {
        let target = match action {
            CommandAction::Complete => &mut keybindings.complete,
            CommandAction::Cancel => &mut keybindings.cancel,
        };
        apply_bindings_update(target, operation, bindings);
        return Ok(());
    }

    let action = line_editor_action_from_name(action_name).ok_or_else(|| {
        format!(
            "line {line_number}: unknown [keybindings.command] key `{}`",
            action_name
        )
    })?;
    keybindings.editor.push_update(action, operation, bindings);
    Ok(())
}

fn parse_tui_keybinding(
    line_number: usize,
    name: &str,
    value: &str,
) -> Result<(TuiAction, KeyBindingOperation, Vec<KeyBinding>), String> {
    let (action_name, operation) = parse_keybinding_operation(name);
    let action = tui_action_from_name(action_name).ok_or_else(|| {
        format!(
            "line {line_number}: unknown [keybindings.tui] key `{}`",
            action_name
        )
    })?;
    let bindings = parse_key_bindings_value(line_number, value)?;
    Ok((action, operation, bindings))
}

fn parse_keybinding_operation(name: &str) -> (&str, KeyBindingOperation) {
    let name = name.trim();
    if let Some(action) = name.strip_suffix(".add") {
        (action.trim(), KeyBindingOperation::Add)
    } else if let Some(action) = name.strip_suffix(".remove") {
        (action.trim(), KeyBindingOperation::Remove)
    } else if let Some(action) = name.strip_suffix(".set") {
        (action.trim(), KeyBindingOperation::Set)
    } else {
        (name, KeyBindingOperation::Set)
    }
}

fn parse_key_bindings_value(line_number: usize, value: &str) -> Result<Vec<KeyBinding>, String> {
    let keys =
        parse_string_array(value.trim()).map_err(|err| format!("line {line_number}: {err}"))?;
    keys.iter()
        .map(|key| parse_key_binding(key).map_err(|err| format!("line {line_number}: {err}")))
        .collect()
}

fn tui_action_from_name(name: &str) -> Option<TuiAction> {
    let name = normalize_name(name);
    TuiAction::ALL
        .into_iter()
        .find(|action| normalize_name(action.name()) == name)
}

fn prompt_action_from_name(name: &str) -> Option<PromptAction> {
    match normalize_name(name).as_str() {
        "complete" => Some(PromptAction::Complete),
        "cycledisplay" => Some(PromptAction::CycleDisplay),
        "commandmode" => Some(PromptAction::CommandMode),
        _ => None,
    }
}

fn command_action_from_name(name: &str) -> Option<CommandAction> {
    match normalize_name(name).as_str() {
        "complete" => Some(CommandAction::Complete),
        "cancel" => Some(CommandAction::Cancel),
        _ => None,
    }
}

fn line_editor_action_from_name(name: &str) -> Option<LineEditorAction> {
    use LineEditorAction as Action;

    match normalize_name(name).as_str() {
        "esc" | "escape" => Some(Action::Esc),
        "ctrlc" | "controlc" => Some(Action::CtrlC),
        "ctrld" | "controld" => Some(Action::CtrlD),
        "clearscreen" => Some(Action::ClearScreen),
        "historymenu" | "searchhistory" => Some(Action::HistoryMenu),
        "openeditor" => Some(Action::OpenEditor),
        "enter" => Some(Action::Enter),
        "insertnewline" => Some(Action::InsertNewline),
        "up" => Some(Action::Up),
        "down" => Some(Action::Down),
        "left" => Some(Action::Left),
        "right" => Some(Action::Right),
        "tostart" => Some(Action::ToStart),
        "toend" => Some(Action::ToEnd),
        "movetostart" => Some(Action::MoveToStart),
        "movetolinestart" | "linestart" => Some(Action::MoveToLineStart),
        "movetolinenonblankstart" => Some(Action::MoveToLineNonBlankStart),
        "movetoend" => Some(Action::MoveToEnd),
        "movetolineend" | "lineend" => Some(Action::MoveToLineEnd),
        "movelineup" => Some(Action::MoveLineUp),
        "movelinedown" => Some(Action::MoveLineDown),
        "moveleft" => Some(Action::MoveLeft),
        "moveright" => Some(Action::MoveRight),
        "movewordleft" | "wordleft" => Some(Action::MoveWordLeft),
        "movewordright" | "wordright" => Some(Action::MoveWordRight),
        "movebigwordleft" => Some(Action::MoveBigWordLeft),
        "movebigwordright" => Some(Action::MoveBigWordRight),
        "movewordrightstart" => Some(Action::MoveWordRightStart),
        "movewordrightend" => Some(Action::MoveWordRightEnd),
        "movebigwordrightstart" => Some(Action::MoveBigWordRightStart),
        "movebigwordrightend" => Some(Action::MoveBigWordRightEnd),
        "backspace" => Some(Action::Backspace),
        "delete" => Some(Action::Delete),
        "backspaceword" => Some(Action::BackspaceWord),
        "deleteword" => Some(Action::DeleteWord),
        "cutchar" => Some(Action::CutChar),
        "clear" => Some(Action::Clear),
        "cleartolineend" => Some(Action::ClearToLineEnd),
        "cutcurrentline" => Some(Action::CutCurrentLine),
        "cutfromstart" => Some(Action::CutFromStart),
        "cutfromlinestart" => Some(Action::CutFromLineStart),
        "cutfromlinenonblankstart" => Some(Action::CutFromLineNonBlankStart),
        "cuttoend" => Some(Action::CutToEnd),
        "cuttolineend" => Some(Action::CutToLineEnd),
        "killline" => Some(Action::KillLine),
        "cutwordleft" => Some(Action::CutWordLeft),
        "cutwordright" => Some(Action::CutWordRight),
        "cutbigwordleft" => Some(Action::CutBigWordLeft),
        "cutbigwordright" => Some(Action::CutBigWordRight),
        "pastecutbufferbefore" => Some(Action::PasteCutBufferBefore),
        "pastecutbufferafter" => Some(Action::PasteCutBufferAfter),
        "paste" => Some(Action::Paste),
        "undo" => Some(Action::Undo),
        "redo" => Some(Action::Redo),
        "uppercaseword" => Some(Action::UppercaseWord),
        "lowercaseword" => Some(Action::LowercaseWord),
        "capitalizechar" => Some(Action::CapitalizeChar),
        "switchcasechar" => Some(Action::SwitchcaseChar),
        "swapwords" => Some(Action::SwapWords),
        "swapgraphemes" => Some(Action::SwapGraphemes),
        "selectall" => Some(Action::SelectAll),
        "copyselection" => Some(Action::CopySelection),
        "cutselection" => Some(Action::CutSelection),
        "copyfromstart" => Some(Action::CopyFromStart),
        "copyfromlinestart" => Some(Action::CopyFromLineStart),
        "copyfromlinenonblankstart" => Some(Action::CopyFromLineNonBlankStart),
        "copytoend" => Some(Action::CopyToEnd),
        "copytolineend" => Some(Action::CopyToLineEnd),
        "copycurrentline" => Some(Action::CopyCurrentLine),
        "copywordleft" => Some(Action::CopyWordLeft),
        "copywordright" => Some(Action::CopyWordRight),
        "copybigwordleft" => Some(Action::CopyBigWordLeft),
        "copybigwordright" => Some(Action::CopyBigWordRight),
        "movelineupselect" | "selectup" => Some(Action::MoveLineUpSelect),
        "movelinedownselect" | "selectdown" => Some(Action::MoveLineDownSelect),
        "moveleftselect" | "selectleft" => Some(Action::MoveLeftSelect),
        "moverightselect" | "selectright" => Some(Action::MoveRightSelect),
        "movewordleftselect" | "selectwordleft" => Some(Action::MoveWordLeftSelect),
        "movewordrightselect" | "selectwordright" => Some(Action::MoveWordRightSelect),
        "movetolinestartselect" | "selectlinestart" => Some(Action::MoveToLineStartSelect),
        "movetolineendselect" | "selectlineend" => Some(Action::MoveToLineEndSelect),
        "movetostartselect" | "selectbufferstart" => Some(Action::MoveToStartSelect),
        "movetoendselect" | "selectbufferend" => Some(Action::MoveToEndSelect),
        _ => None,
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

fn apply_bindings_update(
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
    bindings
        .into_iter()
        .fold(Vec::new(), |mut unique, binding| {
            if !unique.contains(&binding) {
                unique.push(binding);
            }
            unique
        })
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

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn parse_string_value(value: &str) -> Result<String, String> {
    let mut chars = value.chars().peekable();
    skip_whitespace(&mut chars);
    let value = parse_quoted_string(&mut chars)?;
    skip_whitespace(&mut chars);

    if chars.peek().is_none() {
        Ok(value)
    } else {
        Err("unexpected text after string".to_owned())
    }
}

fn parse_key_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.starts_with('"') {
        parse_string_value(name)
    } else if name.is_empty() {
        Err("key binding cannot be empty".to_owned())
    } else {
        Ok(name.to_owned())
    }
}

fn strip_comment(line: &str) -> String {
    let mut output = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                output.push(ch);
                escaped = true;
            }
            '"' => {
                output.push(ch);
                in_string = !in_string;
            }
            '#' if !in_string => break,
            _ => output.push(ch),
        }
    }

    output
}

fn parse_string_array(value: &str) -> Result<Vec<String>, String> {
    let mut chars = value.chars().peekable();
    skip_whitespace(&mut chars);
    expect_char(&mut chars, '[')?;
    skip_whitespace(&mut chars);

    let mut values = Vec::new();
    if consume_char(&mut chars, ']') {
        skip_whitespace(&mut chars);
        return if chars.peek().is_none() {
            Ok(values)
        } else {
            Err("unexpected text after array".to_owned())
        };
    }

    loop {
        values.push(parse_quoted_string(&mut chars)?);
        skip_whitespace(&mut chars);

        if consume_char(&mut chars, ']') {
            break;
        }

        expect_char(&mut chars, ',')?;
        skip_whitespace(&mut chars);
    }

    skip_whitespace(&mut chars);
    if chars.peek().is_none() {
        Ok(values)
    } else {
        Err("unexpected text after array".to_owned())
    }
}

fn parse_quoted_string(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, String> {
    expect_char(chars, '"')?;
    let mut value = String::new();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Ok(value),
            '\\' => value.push(parse_escape(chars)?),
            _ => value.push(ch),
        }
    }

    Err("unterminated string".to_owned())
}

fn parse_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<char, String> {
    match chars.next() {
        Some('"') => Ok('"'),
        Some('\\') => Ok('\\'),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some(ch) => Err(format!("unsupported escape `\\{ch}`")),
        None => Err("unterminated escape".to_owned()),
    }
}

fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
        chars.next();
    }
}

fn expect_char(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    expected: char,
) -> Result<(), String> {
    if consume_char(chars, expected) {
        Ok(())
    } else {
        Err(format!("expected `{expected}`"))
    }
}

fn consume_char(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, expected: char) -> bool {
    if chars.peek() == Some(&expected) {
        chars.next();
        true
    } else {
        false
    }
}

fn key_bindings<const N: usize>(names: [&str; N]) -> Vec<KeyBinding> {
    names
        .into_iter()
        .map(|name| parse_key_binding(name).expect("default key binding is valid"))
        .collect()
}

fn parse_key_binding(name: &str) -> Result<KeyBinding, String> {
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

fn is_text_input_char_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char(_)) && modifiers_without_shift(key.modifiers).is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keybindings_toml_uses_tui_keybindings_table() {
        // Given
        let defaults = DEFAULT_KEYBINDINGS_TOML;

        // When
        let has_tui_table = defaults.contains("[keybindings.tui]");

        // Then
        assert!(has_tui_table);
    }

    #[test]
    fn default_keybindings_toml_parses() {
        // Given
        let defaults = DEFAULT_KEYBINDINGS_TOML;

        // When
        let config = parse_config(defaults).expect("default keybindings should parse");

        // Then
        assert_eq!(config.edit_mode, ConfigEditMode::Emacs);
    }

    #[test]
    fn config_parses_vi_edit_mode() {
        // Given
        let text = "edit_mode = \"vi\"\n";

        // When
        let config = parse_config(text).expect("config should parse");

        // Then
        assert_eq!(config.edit_mode, ConfigEditMode::Vi);
    }

    #[test]
    fn key_remap_swaps_plain_characters() {
        // Given
        let text = "[keybindings.remap]\nh = \"i\"\ni = \"h\"\n";
        let h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        let i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);

        // When
        let config = parse_config(text).expect("config should parse");
        let remapped_h = config.keybindings.remaps.remap_key_event(h);
        let remapped_i = config.keybindings.remaps.remap_key_event(i);

        // Then
        assert_eq!(remapped_h.code, KeyCode::Char('i'));
        assert_eq!(remapped_i.code, KeyCode::Char('h'));
    }

    #[test]
    fn key_remap_swap_syntax_adds_bidirectional_remaps() {
        // Given
        let text = "[keybindings.remap]\nh.swap = \"i\"\n";
        let h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        let i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);

        // When
        let config = parse_config(text).expect("config should parse");
        let remapped_h = config.keybindings.remaps.remap_key_event(h);
        let remapped_i = config.keybindings.remaps.remap_key_event(i);

        // Then
        assert_eq!(remapped_h.code, KeyCode::Char('i'));
        assert_eq!(remapped_i.code, KeyCode::Char('h'));
    }

    #[test]
    fn key_remap_preserves_shifted_character_intent() {
        // Given
        let text = "[keybindings.remap]\nh = \"i\"\n";
        let shifted_h = KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE);

        // When
        let config = parse_config(text).expect("config should parse");
        let remapped = config.keybindings.remaps.remap_key_event(shifted_h);

        // Then
        assert_eq!(remapped.code, KeyCode::Char('i'));
        assert!(remapped.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn key_remap_supports_explicit_modified_keys() {
        // Given
        let text = "[keybindings.remap]\nctrl-h = \"ctrl-i\"\n";
        let ctrl_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);

        // When
        let config = parse_config(text).expect("config should parse");
        let remapped = config.keybindings.remaps.remap_key_event(ctrl_h);

        // Then
        assert_eq!(remapped.code, KeyCode::Char('i'));
        assert_eq!(remapped.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn key_remap_swap_syntax_supports_explicit_modified_keys() {
        // Given
        let text = "[keybindings.remap]\nctrl-h.swap = \"ctrl-i\"\n";
        let ctrl_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL);

        // When
        let config = parse_config(text).expect("config should parse");
        let remapped = config.keybindings.remaps.remap_key_event(ctrl_i);

        // Then
        assert_eq!(remapped.code, KeyCode::Char('h'));
        assert_eq!(remapped.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn config_overrides_default_binding_for_action() {
        // Given
        let text = "[keybindings.tui]\nleft = [\"a\"]\n";
        let left_key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let old_left_key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);

        // When
        let config = parse_config(text).expect("config should parse");

        // Then
        assert_eq!(
            config.keybindings.tui.action_for(left_key),
            Some(TuiAction::Left)
        );
        assert_eq!(config.keybindings.tui.action_for(old_left_key), None);
    }

    #[test]
    fn config_keeps_default_bindings_for_omitted_actions() {
        // Given
        let text = "[keybindings.tui]\nleft = [\"a\"]\n";
        let down_key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);

        // When
        let config = parse_config(text).expect("config should parse");

        // Then
        assert_eq!(
            config.keybindings.tui.action_for(down_key),
            Some(TuiAction::Down)
        );
    }

    #[test]
    fn config_rejects_duplicate_keybindings() {
        // Given
        let text = "[keybindings.tui]\nleft = [\"x\"]\nright = [\"x\"]\n";

        // When
        let result = parse_config(text);

        // Then
        assert!(result.is_err());
    }

    #[test]
    fn prompt_keybindings_can_patch_defaults() {
        // Given
        let text = "[keybindings.prompt]\ncomplete.remove = [\"ctrl-space\"]\ncomplete.add = [\"ctrl-x\"]\n";
        let ctrl_space = parse_key_binding("ctrl-space").expect("binding should parse");
        let ctrl_x = parse_key_binding("ctrl-x").expect("binding should parse");

        // When
        let config = parse_config(text).expect("config should parse");

        // Then
        assert!(!config.keybindings.prompt.complete.contains(&ctrl_space));
        assert!(config.keybindings.prompt.complete.contains(&ctrl_x));
    }

    #[test]
    fn line_editor_keybindings_patch_reedline_defaults() {
        // Given
        let text = "[keybindings.prompt.insert]\nClearScreen = [\"ctrl-x\"]\n";
        let mut keybindings = reedline::default_emacs_keybindings();

        // When
        let config = parse_config(text).expect("config should parse");
        config.keybindings.prompt.insert.apply_to(&mut keybindings);

        // Then
        assert_eq!(
            keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char('x')),
            Some(ReedlineEvent::ClearScreen)
        );
        assert_eq!(
            keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char('l')),
            None
        );
    }

    #[test]
    fn key_binding_matches_shift_char_sent_as_uppercase() {
        // Given
        let binding = parse_key_binding("shift-h").expect("binding should parse");
        let key = KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE);

        // When
        let matches = binding.matches(key);

        // Then
        assert!(matches);
    }

    #[test]
    fn key_binding_matches_shifted_punctuation() {
        // Given
        let binding = parse_key_binding(":").expect("binding should parse");
        let key = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::SHIFT);

        // When
        let matches = binding.matches(key);

        // Then
        assert!(matches);
    }

    #[test]
    fn key_binding_parses_ctrl_and_named_keys() {
        // Given
        let key = KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL);

        // When
        let binding = parse_key_binding("ctrl-pageup").expect("binding should parse");

        // Then
        assert!(binding.matches(key));
    }

    #[test]
    fn default_keybindings_include_preview_controls() {
        // Given
        let keybindings = TuiKeybindings::default();
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);

        // When
        let enter_action = keybindings.action_for(enter);
        let tab_action = keybindings.action_for(tab);
        let edit_action = keybindings.action_for(c);
        let stage_action = keybindings.action_for(ctrl_s);
        let null_action = keybindings.action_for(ctrl_x);
        let update_action = keybindings.action_for(ctrl_u);

        // Then
        assert_eq!(enter_action, Some(TuiAction::TogglePreview));
        assert_eq!(tab_action, Some(TuiAction::FocusNext));
        assert_eq!(edit_action, Some(TuiAction::EditPreview));
        assert_eq!(stage_action, Some(TuiAction::StagePreview));
        assert_eq!(null_action, Some(TuiAction::SetNull));
        assert_eq!(update_action, Some(TuiAction::UpdateRow));
    }
}
