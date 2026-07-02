use std::{env, ffi::OsString, path::PathBuf};

const DEFAULT_HISTORY_FILE: &str = "history";

pub(crate) fn default_config_path() -> Option<PathBuf> {
    default_config_path_from_env(env::var_os, default_path_platform())
}

pub(crate) fn default_history_path(history_context: Option<&str>) -> Option<PathBuf> {
    default_history_path_from_env(history_context, env::var_os)
}

fn default_config_path_from_env(
    mut env_var: impl FnMut(&'static str) -> Option<OsString>,
    platform: UserPathPlatform,
) -> Option<PathBuf> {
    xdg_config_path(env_var("XDG_CONFIG_HOME")).or_else(|| match platform {
        UserPathPlatform::Windows => {
            windows_config_path(env_var("APPDATA"), env_var("USERPROFILE"))
                .or_else(|| home_config_path(env_var("HOME")))
        }
        UserPathPlatform::Unix => home_config_path(env_var("HOME")),
    })
}

fn default_history_path_from_env(
    history_context: Option<&str>,
    env_var: impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    default_history_path_from_env_for_platform(history_context, env_var, default_path_platform())
}

fn default_history_path_from_env_for_platform(
    history_context: Option<&str>,
    mut env_var: impl FnMut(&'static str) -> Option<OsString>,
    platform: UserPathPlatform,
) -> Option<PathBuf> {
    state_home_history_path(env_var("XDG_STATE_HOME"), history_context)
        .or_else(|| state_home_history_path(env_var("HOST_XDG_STATE_HOME"), history_context))
        .or_else(|| match platform {
            UserPathPlatform::Windows => windows_history_path(
                env_var("LOCALAPPDATA"),
                env_var("USERPROFILE"),
                history_context,
            )
            .or_else(|| home_history_path(env_var("HOME"), history_context)),
            UserPathPlatform::Unix => home_history_path(env_var("HOME"), history_context),
        })
}

#[derive(Clone, Copy)]
enum UserPathPlatform {
    Unix,
    Windows,
}

fn default_path_platform() -> UserPathPlatform {
    if cfg!(windows) {
        UserPathPlatform::Windows
    } else {
        UserPathPlatform::Unix
    }
}

fn xdg_config_path(path: Option<OsString>) -> Option<PathBuf> {
    env_path(path).map(config_path_in_dir)
}

fn windows_config_path(
    appdata: Option<OsString>,
    user_profile: Option<OsString>,
) -> Option<PathBuf> {
    env_path(appdata)
        .or_else(|| env_path(user_profile).map(|path| path.join("AppData").join("Roaming")))
        .map(config_path_in_dir)
}

fn home_config_path(path: Option<OsString>) -> Option<PathBuf> {
    env_path(path).map(|path| config_path_in_dir(path.join(".config")))
}

fn config_path_in_dir(config_dir: PathBuf) -> PathBuf {
    config_dir.join("dbcrab").join("config.toml")
}

fn state_home_history_path(
    path: Option<OsString>,
    history_context: Option<&str>,
) -> Option<PathBuf> {
    env_path(path).map(|path| history_path_in_state_dir(path, history_context))
}

fn home_history_path(path: Option<OsString>, history_context: Option<&str>) -> Option<PathBuf> {
    env_path(path).map(|path| {
        let state_dir = path.join(".local").join("state");
        history_path_in_state_dir(state_dir, history_context)
    })
}

fn windows_history_path(
    local_appdata: Option<OsString>,
    user_profile: Option<OsString>,
    history_context: Option<&str>,
) -> Option<PathBuf> {
    env_path(local_appdata)
        .or_else(|| env_path(user_profile).map(|path| path.join("AppData").join("Local")))
        .map(|path| history_path_in_state_dir(path, history_context))
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

fn env_path(path: Option<OsString>) -> Option<PathBuf> {
    path.filter(|path| !path.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_prefers_xdg_config_home() {
        // Given
        let env_var = |name| match name {
            "XDG_CONFIG_HOME" => Some(OsString::from("/xdg-config")),
            "APPDATA" => Some(OsString::from("C:\\Users\\Alice\\AppData\\Roaming")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_config_path_from_env(env_var, UserPathPlatform::Windows);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-config")
                    .join("dbcrab")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn config_path_uses_windows_appdata() {
        // Given
        let env_var = |name| match name {
            "APPDATA" => Some(OsString::from("C:\\Users\\Alice\\AppData\\Roaming")),
            "USERPROFILE" => Some(OsString::from("C:\\Users\\Alice")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_config_path_from_env(env_var, UserPathPlatform::Windows);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("C:\\Users\\Alice\\AppData\\Roaming")
                    .join("dbcrab")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn config_path_uses_windows_user_profile_without_appdata() {
        // Given
        let env_var = |name| match name {
            "USERPROFILE" => Some(OsString::from("C:\\Users\\Alice")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_config_path_from_env(env_var, UserPathPlatform::Windows);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("C:\\Users\\Alice")
                    .join("AppData")
                    .join("Roaming")
                    .join("dbcrab")
                    .join("config.toml")
            )
        );
    }

    #[test]
    fn config_path_falls_back_to_home_config_on_unix() {
        // Given
        let env_var = |name| match name {
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path = default_config_path_from_env(env_var, UserPathPlatform::Unix);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/home/alice")
                    .join(".config")
                    .join("dbcrab")
                    .join("config.toml")
            )
        );
    }

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
    fn history_path_uses_windows_local_appdata() {
        // Given
        let env_var = |name| match name {
            "LOCALAPPDATA" => Some(OsString::from("C:\\Users\\Alice\\AppData\\Local")),
            "USERPROFILE" => Some(OsString::from("C:\\Users\\Alice")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path =
            default_history_path_from_env_for_platform(None, env_var, UserPathPlatform::Windows);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("C:\\Users\\Alice\\AppData\\Local")
                    .join("dbcrab")
                    .join("history")
                    .join("history")
            )
        );
    }

    #[test]
    fn history_path_uses_windows_user_profile_without_local_appdata() {
        // Given
        let env_var = |name| match name {
            "USERPROFILE" => Some(OsString::from("C:\\Users\\Alice")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        };

        // When
        let path =
            default_history_path_from_env_for_platform(None, env_var, UserPathPlatform::Windows);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("C:\\Users\\Alice")
                    .join("AppData")
                    .join("Local")
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
    fn history_path_sanitizes_context_file_name() {
        // Given
        let env_var = |name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/xdg-state")),
            _ => None,
        };

        // When
        let path = default_history_path_from_env(Some("billing/prod us"), env_var);

        // Then
        assert_eq!(
            path,
            Some(
                PathBuf::from("/xdg-state")
                    .join("dbcrab")
                    .join("history")
                    .join("billing_prod_us.history")
            )
        );
    }
}
