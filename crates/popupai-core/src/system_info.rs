//! A short description of the user's system, given to the model so that its answers fit
//! (commands for the right distribution, the right shell, today's date).
//!
//! Only coarse facts that a user would say out loud: the distribution and version, the
//! kernel, the architecture, the desktop and session type, the shell, the language and the
//! date. Never the user name, the host name, the home folder, the network, hardware
//! identifiers or anything from the file system. It is sent to the provider with each request,
//! and can be turned off with `advanced.share_system_info = false`.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// `PRETTY_NAME` from the text of an `os-release` file.
pub fn pretty_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.strip_prefix("PRETTY_NAME=")?;
        let value = value.trim().trim_matches('"').trim_matches('\'');
        (!value.is_empty() && value.len() <= 100 && !value.chars().any(char::is_control))
            .then(|| value.to_owned())
    })
}

/// The civil date (UTC) for a number of seconds since the Unix epoch.
pub fn date_from_unix(seconds: u64) -> (u64, u64, u64) {
    // Howard Hinnant's days-to-civil algorithm.
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        u64::try_from(year).unwrap_or(0),
        u64::try_from(month).unwrap_or(0),
        u64::try_from(day).unwrap_or(0),
    )
}

/// A value from the environment, kept only if it is short, printable text.
fn env_text(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let value = value.trim();
    (!value.is_empty() && value.len() <= 60 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn shell_name() -> Option<String> {
    let shell = env_text("SHELL")?;
    shell.rsplit('/').next().map(str::to_owned)
}

/// The description sent to the model. The facts that do not change while the application
/// runs are read once; the date is read each time.
pub fn describe(can_run_commands: bool) -> String {
    static FIXED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let fixed = FIXED.get_or_init(|| {
        let mut lines: Vec<String> = Vec::new();
        let os = fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|text| pretty_name(&text))
            .unwrap_or_else(|| std::env::consts::OS.to_owned());
        lines.push(format!("- Operating system: {os}"));
        if let Some(kernel) = fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty() && k.len() <= 60)
        {
            lines.push(format!("- Kernel: Linux {kernel}"));
        }
        lines.push(format!("- Architecture: {}", std::env::consts::ARCH));
        let desktop = env_text("XDG_CURRENT_DESKTOP");
        let session = env_text("XDG_SESSION_TYPE");
        match (desktop, session) {
            (Some(d), Some(s)) => lines.push(format!("- Desktop: {d} ({s} session)")),
            (Some(d), None) => lines.push(format!("- Desktop: {d}")),
            (None, Some(s)) => lines.push(format!("- Session type: {s}")),
            (None, None) => {}
        }
        if let Some(shell) = shell_name() {
            lines.push(format!("- Shell: {shell}"));
        }
        if let Some(language) = env_text("LANG") {
            lines.push(format!("- Language: {language}"));
        }
        lines.join("\n")
    });
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day) = date_from_unix(seconds);
    let abilities = if can_run_commands {
        "You can run shell commands on it with the run_command tool, for example to find a file or \
         check a setting: use it when it helps instead of only explaining. Simple read-only \
         commands run at once; any other command is shown to the user, who may refuse it. Write \
         commands for this system, never destructive ones unless the user asked."
    } else {
        "You cannot run commands or read files on it here: when a command would help, give one the \
         user can run, written for this system."
    };
    format!("The user's computer:\n{fixed}\n- Date: {year:04}-{month:02}-{day:02}\n{abilities}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pretty_name_is_read_from_os_release() {
        let text = "NAME=\"Fedora Linux\"\nVERSION_ID=44\nPRETTY_NAME=\"Fedora Linux 44 (KDE Plasma Desktop Edition)\"\n";
        assert_eq!(
            pretty_name(text).as_deref(),
            Some("Fedora Linux 44 (KDE Plasma Desktop Edition)")
        );
        assert_eq!(pretty_name("NAME=x\n"), None);
        assert_eq!(pretty_name("PRETTY_NAME=\n"), None);
        assert_eq!(
            pretty_name(&format!("PRETTY_NAME={}", "x".repeat(200))),
            None
        );
    }

    #[test]
    fn dates_are_computed_from_the_epoch() {
        assert_eq!(date_from_unix(0), (1970, 1, 1));
        assert_eq!(date_from_unix(951_782_400), (2000, 2, 29), "a leap day");
        assert_eq!(date_from_unix(1_793_664_000), (2026, 11, 3));
        assert_eq!(date_from_unix(1_709_164_800), (2024, 2, 29));
    }

    #[test]
    fn the_description_has_the_system_and_leaves_out_identity() {
        let text = describe(false);
        assert!(text.contains("Operating system:"), "{text}");
        assert!(text.contains("Architecture:"), "{text}");
        assert!(text.contains("Date: 20"), "{text}");
        assert!(text.contains("cannot run commands"), "{text}");
        assert!(
            describe(true).contains("run_command"),
            "tools are mentioned only when available"
        );
        assert!(!text.contains("run_command"), "{text}");
        for private in [
            std::env::var("USER").unwrap_or_default(),
            std::env::var("HOME").unwrap_or_default(),
            std::env::var("HOSTNAME").unwrap_or_default(),
        ] {
            if private.len() > 3 {
                assert!(
                    !text.contains(&private),
                    "the description must not contain {private}"
                );
            }
        }
    }
}
