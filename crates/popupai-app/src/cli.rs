//! Command-line options.

pub const USAGE: &str = "\
PopUpAI: ask an AI from anywhere.

Usage: popupai [OPTION]

  (no option)    show the popup; start PopUpAI first if it is not running
  --toggle       show the popup if hidden, hide it if shown
  --background   start hidden and stay running, for autostart at login
  --setup-shortcut
                 bind your shortcut in the desktop, so it starts PopUpAI even when closed
  --quit         stop the running PopUpAI
  -h, --help     show this help

Only one option may be given. A second launch never starts a second PopUpAI: it asks
the running one to do the job and exits.
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Run (or reach) the instance; `visible` says whether the popup shows at start.
    Run {
        visible: bool,
    },
    Toggle,
    Quit,
    Help,
    SetupShortcut,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Action, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Action::Run { visible: true });
    };
    if args.next().is_some() {
        return Err("only one option may be given".to_owned());
    }
    match first.as_str() {
        "--toggle" => Ok(Action::Toggle),
        "--background" => Ok(Action::Run { visible: false }),
        "--quit" => Ok(Action::Quit),
        "--setup-shortcut" => Ok(Action::SetupShortcut),
        "-h" | "--help" => Ok(Action::Help),
        other => Err(format!("unknown option `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Action, String> {
        parse(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn no_option_runs_and_shows_the_popup() {
        assert_eq!(parse_args(&[]), Ok(Action::Run { visible: true }));
    }

    #[test]
    fn each_option_maps_to_its_action() {
        assert_eq!(parse_args(&["--toggle"]), Ok(Action::Toggle));
        assert_eq!(
            parse_args(&["--background"]),
            Ok(Action::Run { visible: false })
        );
        assert_eq!(parse_args(&["--quit"]), Ok(Action::Quit));
        assert_eq!(parse_args(&["--setup-shortcut"]), Ok(Action::SetupShortcut));
        assert_eq!(parse_args(&["-h"]), Ok(Action::Help));
        assert_eq!(parse_args(&["--help"]), Ok(Action::Help));
    }

    #[test]
    fn unknown_or_combined_options_are_rejected_with_a_message() {
        assert_eq!(
            parse_args(&["--frobnicate"]),
            Err("unknown option `--frobnicate`".into())
        );
        assert!(
            parse_args(&["--toggle", "--quit"])
                .unwrap_err()
                .contains("only one")
        );
    }

    #[test]
    fn a_prompt_on_the_command_line_is_not_a_feature() {
        // The remote interface never carries text, and neither does the CLI.
        assert!(parse_args(&["what is DNS?"]).is_err());
    }

    #[test]
    fn the_usage_text_mentions_every_option() {
        for option in ["--toggle", "--background", "--quit", "--help"] {
            assert!(USAGE.contains(option), "{option}");
        }
    }
}
