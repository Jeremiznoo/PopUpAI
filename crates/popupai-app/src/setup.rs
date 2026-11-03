//! `popupai --setup-shortcut`: bind the shortcut in the desktop itself, after asking.

use std::io::{BufRead, Write};
use std::process::{Command, ExitCode};

use popupai_core::config::Config;
use popupai_platform::shortcut_setup::{Desktop, detect, parse_gvariant_list, plan};

use crate::config_path;

const GNOME_LIST: [&str; 3] = [
    "get",
    "org.gnome.settings-daemon.plugins.media-keys",
    "custom-keybindings",
];

pub fn run() -> ExitCode {
    let config = Config::load(&config_path::config_path()).unwrap_or_default();
    let chord = match config.shortcut() {
        Ok(chord) => chord,
        Err(err) => {
            eprintln!("popupai: the shortcut in the configuration is not usable: {err}");
            return ExitCode::FAILURE;
        }
    };
    let exe = std::env::current_exe().map_or_else(
        |_| "popupai".to_owned(),
        |path| path.to_string_lossy().into_owned(),
    );
    let desktop = detect(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default());
    let existing = if desktop == Desktop::Gnome {
        Command::new("gsettings")
            .args(GNOME_LIST)
            .output()
            .ok()
            .map(|out| parse_gvariant_list(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let plan = plan(&desktop, &chord, &exe, &existing);

    println!(
        "Shortcut: {chord}\nDesktop: {:?}\n\n{}\n",
        plan.desktop, plan.note
    );
    if plan.commands.is_empty() {
        return ExitCode::SUCCESS;
    }
    println!("This will run:");
    for command in &plan.commands {
        println!("    {}", command.join(" "));
    }
    print!("\nApply? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err()
        || !matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes" | "o" | "oui"
        )
    {
        println!("Nothing was changed.");
        return ExitCode::SUCCESS;
    }
    for command in &plan.commands {
        let Some((program, args)) = command.split_first() else {
            continue;
        };
        match Command::new(program).args(args).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("popupai: `{program}` failed ({status}); the setup is incomplete.");
                return ExitCode::FAILURE;
            }
            Err(err) => {
                eprintln!("popupai: could not run `{program}`: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    println!("Done.");
    ExitCode::SUCCESS
}
