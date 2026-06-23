use std::{env, ffi::OsString, path::PathBuf};

use reedline::{FileBackedHistory, History};

pub(super) const HISTORY_LIMIT: usize = 1000;

const DEFAULT_HISTORY_FILE: &str = "history";

pub(super) fn persistent_history(history_context: Option<&str>) -> Option<Box<dyn History>> {
    let Some(path) = default_history_path(history_context) else {
        eprintln!(
            "warning: persistent history disabled: set XDG_STATE_HOME, HOST_XDG_STATE_HOME, or HOME"
        );
        return None;
    };

    match FileBackedHistory::with_file(HISTORY_LIMIT, path.clone()) {
        Ok(history) => Some(Box::new(history)),
        Err(err) => {
            eprintln!(
                "warning: persistent history disabled: failed to open `{}`: {err}",
                path.display()
            );
            None
        }
    }
}

fn default_history_path(history_context: Option<&str>) -> Option<PathBuf> {
    default_history_path_from_env(history_context, env::var_os)
}

fn default_history_path_from_env(
    history_context: Option<&str>,
    mut env_var: impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    state_home_history_path(env_var("XDG_STATE_HOME"), history_context)
        .or_else(|| state_home_history_path(env_var("HOST_XDG_STATE_HOME"), history_context))
        .or_else(|| home_history_path(env_var("HOME"), history_context))
}

fn state_home_history_path(
    path: Option<OsString>,
    history_context: Option<&str>,
) -> Option<PathBuf> {
    path.filter(|path| !path.is_empty())
        .map(|path| history_path_in_state_dir(PathBuf::from(path), history_context))
}

fn home_history_path(path: Option<OsString>, history_context: Option<&str>) -> Option<PathBuf> {
    path.filter(|path| !path.is_empty()).map(|path| {
        let state_dir = PathBuf::from(path).join(".local").join("state");
        history_path_in_state_dir(state_dir, history_context)
    })
}

fn history_path_in_state_dir(state_dir: PathBuf, history_context: Option<&str>) -> PathBuf {
    state_dir
        .join("dbcrab")
        .join("history")
        .join(history_file_name(history_context))
}

fn history_file_name(history_context: Option<&str>) -> String {
    match history_context {
        Some(context) => format!("{}.history", sanitize_history_context(context)),
        None => DEFAULT_HISTORY_FILE.to_owned(),
    }
}

fn sanitize_history_context(context: &str) -> String {
    let sanitized = context
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "context".to_owned()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{HistoryItem, SearchDirection, SearchQuery};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn history_path_prefers_xdg_state_home() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/xdg-state")),
            "HOST_XDG_STATE_HOME" => Some(OsString::from("/host-state")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_uses_host_state_home_before_home() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::new()),
            "HOST_XDG_STATE_HOME" => Some(OsString::from("/host-state")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/host-state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_falls_back_to_home_local_state() {
        // Given
        let env_var = |name| match name {
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(None, env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/home/alice")
                    .join(".local")
                    .join("state")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_uses_context_file_inside_history_directory() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/xdg-state")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(Some("app"), env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-state")
                    .join("dbcrab")
                    .join("history")
                    .join("app.history")
            )
        );
    }

    #[test]
    fn history_context_is_sanitized_for_filename() {
        // Given
        let context = "billing/prod us";

        // When
        let file_name = history_file_name(Some(context));

        // Then
        assert_eq!(file_name, "billing_prod_us.history");
    }

    #[test]
    fn file_history_restores_multiline_entries() {
        // Given
        let path = temp_history_path("multiline");
        let input = "select *\nfrom users\nwhere id = 1;";
        {
            let mut history = FileBackedHistory::with_file(HISTORY_LIMIT, path.clone())
                .expect("history file should open");
            history
                .save(HistoryItem::from_command_line(input))
                .expect("history item should save");
            history.sync().expect("history should sync");
        }

        // When
        let history = FileBackedHistory::with_file(HISTORY_LIMIT, path.clone())
            .expect("history file should reopen");
        let entries = history
            .search(SearchQuery::everything(SearchDirection::Forward, None))
            .expect("history should search");

        // Then
        assert_eq!(entries[0].command_line, input);

        let _ = std::fs::remove_file(path);
    }

    fn temp_history_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "dbcrab-{name}-{}-{nanos}.history",
            std::process::id()
        ))
    }
}
