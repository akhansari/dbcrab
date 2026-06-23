use std::{env, fmt, fs, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::errors::{AppError, AppResult};

pub const DEFAULT_KEYBINDINGS_TOML: &str = r#"[keybindings.tui]
left = ["left", "h"]
up = ["up", "k"]
right = ["right", "l"]
down = ["down", "j"]

half_page_left = ["shift-h"]
half_page_up = ["shift-k", "ctrl-u"]
half_page_right = ["shift-l"]
half_page_down = ["shift-j", "ctrl-d"]

full_page_left = ["ctrl-h"]
full_page_up = ["ctrl-k", "pageup"]
full_page_right = ["ctrl-l"]
full_page_down = ["ctrl-j", "pagedown"]

toggle_preview = ["enter"]
focus_next = ["tab"]

quit = ["q", "esc", "ctrl-c"]
"#;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct KeybindingsConfig {
    pub tui: TuiKeybindings,
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
            half_page_up: key_bindings(["shift-k", "ctrl-u"]),
            half_page_right: key_bindings(["shift-l"]),
            half_page_down: key_bindings(["shift-j", "ctrl-d"]),
            full_page_left: key_bindings(["ctrl-h"]),
            full_page_up: key_bindings(["ctrl-k", "pageup"]),
            full_page_right: key_bindings(["ctrl-l"]),
            full_page_down: key_bindings(["ctrl-j", "pagedown"]),
            toggle_preview: key_bindings(["enter"]),
            focus_next: key_bindings(["tab"]),
            quit: key_bindings(["q", "esc", "ctrl-c"]),
        }
    }
}

impl TuiKeybindings {
    pub fn action_for(&self, key: KeyEvent) -> Option<TuiAction> {
        TuiAction::ALL.into_iter().find(|action| {
            self.bindings_for(*action)
                .iter()
                .any(|binding| binding.matches(key))
        })
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
            TuiAction::Quit => self.quit = bindings,
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
    Quit,
}

impl TuiAction {
    const ALL: [Self; 15] = [
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
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    fn matches(self, key: KeyEvent) -> bool {
        match (self.code, key.code) {
            (KeyCode::Char(expected), KeyCode::Char(actual)) => {
                modifiers_without_shift(self.modifiers) == modifiers_without_shift(key.modifiers)
                    && if self.modifiers.contains(KeyModifiers::SHIFT) {
                        actual.eq_ignore_ascii_case(&expected)
                            && (key.modifiers.contains(KeyModifiers::SHIFT)
                                || actual.is_ascii_uppercase())
                    } else {
                        actual == expected && !key.modifiers.contains(KeyModifiers::SHIFT)
                    }
            }
            _ => self.code == key.code && self.modifiers == key.modifiers,
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
    let mut in_tui_keybindings = false;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = strip_comment(raw_line).trim().to_owned();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            in_tui_keybindings = line == "[keybindings.tui]";
            continue;
        }

        if !in_tui_keybindings {
            continue;
        }

        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {line_number}: expected `name = [\"key\"]`"))?;
        let action = action_from_name(name.trim()).ok_or_else(|| {
            format!(
                "line {line_number}: unknown [keybindings.tui] key `{}`",
                name.trim()
            )
        })?;
        let keys =
            parse_string_array(value.trim()).map_err(|err| format!("line {line_number}: {err}"))?;
        let bindings = keys
            .iter()
            .map(|key| parse_key_binding(key).map_err(|err| format!("line {line_number}: {err}")))
            .collect::<Result<Vec<_>, _>>()?;
        config.keybindings.tui.set_bindings(action, bindings);
    }

    config.keybindings.tui.validate()?;
    Ok(config)
}

fn action_from_name(name: &str) -> Option<TuiAction> {
    TuiAction::ALL
        .into_iter()
        .find(|action| action.name() == name)
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
        "pageup" | "page-up" => KeyCode::PageUp,
        "pagedown" | "page-down" => KeyCode::PageDown,
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
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
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
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

        // When
        let enter_action = keybindings.action_for(enter);
        let tab_action = keybindings.action_for(tab);

        // Then
        assert_eq!(enter_action, Some(TuiAction::TogglePreview));
        assert_eq!(tab_action, Some(TuiAction::FocusNext));
    }
}
