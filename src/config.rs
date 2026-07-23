mod keybindings;
mod parser;
mod template;

#[cfg(test)]
mod tests;

pub const CONFIG_SCHEMA_KDL: &str = include_str!("../config.schema.kdl");

pub(crate) use keybindings::{
    CommandKeybindings, ConfigEditMode, HISTORY_MENU, KeyBinding, KeyRemaps, KeybindingsConfig,
    NamedSqlConfig, TuiAction, TuiKeybindings, ViRemapMode, default_emacs_editor_keybindings,
    default_vi_insert_editor_keybindings, default_vi_normal_editor_keybindings,
};
pub(crate) use parser::{ConfigSource, load};
pub(crate) use template::default_config;

#[cfg(test)]
pub(crate) use keybindings::history_menu_event;
