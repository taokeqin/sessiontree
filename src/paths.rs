//! Where the app keeps its own files on each platform.
use std::path::PathBuf;

/// The app's data directory (`Application Support` on macOS, `%LOCALAPPDATA%`
/// on Windows, `$XDG_DATA_HOME` on Linux).
pub(crate) fn app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = home_dir().map(|home| home.join("Library/Application Support/Sessiontree"));
    #[cfg(windows)]
    let dir = env_path("LOCALAPPDATA")
        .or_else(|| env_path("APPDATA"))
        .map(|dir| dir.join("Sessiontree"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = env_path("XDG_DATA_HOME")
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
        .map(|dir| dir.join("sessiontree"));
    dir
}

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env_path("USERPROFILE").or_else(|| env_path("HOME"))
    }
    #[cfg(not(windows))]
    {
        env_path("HOME")
    }
}
