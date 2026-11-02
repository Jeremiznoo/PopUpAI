//! Start with the session, through an XDG autostart entry.
//!
//! The entry is a `.desktop` file in the user's autostart folder
//! (`$XDG_CONFIG_HOME/autostart`, by default `~/.config/autostart`). It runs the
//! application with `--background`, so it waits for the shortcut without showing the
//! popup. Nothing outside that one file is touched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ipc::BUS_NAME;

fn file_name() -> String {
    format!("{BUS_NAME}.desktop")
}

pub struct Autostart {
    dir: PathBuf,
}

/// Quote one argument of an `Exec` line as the Desktop Entry specification asks:
/// double quotes, with `"`, `` ` ``, `$` and `\` escaped by a backslash. A literal
/// `%` is doubled, because `%` introduces a field code.
fn quote_exec(argument: &str) -> String {
    let mut out = String::with_capacity(argument.len() + 2);
    out.push('"');
    for c in argument.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                // In the file the backslash itself is escaped once more: the value
                // goes through string unescaping first, then through argument parsing.
                out.push_str("\\\\");
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=PopUpAI\n\
         Comment=Quick AI question popup (waits for its shortcut)\n\
         Exec={} --background\n\
         Icon=dialog-question\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        quote_exec(&exec.to_string_lossy())
    )
}

impl Autostart {
    /// The current user's autostart folder, `None` if there is no home directory.
    pub fn user() -> Option<Self> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(Self::in_dir(base.join("autostart")))
    }

    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(file_name())
    }

    pub fn is_enabled(&self) -> bool {
        self.path().is_file()
    }

    /// Create or refresh the entry so it starts `exec`. Written to a temporary file
    /// and renamed, so a half-written entry is never seen by the session.
    pub fn enable(&self, exec: &Path) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{}.tmp", file_name()));
        fs::write(&tmp, entry(exec))?;
        fs::rename(&tmp, self.path()).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }

    /// Remove the entry. Not having one is not an error.
    pub fn disable(&self) -> io::Result<()> {
        match fs::remove_file(self.path()) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "popupai-autostart-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            Self(path.join("autostart"))
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = fs::remove_dir_all(parent);
            }
        }
    }

    #[test]
    fn enabling_writes_one_entry_that_starts_the_app_in_the_background() {
        let dir = Dir::new();
        let autostart = Autostart::in_dir(&dir.0);
        assert!(!autostart.is_enabled());

        autostart.enable(Path::new("/usr/bin/popupai")).unwrap();
        assert!(autostart.is_enabled());
        let text = fs::read_to_string(dir.0.join("io.github.jeremiznoo.PopUpAI.desktop")).unwrap();
        assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
        assert!(
            text.contains("Exec=\"/usr/bin/popupai\" --background\n"),
            "{text}"
        );
        let files: Vec<_> = fs::read_dir(&dir.0).unwrap().collect();
        assert_eq!(files.len(), 1, "no temporary file is left behind");
    }

    #[test]
    fn enabling_again_replaces_the_entry() {
        let dir = Dir::new();
        let autostart = Autostart::in_dir(&dir.0);
        autostart.enable(Path::new("/old/popupai")).unwrap();
        autostart.enable(Path::new("/new/popupai")).unwrap();
        let text = fs::read_to_string(dir.0.join("io.github.jeremiznoo.PopUpAI.desktop")).unwrap();
        assert!(
            text.contains("/new/popupai") && !text.contains("/old/popupai"),
            "{text}"
        );
    }

    #[test]
    fn disabling_removes_the_entry_and_is_harmless_when_there_is_none() {
        let dir = Dir::new();
        let autostart = Autostart::in_dir(&dir.0);
        autostart.disable().unwrap();
        autostart.enable(Path::new("/usr/bin/popupai")).unwrap();
        autostart.disable().unwrap();
        assert!(!autostart.is_enabled());
        autostart.disable().unwrap();
    }

    #[test]
    fn a_path_with_spaces_and_special_characters_is_quoted() {
        assert_eq!(quote_exec("/opt/my app/popupai"), "\"/opt/my app/popupai\"");
        assert_eq!(quote_exec("/a\"b"), "\"/a\\\\\"b\"");
        assert_eq!(quote_exec("/a$b"), "\"/a\\\\$b\"");
        assert_eq!(quote_exec("/100%/popupai"), "\"/100%%/popupai\"");
    }

    #[test]
    fn it_only_ever_touches_its_own_file() {
        let dir = Dir::new();
        fs::create_dir_all(&dir.0).unwrap();
        let other = dir.0.join("something-else.desktop");
        fs::write(&other, "keep me").unwrap();
        let autostart = Autostart::in_dir(&dir.0);
        autostart.enable(Path::new("/usr/bin/popupai")).unwrap();
        autostart.disable().unwrap();
        assert_eq!(fs::read_to_string(&other).unwrap(), "keep me");
    }
}
