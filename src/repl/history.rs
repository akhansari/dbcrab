use reedline::{FileBackedHistory, History};

use crate::paths;

pub(super) const HISTORY_LIMIT: usize = 1000;

pub(super) fn persistent_history(history_context: Option<&str>) -> Option<Box<dyn History>> {
    let Some(path) = paths::default_history_path(history_context) else {
        eprintln!(
            "warning: persistent history disabled: set XDG_STATE_HOME, HOST_XDG_STATE_HOME, LOCALAPPDATA, USERPROFILE, or HOME"
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

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{HistoryItem, SearchDirection, SearchQuery};
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

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
