use kdl::KdlDocument;
use reedline::Keybindings as ReedlineKeybindings;

use super::keybindings::{
    AppConfig, CommandAction, KeyBinding, KeyBindingOperation, LineEditorAction, PromptAction,
    TuiAction, default_emacs_editor_keybindings, default_vi_insert_editor_keybindings,
    default_vi_normal_editor_keybindings,
};

type EditorActionBindings = Vec<(LineEditorAction, Vec<KeyBinding>)>;

struct EditorDefaultBindingGroups {
    shared: EditorActionBindings,
    emacs: EditorActionBindings,
    vi_insert: EditorActionBindings,
    vi_normal: EditorActionBindings,
}

fn editor_default_binding_groups() -> EditorDefaultBindingGroups {
    let emacs = default_emacs_editor_keybindings();
    let vi_insert = default_vi_insert_editor_keybindings();
    let vi_normal = default_vi_normal_editor_keybindings();
    let mut groups = EditorDefaultBindingGroups {
        shared: Vec::new(),
        emacs: Vec::new(),
        vi_insert: Vec::new(),
        vi_normal: Vec::new(),
    };

    for &action in LineEditorAction::ALL {
        let emacs_bindings = reedline_bindings_for_action(&emacs, action);
        let vi_insert_bindings = reedline_bindings_for_action(&vi_insert, action);
        let shared = emacs_bindings
            .iter()
            .filter(|binding| vi_insert_bindings.contains(binding))
            .copied()
            .collect::<Vec<_>>();
        let emacs_only = emacs_bindings
            .iter()
            .filter(|binding| !shared.contains(binding))
            .copied()
            .collect::<Vec<_>>();
        let vi_insert_only = vi_insert_bindings
            .iter()
            .filter(|binding| !shared.contains(binding))
            .copied()
            .collect::<Vec<_>>();

        push_nonempty_action_bindings(&mut groups.shared, action, shared);
        push_nonempty_action_bindings(&mut groups.emacs, action, emacs_only);
        push_nonempty_action_bindings(&mut groups.vi_insert, action, vi_insert_only);
        push_nonempty_action_bindings(
            &mut groups.vi_normal,
            action,
            reedline_bindings_for_action(&vi_normal, action),
        );
    }

    groups
}

fn reedline_bindings_for_action(
    keybindings: &ReedlineKeybindings,
    action: LineEditorAction,
) -> Vec<KeyBinding> {
    let event = action.event();
    let mut bindings = keybindings
        .get_keybindings()
        .iter()
        .filter_map(|(binding, bound_event)| {
            (bound_event == &event).then_some(KeyBinding {
                code: binding.key_code,
                modifiers: binding.modifier,
            })
        })
        .collect::<Vec<_>>();
    bindings.sort_by_key(ToString::to_string);
    bindings
}

fn push_nonempty_action_bindings(
    group: &mut EditorActionBindings,
    action: LineEditorAction,
    bindings: Vec<KeyBinding>,
) {
    if !bindings.is_empty() {
        group.push((action, bindings));
    }
}

fn push_editor_default_group(
    output: &mut String,
    heading: &str,
    operation: KeyBindingOperation,
    group: &EditorActionBindings,
) {
    if group.is_empty() {
        return;
    }

    output.push_str(heading);
    for (action, bindings) in group {
        push_binding_line(
            output,
            "            // ",
            action.name(),
            operation,
            bindings.iter().map(config_key_name),
        );
    }
}

pub fn default_config() -> String {
    let config = AppConfig::default();
    let mut output = format!(
        "/- kdl-version 2\n\n// emacs | vi\nedit-mode {}\n\nkeybindings {{\n    editor {{\n        emacs-vi-insert {{\n",
        config.edit_mode.name()
    );
    let editor_defaults = editor_default_binding_groups();
    push_editor_default_group(
        &mut output,
        "            // Shared Emacs and Vi-insert defaults:\n",
        KeyBindingOperation::Set,
        &editor_defaults.shared,
    );
    push_editor_default_group(
        &mut output,
        "            //\n            // Additional Emacs defaults:\n",
        KeyBindingOperation::Add,
        &editor_defaults.emacs,
    );
    push_editor_default_group(
        &mut output,
        "            //\n            // Additional Vi-insert defaults:\n",
        KeyBindingOperation::Add,
        &editor_defaults.vi_insert,
    );

    output.push_str("        }\n\n        vi-normal {\n");
    push_editor_default_group(
        &mut output,
        "            // Grammar commands are not listed:\n",
        KeyBindingOperation::Set,
        &editor_defaults.vi_normal,
    );
    output.push_str("        }\n    }\n\n    prompt {\n");
    for &action in PromptAction::ALL {
        push_runtime_binding_line(
            &mut output,
            "        ",
            action.name(),
            config.keybindings.prompt.bindings_for(action),
        );
    }

    output.push_str(
        "    }\n\n    vi-remap modes=normal,visual {\n        // h do=swap i\n    }\n\n    shortcut-nav-remap {\n        // ctrl-h do=swap ctrl-i\n    }\n\n    command {\n",
    );
    for &action in CommandAction::ALL {
        push_runtime_binding_line(
            &mut output,
            "        ",
            action.name(),
            config.keybindings.command.bindings_for(action),
        );
    }

    output.push_str("    }\n\n    tui {\n");
    for &action in TuiAction::ALL {
        push_runtime_binding_line(
            &mut output,
            "        ",
            action.name(),
            config.keybindings.tui.bindings_for(action),
        );
        if action.ends_template_group() {
            output.push('\n');
        }
    }
    output.push_str("    }\n}\n");
    output
}

fn push_runtime_binding_line(
    output: &mut String,
    indentation: &str,
    action: &str,
    bindings: &[KeyBinding],
) {
    push_binding_line(
        output,
        indentation,
        action,
        KeyBindingOperation::Set,
        bindings.iter().map(config_key_name),
    );
}

fn push_binding_line<I, S>(
    output: &mut String,
    indentation: &str,
    action: &str,
    operation: KeyBindingOperation,
    bindings: I,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    output.push_str(indentation);
    output.push_str(action);
    match operation {
        KeyBindingOperation::Set => {}
        KeyBindingOperation::Add => output.push_str(" do=add"),
        KeyBindingOperation::Remove => output.push_str(" do=remove"),
    }
    for binding in bindings {
        output.push(' ');
        output.push_str(binding.as_ref());
    }
    output.push('\n');
}

fn config_key_name(binding: &KeyBinding) -> String {
    config_key_token(&binding.to_string())
}

fn config_key_token(name: &str) -> String {
    let candidate = format!("binding {name}");
    if name != ":"
        && candidate.parse::<KdlDocument>().is_ok_and(|document| {
            document.nodes().first().is_some_and(|node| {
                node.entries().len() == 1
                    && node.entries()[0].name().is_none()
                    && node.entries()[0].value().as_string() == Some(name)
            })
        })
    {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}
