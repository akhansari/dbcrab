use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kdl::{KdlDocument, KdlNode};
use reedline::ReedlineEvent;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    CONFIG_SCHEMA_KDL,
    keybindings::{
        AppConfig, CommandAction, ConfigEditMode, LineEditorAction, PromptAction, TuiAction,
        TuiKeybindings, ViRemapMode, parse_key_binding,
    },
    parser::{ConfigSource, load, parse_config},
    template::default_config,
};

#[test]
fn loaded_config_retains_explicit_source_path() {
    // Given
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "dbcrab-config-source-{}-{unique}.kdl",
        std::process::id()
    ));
    fs::write(&path, "edit-mode \"vi\"").expect("config should be written");

    // When
    let loaded = load(Some(path.clone())).expect("config should load");

    // Then
    assert_eq!(loaded.source, ConfigSource::File(path.clone()));
    assert_eq!(loaded.settings.edit_mode, ConfigEditMode::Vi);
    let _ = fs::remove_file(path);
}

#[test]
fn config_schema_is_valid_kdl() {
    // Given
    let schema = CONFIG_SCHEMA_KDL;

    // When
    let result = schema.parse::<KdlDocument>();

    // Then
    assert!(result.is_ok());
}

#[test]
fn default_config_kdl_matches_runtime_defaults() {
    // Given
    let defaults = default_config();

    // When
    let config = parse_config(&defaults).expect("default config should parse");

    // Then
    assert_eq!(config, AppConfig::default());
}

#[test]
fn named_sql_shared_path_is_parsed() {
    // Given
    let text = "named-sql { shared-path \"/srv/dbcrab/shared\" }";

    // When
    let config = parse_config(text).expect("named SQL config should parse");

    // Then
    assert_eq!(
        config.named_sql.shared_path,
        Some(PathBuf::from("/srv/dbcrab/shared"))
    );
}

#[test]
fn unknown_named_sql_setting_is_rejected() {
    // Given
    let text = "named-sql { unknown \"value\" }";

    // When
    let result = parse_config(text);

    // Then
    assert_eq!(
        result.expect_err("unknown setting should fail"),
        "line 1: unknown `named-sql` node `unknown`"
    );
}

#[test]
fn default_config_documents_reedline_editor_defaults() {
    // Given
    let defaults = default_config();

    // When
    let documents_shared_default = defaults.contains("// clear-screen ctrl-l");
    let documents_emacs_default = defaults.contains("// redo do=add ctrl-g");
    let documents_vi_normal_default = defaults.contains("// move-left backspace");

    // Then
    assert!(documents_shared_default);
    assert!(documents_emacs_default);
    assert!(documents_vi_normal_default);
}

#[test]
fn config_schema_declares_all_prompt_actions() {
    // Given
    let schema = CONFIG_SCHEMA_KDL
        .parse::<KdlDocument>()
        .expect("config schema should parse");
    let expected = PromptAction::ALL
        .iter()
        .map(|action| action.name().to_owned())
        .collect::<Vec<_>>();

    // When
    let actions = inline_schema_action_names(&schema, "prompt");

    // Then
    assert_eq!(actions, expected);
}

#[test]
fn config_schema_declares_all_command_actions() {
    // Given
    let schema = CONFIG_SCHEMA_KDL
        .parse::<KdlDocument>()
        .expect("config schema should parse");
    let expected = CommandAction::ALL
        .iter()
        .map(|action| action.name().to_owned())
        .chain(
            LineEditorAction::ALL
                .iter()
                .map(|action| action.name().to_owned()),
        )
        .collect::<Vec<_>>();

    // When
    let actions = definition_action_names(&schema, "command-actions");

    // Then
    assert_eq!(actions, expected);
}

#[test]
fn config_schema_declares_all_tui_actions() {
    // Given
    let schema = CONFIG_SCHEMA_KDL
        .parse::<KdlDocument>()
        .expect("config schema should parse");
    let expected = TuiAction::ALL
        .iter()
        .map(|action| action.name().to_owned())
        .collect::<Vec<_>>();

    // When
    let actions = inline_schema_action_names(&schema, "tui");

    // Then
    assert_eq!(actions, expected);
}

#[test]
fn config_schema_declares_all_line_editor_actions() {
    // Given
    let schema = CONFIG_SCHEMA_KDL
        .parse::<KdlDocument>()
        .expect("config schema should parse");
    let expected = LineEditorAction::ALL
        .iter()
        .map(|action| action.name().to_owned())
        .collect::<Vec<_>>();

    // When
    let actions = definition_action_names(&schema, "line-editor-actions");

    // Then
    assert_eq!(actions, expected);
}

#[test]
fn config_schema_groups_reference_their_action_definitions() {
    // Given
    let schema = CONFIG_SCHEMA_KDL
        .parse::<KdlDocument>()
        .expect("config schema should parse");
    let document = schema_document(&schema);
    let keybindings = named_schema_node(document, "keybindings");
    let keybinding_children = schema_node_children(keybindings);
    let editor = named_schema_node(keybinding_children, "editor");
    let editor_children = schema_node_children(editor);

    // When
    let references = (
        schema_node_children_reference(named_schema_node(editor_children, "emacs-vi-insert")),
        schema_node_children_reference(named_schema_node(editor_children, "vi-normal")),
        schema_node_children_reference(named_schema_node(keybinding_children, "command")),
    );

    // Then
    assert_eq!(
        references,
        (
            Some("[id=\"line-editor-actions\"]"),
            Some("[id=\"line-editor-actions\"]"),
            Some("[id=\"command-actions\"]"),
        )
    );
}

fn inline_schema_action_names(schema: &KdlDocument, group: &str) -> Vec<String> {
    let document = schema_document(schema);
    let keybindings = named_schema_node(document, "keybindings");
    let keybinding_children = schema_node_children(keybindings);
    let group = named_schema_node(keybinding_children, group);
    schema_node_names(schema_node_children(group))
}

fn definition_action_names(schema: &KdlDocument, id: &str) -> Vec<String> {
    let definitions = schema_document(schema)
        .nodes()
        .iter()
        .find(|node| node.name().value() == "definitions")
        .and_then(KdlNode::children)
        .expect("config schema should have definitions");
    let definition = definitions
        .nodes()
        .iter()
        .find(|node| {
            node.name().value() == "children"
                && node.entries().iter().any(|entry| {
                    entry.name().map(|name| name.value()) == Some("id")
                        && entry.value().as_string() == Some(id)
                })
        })
        .and_then(KdlNode::children)
        .expect("config schema should have the requested definition");
    schema_node_names(definition)
}

fn schema_document(schema: &KdlDocument) -> &KdlDocument {
    schema
        .nodes()
        .iter()
        .find(|node| node.name().value() == "document")
        .and_then(KdlNode::children)
        .expect("config schema should have a document node")
}

fn named_schema_node<'a>(document: &'a KdlDocument, name: &str) -> &'a KdlNode {
    document
        .nodes()
        .iter()
        .find(|node| {
            node.name().value() == "node"
                && node
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .and_then(|entry| entry.value().as_string())
                    == Some(name)
        })
        .expect("config schema should have the requested node")
}

fn schema_node_children(node: &KdlNode) -> &KdlDocument {
    schema_node_children_rule(node)
        .and_then(KdlNode::children)
        .expect("config schema node should have children rules")
}

fn schema_node_children_reference(node: &KdlNode) -> Option<&str> {
    schema_node_children_rule(node).and_then(|children| {
        children.entries().iter().find_map(|entry| {
            (entry.name().map(|name| name.value()) == Some("ref"))
                .then(|| entry.value().as_string())
                .flatten()
        })
    })
}

fn schema_node_children_rule(node: &KdlNode) -> Option<&KdlNode> {
    node.children().and_then(|document| {
        document
            .nodes()
            .iter()
            .find(|child| child.name().value() == "children")
    })
}

fn schema_node_names(document: &KdlDocument) -> Vec<String> {
    document
        .nodes()
        .iter()
        .filter(|node| node.name().value() == "node")
        .filter_map(|node| {
            node.entries()
                .iter()
                .find(|entry| entry.name().is_none())
                .and_then(|entry| entry.value().as_string())
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn config_parses_vi_edit_mode() {
    // Given
    let text = "edit-mode vi\n";

    // When
    let config = parse_config(text).expect("config should parse");

    // Then
    assert_eq!(config.edit_mode, ConfigEditMode::Vi);
}

#[test]
fn config_rejects_unknown_top_level_nodes() {
    // Given
    let text = "edit_mode vi\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn config_errors_count_kdl_carriage_return_newlines() {
    // Given
    let text = "edit-mode vi\runknown {}\r";

    // When
    let error = parse_config(text).expect_err("unknown node should fail");

    // Then
    assert!(error.starts_with("line 2:"));
}

#[test]
fn config_rejects_duplicate_singleton_groups() {
    // Given
    let text = "keybindings { prompt {}; prompt {} }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn vi_remap_swap_applies_in_selected_modes() {
    // Given
    let text = "keybindings { vi-remap modes=normal,visual { h do=swap i } }\n";
    let h = Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    let i = Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    // When
    let config = parse_config(text).expect("config should parse");
    let normal = config
        .keybindings
        .remaps
        .remap_editor_event(Some(ViRemapMode::Normal), h);
    let visual = config
        .keybindings
        .remaps
        .remap_editor_event(Some(ViRemapMode::Visual), i);

    // Then
    assert_eq!(
        normal,
        Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
    );
    assert_eq!(
        visual,
        Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
    );
}

#[test]
fn vi_remap_does_not_apply_outside_selected_modes() {
    // Given
    let text = "keybindings { vi-remap modes=normal { h i } }\n";
    let h = Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));

    // When
    let config = parse_config(text).expect("config should parse");
    let remapped = config
        .keybindings
        .remaps
        .remap_editor_event(None, h.clone());

    // Then
    assert_eq!(remapped, h);
}

#[test]
fn vi_remap_rejects_overlapping_mode_blocks() {
    // Given
    let text = "keybindings { vi-remap modes=normal,visual {}; vi-remap modes=normal {} }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn vi_remap_rejects_modified_sources() {
    // Given
    let text = "keybindings { vi-remap modes=normal { ctrl-h ctrl-i } }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn shortcut_nav_remap_applies_in_all_editor_modes() {
    // Given
    let text = "keybindings { shortcut-nav-remap { ctrl-h do=swap ctrl-i } }\n";
    let ctrl_i = Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));

    // When
    let config = parse_config(text).expect("config should parse");
    let remapped = config.keybindings.remaps.remap_editor_event(None, ctrl_i);

    // Then
    assert_eq!(
        remapped,
        Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL))
    );
}

#[test]
fn shortcut_nav_remap_rejects_unmodified_sources() {
    // Given
    let text = "keybindings { shortcut-nav-remap { h i } }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn shortcut_nav_swap_rejects_unmodified_targets() {
    // Given
    let text = "keybindings { shortcut-nav-remap { ctrl-h do=swap i } }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn config_overrides_default_binding_for_action() {
    // Given
    let text = "keybindings { tui { left a } }\n";
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
    let text = "keybindings { tui { left a } }\n";
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
    let text = "keybindings { tui { left x; right x } }\n";

    // When
    let result = parse_config(text);

    // Then
    assert!(result.is_err());
}

#[test]
fn prompt_keybindings_can_patch_defaults() {
    // Given
    let text = "keybindings { prompt { complete do=remove ctrl-space; complete do=add ctrl-x } }\n";
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
    let text = "keybindings { editor { emacs-vi-insert { clear-screen ctrl-x } } }\n";
    let mut keybindings = reedline::default_emacs_keybindings();

    // When
    let config = parse_config(text).expect("config should parse");
    config
        .keybindings
        .editor
        .emacs_vi_insert
        .apply_to(&mut keybindings);

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
fn line_editor_remove_keeps_keys_bound_to_other_actions() {
    // Given
    let text = "keybindings { editor { emacs-vi-insert { clear-screen do=remove ctrl-c } } }\n";
    let mut keybindings = reedline::default_emacs_keybindings();

    // When
    let config = parse_config(text).expect("config should parse");
    config
        .keybindings
        .editor
        .emacs_vi_insert
        .apply_to(&mut keybindings);

    // Then
    assert_eq!(
        keybindings.find_binding(KeyModifiers::CONTROL, KeyCode::Char('c')),
        Some(ReedlineEvent::CtrlC)
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
    let y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);

    // When
    let enter_action = keybindings.action_for(enter);
    let tab_action = keybindings.action_for(tab);
    let edit_action = keybindings.action_for(c);
    let stage_action = keybindings.action_for(ctrl_s);
    let null_action = keybindings.action_for(ctrl_x);
    let update_action = keybindings.action_for(ctrl_u);
    let yank_action = keybindings.action_for(y);

    // Then
    assert_eq!(enter_action, Some(TuiAction::TogglePreview));
    assert_eq!(tab_action, Some(TuiAction::FocusNext));
    assert_eq!(edit_action, Some(TuiAction::EditCell));
    assert_eq!(stage_action, Some(TuiAction::StageChange));
    assert_eq!(null_action, Some(TuiAction::StageNull));
    assert_eq!(update_action, Some(TuiAction::UpdateRow));
    assert_eq!(yank_action, Some(TuiAction::YankCell));
}
