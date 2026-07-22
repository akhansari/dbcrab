use std::{fmt, fs, path::PathBuf};

use crossterm::event::KeyModifiers;
use kdl::{KdlDocument, KdlNode};

use crate::{
    errors::{AppError, AppResult},
    paths,
};

use super::keybindings::{
    AppConfig, CommandAction, CommandKeybindings, ConfigEditMode, EditorKeybindings, KeyBinding,
    KeyBindingOperation, KeyRemapOperation, KeyRemapScope, KeyRemaps, KeybindingsConfig,
    LineEditorAction, LineEditorKeybindings, PromptAction, PromptKeybindings, TuiAction,
    TuiKeybindings, ViModeSelection, apply_bindings_update, parse_key_binding,
};

pub fn load(path: Option<PathBuf>) -> AppResult<AppConfig> {
    let explicit_path = path.is_some();
    let Some(path) = path.or_else(paths::default_config_path) else {
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
        .map_err(|err| AppError::message(format!("Invalid config `{}`:\n{err}", path.display())))
}

pub(super) fn parse_config(text: &str) -> Result<AppConfig, String> {
    let document = text
        .parse::<KdlDocument>()
        .map_err(|err| format!("KDL parse error: {err}"))?;
    let mut config = AppConfig::default();
    let mut edit_mode_seen = false;
    let mut keybindings_seen = false;

    for node in document.nodes() {
        match node.name().value() {
            "edit-mode" => {
                reject_duplicate(text, node, "edit-mode", &mut edit_mode_seen)?;
                config.edit_mode = parse_edit_mode_node(text, node)?;
            }
            "keybindings" => {
                reject_duplicate(text, node, "keybindings", &mut keybindings_seen)?;
                parse_keybindings(text, node, &mut config.keybindings)?;
            }
            name => {
                return Err(node_error(
                    text,
                    node,
                    format!("unknown top-level node `{name}`"),
                ));
            }
        }
    }

    config.keybindings.tui.validate()?;
    Ok(config)
}

fn parse_edit_mode_node(text: &str, node: &KdlNode) -> Result<ConfigEditMode, String> {
    validate_leaf_node(text, node, "edit-mode")?;
    let values = string_arguments(text, node, "edit-mode")?;
    if values.len() != 1 {
        return Err(node_error(
            text,
            node,
            "`edit-mode` requires exactly one value",
        ));
    }

    match values[0] {
        "emacs" => Ok(ConfigEditMode::Emacs),
        "vi" => Ok(ConfigEditMode::Vi),
        _ => Err(node_error(
            text,
            node,
            "`edit-mode` must be `emacs` or `vi`",
        )),
    }
}

fn parse_keybindings(
    text: &str,
    node: &KdlNode,
    keybindings: &mut KeybindingsConfig,
) -> Result<(), String> {
    let children = group_children(text, node, "keybindings")?;
    let mut editor_seen = false;
    let mut prompt_seen = false;
    let mut shortcut_nav_seen = false;
    let mut command_seen = false;
    let mut tui_seen = false;
    let mut normal_remaps_seen = false;
    let mut visual_remaps_seen = false;

    for child in children {
        match child.name().value() {
            "editor" => {
                reject_duplicate(text, child, "keybindings.editor", &mut editor_seen)?;
                parse_editor(text, child, &mut keybindings.editor)?;
            }
            "prompt" => {
                reject_duplicate(text, child, "keybindings.prompt", &mut prompt_seen)?;
                parse_prompt(text, child, &mut keybindings.prompt)?;
            }
            "vi-remap" => parse_vi_remap(
                text,
                child,
                &mut keybindings.remaps,
                &mut normal_remaps_seen,
                &mut visual_remaps_seen,
            )?,
            "shortcut-nav-remap" => {
                reject_duplicate(
                    text,
                    child,
                    "keybindings.shortcut-nav-remap",
                    &mut shortcut_nav_seen,
                )?;
                parse_shortcut_nav_remap(text, child, &mut keybindings.remaps)?;
            }
            "command" => {
                reject_duplicate(text, child, "keybindings.command", &mut command_seen)?;
                parse_command(text, child, &mut keybindings.command)?;
            }
            "tui" => {
                reject_duplicate(text, child, "keybindings.tui", &mut tui_seen)?;
                parse_tui(text, child, &mut keybindings.tui)?;
            }
            name => {
                return Err(node_error(
                    text,
                    child,
                    format!("unknown `keybindings` node `{name}`"),
                ));
            }
        }
    }

    Ok(())
}

fn parse_editor(text: &str, node: &KdlNode, editor: &mut EditorKeybindings) -> Result<(), String> {
    let children = group_children(text, node, "keybindings.editor")?;
    let mut emacs_vi_insert_seen = false;
    let mut vi_normal_seen = false;

    for child in children {
        match child.name().value() {
            "emacs-vi-insert" => {
                reject_duplicate(
                    text,
                    child,
                    "keybindings.editor.emacs-vi-insert",
                    &mut emacs_vi_insert_seen,
                )?;
                parse_line_editor_group(
                    text,
                    child,
                    &mut editor.emacs_vi_insert,
                    "keybindings.editor.emacs-vi-insert",
                )?;
            }
            "vi-normal" => {
                reject_duplicate(
                    text,
                    child,
                    "keybindings.editor.vi-normal",
                    &mut vi_normal_seen,
                )?;
                parse_line_editor_group(
                    text,
                    child,
                    &mut editor.vi_normal,
                    "keybindings.editor.vi-normal",
                )?;
            }
            name => {
                return Err(node_error(
                    text,
                    child,
                    format!("unknown `keybindings.editor` node `{name}`"),
                ));
            }
        }
    }

    Ok(())
}

fn parse_prompt(text: &str, node: &KdlNode, prompt: &mut PromptKeybindings) -> Result<(), String> {
    for child in group_children(text, node, "keybindings.prompt")? {
        let path = format!("keybindings.prompt.{}", child.name().value());
        let action = PromptAction::from_name(child.name().value()).ok_or_else(|| {
            node_error(
                text,
                child,
                format!(
                    "unknown `keybindings.prompt` action `{}`",
                    child.name().value()
                ),
            )
        })?;
        let (operation, bindings) = parse_binding_update(text, child, &path)?;
        let target = match action {
            PromptAction::Complete => &mut prompt.complete,
            PromptAction::CycleDisplay => &mut prompt.cycle_display,
            PromptAction::CommandMode => &mut prompt.command_mode,
        };
        apply_bindings_update(target, operation, bindings);
    }

    Ok(())
}

fn parse_line_editor_group(
    text: &str,
    node: &KdlNode,
    editor: &mut LineEditorKeybindings,
    path: &str,
) -> Result<(), String> {
    for child in group_children(text, node, path)? {
        let action = LineEditorAction::from_name(child.name().value()).ok_or_else(|| {
            node_error(
                text,
                child,
                format!("unknown `{path}` action `{}`", child.name().value()),
            )
        })?;
        let action_path = format!("{path}.{}", child.name().value());
        let (operation, bindings) = parse_binding_update(text, child, &action_path)?;
        editor.push_update(action, operation, bindings);
    }

    Ok(())
}

fn parse_vi_remap(
    text: &str,
    node: &KdlNode,
    remaps: &mut KeyRemaps,
    normal_seen: &mut bool,
    visual_seen: &mut bool,
) -> Result<(), String> {
    let modes = parse_vi_modes(text, node)?;
    if modes.normal && *normal_seen {
        return Err(node_error(
            text,
            node,
            "overlapping `vi-remap` mode `normal`",
        ));
    }
    if modes.visual && *visual_seen {
        return Err(node_error(
            text,
            node,
            "overlapping `vi-remap` mode `visual`",
        ));
    }
    *normal_seen |= modes.normal;
    *visual_seen |= modes.visual;

    let children = node
        .children()
        .ok_or_else(|| node_error(text, node, "`vi-remap` requires a children block"))?;
    for child in children.nodes() {
        let (from, to, operation) = parse_remap(text, child, "keybindings.vi-remap")?;
        if has_ctrl_or_alt(from) {
            return Err(node_error(
                text,
                child,
                "`vi-remap` sources cannot contain Ctrl or Alt",
            ));
        }
        if operation == KeyRemapOperation::Swap && has_ctrl_or_alt(to) {
            return Err(node_error(
                text,
                child,
                "both sides of a `vi-remap` swap must omit Ctrl and Alt",
            ));
        }
        if modes.normal {
            apply_remap(remaps, KeyRemapScope::Normal, from, to, operation);
        }
        if modes.visual {
            apply_remap(remaps, KeyRemapScope::Visual, from, to, operation);
        }
    }

    Ok(())
}

fn parse_vi_modes(text: &str, node: &KdlNode) -> Result<ViModeSelection, String> {
    validate_node_type(text, node, "keybindings.vi-remap")?;
    if node.children().is_none() {
        return Err(node_error(
            text,
            node,
            "`vi-remap` requires a children block",
        ));
    }
    if node.entries().len() != 1 {
        return Err(node_error(
            text,
            node,
            "`vi-remap` requires exactly one `modes` property",
        ));
    }
    let entry = &node.entries()[0];
    if entry.ty().is_some() || entry.name().map(|name| name.value()) != Some("modes") {
        return Err(node_error(
            text,
            node,
            "`vi-remap` requires exactly one untyped `modes` property",
        ));
    }
    match entry.value().as_string() {
        Some("normal") => Ok(ViModeSelection {
            normal: true,
            visual: false,
        }),
        Some("visual") => Ok(ViModeSelection {
            normal: false,
            visual: true,
        }),
        Some("normal,visual") => Ok(ViModeSelection {
            normal: true,
            visual: true,
        }),
        _ => Err(node_error(
            text,
            node,
            "`vi-remap` modes must be `normal`, `visual`, or `normal,visual`",
        )),
    }
}

fn parse_shortcut_nav_remap(
    text: &str,
    node: &KdlNode,
    remaps: &mut KeyRemaps,
) -> Result<(), String> {
    for child in group_children(text, node, "keybindings.shortcut-nav-remap")? {
        let (from, to, operation) = parse_remap(text, child, "keybindings.shortcut-nav-remap")?;
        if !has_ctrl_or_alt(from) {
            return Err(node_error(
                text,
                child,
                "`shortcut-nav-remap` sources must contain Ctrl or Alt",
            ));
        }
        if operation == KeyRemapOperation::Swap && !has_ctrl_or_alt(to) {
            return Err(node_error(
                text,
                child,
                "both sides of a `shortcut-nav-remap` swap must contain Ctrl or Alt",
            ));
        }
        apply_remap(remaps, KeyRemapScope::ShortcutNav, from, to, operation);
    }

    Ok(())
}

fn parse_remap(
    text: &str,
    node: &KdlNode,
    path: &str,
) -> Result<(KeyBinding, KeyBinding, KeyRemapOperation), String> {
    validate_leaf_node(text, node, path)?;
    let mut operation = KeyRemapOperation::Set;
    let mut operation_seen = false;
    let mut target = None;

    for entry in node.entries() {
        if entry.ty().is_some() {
            return Err(node_error(text, node, "type annotations are not allowed"));
        }
        if let Some(name) = entry.name() {
            if name.value() != "do" {
                return Err(node_error(
                    text,
                    node,
                    format!("unknown remap property `{}`", name.value()),
                ));
            }
            if operation_seen {
                return Err(node_error(text, node, "duplicate `do` property"));
            }
            operation_seen = true;
            operation = match entry.value().as_string() {
                Some("set") => KeyRemapOperation::Set,
                Some("swap") => KeyRemapOperation::Swap,
                _ => return Err(node_error(text, node, "remap `do` must be `set` or `swap`")),
            };
        } else if target.is_some() {
            return Err(node_error(
                text,
                node,
                "a remap requires exactly one target key",
            ));
        } else {
            let value = entry
                .value()
                .as_string()
                .ok_or_else(|| node_error(text, node, "remap target keys must be strings"))?;
            target = Some(parse_key_binding(value).map_err(|err| node_error(text, node, err))?);
        }
    }

    let from = parse_key_binding(node.name().value()).map_err(|err| node_error(text, node, err))?;
    let to = target.ok_or_else(|| node_error(text, node, "a remap requires one target key"))?;
    Ok((from, to, operation))
}

fn apply_remap(
    remaps: &mut KeyRemaps,
    scope: KeyRemapScope,
    from: KeyBinding,
    to: KeyBinding,
    operation: KeyRemapOperation,
) {
    remaps.set(scope, from, to);
    if operation == KeyRemapOperation::Swap {
        remaps.set(scope, to, from);
    }
}

fn has_ctrl_or_alt(binding: KeyBinding) -> bool {
    binding
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn parse_command(
    text: &str,
    node: &KdlNode,
    command: &mut CommandKeybindings,
) -> Result<(), String> {
    for child in group_children(text, node, "keybindings.command")? {
        let path = format!("keybindings.command.{}", child.name().value());
        let (operation, bindings) = parse_binding_update(text, child, &path)?;
        if let Some(action) = CommandAction::from_name(child.name().value()) {
            let target = match action {
                CommandAction::Complete => &mut command.complete,
                CommandAction::Cancel => &mut command.cancel,
            };
            apply_bindings_update(target, operation, bindings);
            continue;
        }

        let action = LineEditorAction::from_name(child.name().value()).ok_or_else(|| {
            node_error(
                text,
                child,
                format!(
                    "unknown `keybindings.command` action `{}`",
                    child.name().value()
                ),
            )
        })?;
        command.editor.push_update(action, operation, bindings);
    }

    Ok(())
}

fn parse_tui(text: &str, node: &KdlNode, tui: &mut TuiKeybindings) -> Result<(), String> {
    for child in group_children(text, node, "keybindings.tui")? {
        let action = TuiAction::from_name(child.name().value()).ok_or_else(|| {
            node_error(
                text,
                child,
                format!(
                    "unknown `keybindings.tui` action `{}`",
                    child.name().value()
                ),
            )
        })?;
        let path = format!("keybindings.tui.{}", child.name().value());
        let (operation, bindings) = parse_binding_update(text, child, &path)?;
        tui.apply_update(action, operation, bindings);
    }

    Ok(())
}

fn parse_binding_update(
    text: &str,
    node: &KdlNode,
    path: &str,
) -> Result<(KeyBindingOperation, Vec<KeyBinding>), String> {
    validate_leaf_node(text, node, path)?;
    let mut operation = KeyBindingOperation::Set;
    let mut operation_seen = false;
    let mut bindings = Vec::new();

    for entry in node.entries() {
        if entry.ty().is_some() {
            return Err(node_error(text, node, "type annotations are not allowed"));
        }
        if let Some(name) = entry.name() {
            if name.value() != "do" {
                return Err(node_error(
                    text,
                    node,
                    format!("unknown `{path}` property `{}`", name.value()),
                ));
            }
            if operation_seen {
                return Err(node_error(text, node, "duplicate `do` property"));
            }
            operation_seen = true;
            operation = match entry.value().as_string() {
                Some("set") => KeyBindingOperation::Set,
                Some("add") => KeyBindingOperation::Add,
                Some("remove") => KeyBindingOperation::Remove,
                _ => {
                    return Err(node_error(
                        text,
                        node,
                        "binding `do` must be `set`, `add`, or `remove`",
                    ));
                }
            };
        } else {
            let value = entry
                .value()
                .as_string()
                .ok_or_else(|| node_error(text, node, format!("`{path}` keys must be strings")))?;
            bindings.push(parse_key_binding(value).map_err(|err| node_error(text, node, err))?);
        }
    }

    Ok((operation, bindings))
}

fn group_children<'a>(text: &str, node: &'a KdlNode, path: &str) -> Result<&'a [KdlNode], String> {
    validate_node_type(text, node, path)?;
    if !node.entries().is_empty() {
        return Err(node_error(
            text,
            node,
            format!("`{path}` does not accept arguments or properties"),
        ));
    }
    node.children()
        .map(KdlDocument::nodes)
        .ok_or_else(|| node_error(text, node, format!("`{path}` requires a children block")))
}

fn validate_leaf_node(text: &str, node: &KdlNode, path: &str) -> Result<(), String> {
    validate_node_type(text, node, path)?;
    if node.children().is_some() {
        return Err(node_error(
            text,
            node,
            format!("`{path}` does not accept a children block"),
        ));
    }
    Ok(())
}

fn validate_node_type(text: &str, node: &KdlNode, path: &str) -> Result<(), String> {
    if node.ty().is_some() {
        Err(node_error(
            text,
            node,
            format!("type annotations are not allowed on `{path}`"),
        ))
    } else {
        Ok(())
    }
}

fn string_arguments<'a>(text: &str, node: &'a KdlNode, path: &str) -> Result<Vec<&'a str>, String> {
    node.entries()
        .iter()
        .map(|entry| {
            if entry.ty().is_some() {
                return Err(node_error(text, node, "type annotations are not allowed"));
            }
            if let Some(name) = entry.name() {
                return Err(node_error(
                    text,
                    node,
                    format!("`{path}` does not accept property `{}`", name.value()),
                ));
            }
            entry
                .value()
                .as_string()
                .ok_or_else(|| node_error(text, node, format!("`{path}` values must be strings")))
        })
        .collect()
}

fn reject_duplicate(text: &str, node: &KdlNode, path: &str, seen: &mut bool) -> Result<(), String> {
    if *seen {
        Err(node_error(text, node, format!("duplicate `{path}` node")))
    } else {
        *seen = true;
        Ok(())
    }
}

fn node_error(text: &str, node: &KdlNode, message: impl fmt::Display) -> String {
    let offset = node.span().offset().min(text.len());
    let line = kdl_line_number(&text[..offset]);
    format!("line {line}: {message}")
}

fn kdl_line_number(text: &str) -> usize {
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
            }
            '\n' | '\u{85}' | '\u{0b}' | '\u{0c}' | '\u{2028}' | '\u{2029}' => line += 1,
            _ => {}
        }
    }
    line
}
