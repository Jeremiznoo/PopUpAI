//! Where the configuration file lives.

use std::ffi::OsString;
use std::path::PathBuf;

/// `POPUPAI_CONFIG` overrides everything (for development and tests). Otherwise the
/// file is `$XDG_CONFIG_HOME/popupai/config.toml`, falling back to
/// `~/.config/popupai/config.toml`. A relative `XDG_CONFIG_HOME` is ignored, as the
/// XDG specification requires.
pub fn config_path_from(
    explicit: Option<OsString>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> PathBuf {
    if let Some(path) = explicit.filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let base = xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("popupai").join("config.toml")
}

pub fn config_path() -> PathBuf {
    config_path_from(
        std::env::var_os("POPUPAI_CONFIG"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    #[test]
    fn an_explicit_path_wins() {
        let path = config_path_from(os("/tmp/test.toml"), os("/xdg"), os("/home/u"));
        assert_eq!(path, Path::new("/tmp/test.toml"));
    }

    #[test]
    fn xdg_config_home_is_used_when_absolute() {
        let path = config_path_from(None, os("/xdg"), os("/home/u"));
        assert_eq!(path, Path::new("/xdg/popupai/config.toml"));
    }

    #[test]
    fn a_relative_or_empty_xdg_value_falls_back_to_the_home_directory() {
        for xdg in [os("relative/dir"), os(""), None] {
            let path = config_path_from(None, xdg, os("/home/u"));
            assert_eq!(path, Path::new("/home/u/.config/popupai/config.toml"));
        }
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let path = config_path_from(os(""), None, os("/home/u"));
        assert_eq!(path, Path::new("/home/u/.config/popupai/config.toml"));
    }
}
