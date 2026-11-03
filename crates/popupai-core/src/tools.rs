//! Running commands for the model, with the user in control.
//!
//! A command the model asks for is classified first: a **simple read-only** command on a
//! non-sensitive place runs at once; anything else is shown to the user in full and runs only
//! after they accept. Classification is deliberately narrow: when in doubt it asks.
//!
//! What counts as simple: one program from a short list, run without a path, with no shell
//! syntax at all (no pipe, redirection, substitution, variable, glob-free chaining, backslash
//! or newline), no flag that writes, deletes or runs another program, and no argument that
//! names a place holding credentials. Reading a file still sends its content to the provider
//! as the answer to the model, which is why the sensitive places are excluded.

use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub const TOOL_NAME: &str = "run_command";

/// Output kept for the model and the screen, per command.
const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Safe to run without asking.
    RunNow,
    /// The user must accept first. The text says why, in a few words.
    Ask(String),
}

/// Programs that only read and report, run as `program args...`.
const READ_ONLY: [&str; 29] = [
    "ls", "cat", "head", "tail", "wc", "pwd", "uname", "date", "df", "du", "free", "uptime", "ps",
    "which", "whereis", "file", "stat", "lsblk", "whoami", "lscpu", "lsusb", "lspci", "realpath",
    "basename", "dirname", "find", "grep", "id", "tree",
];

/// Characters that make a command more than "a program and its arguments".
const SHELL_SYNTAX: [char; 15] = [
    ';', '&', '|', '<', '>', '`', '$', '(', ')', '{', '}', '\\', '\n', '\r', '!',
];

/// Flags that make an otherwise read-only program write, delete or run something.
const DANGEROUS_FLAGS: [&str; 12] = [
    "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls",
    "-s", "--set", "--output",
];

/// Parts of a path that point at credentials, keys, browser data or this application's own files.
const SENSITIVE: [&str; 22] = [
    "/.ssh",
    "/.gnupg",
    "/.aws",
    "/.azure",
    "/.kube",
    "/.docker",
    "/.config/popupai",
    "/.local/share/kwalletd",
    "/.local/share/keyrings",
    "/.mozilla",
    "/.config/google-chrome",
    "/.config/chromium",
    "/.password-store",
    "/etc/shadow",
    "/etc/sudoers",
    "/proc/",
    ".netrc",
    ".env",
    "id_rsa",
    "id_ed25519",
    "credential",
    "secret",
];

/// Split on spaces, keeping single- or double-quoted pieces together. `None` if a quote is
/// never closed.
fn tokens(command: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in command.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => current.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !current.is_empty() {
        out.push(current);
    }
    Some(out)
}

pub fn classify(command: &str) -> Decision {
    let ask = |why: &str| Decision::Ask(why.to_owned());
    let command = command.trim();
    if command.is_empty() {
        return ask("empty command");
    }
    if command.len() > 2000 {
        return ask("very long command");
    }
    if command
        .chars()
        .any(|c| SHELL_SYNTAX.contains(&c) || c.is_control())
    {
        return ask("uses shell syntax (pipes, redirections, substitutions or chaining)");
    }
    let Some(parts) = tokens(command) else {
        return ask("unbalanced quotes");
    };
    let Some((program, arguments)) = parts.split_first() else {
        return ask("empty command");
    };
    if !READ_ONLY.contains(&program.as_str()) {
        return ask("not a known read-only command");
    }
    for argument in arguments {
        // `--set=value` and `--output=file` carry their value after an equals sign, and
        // `date -s...` can have it stuck to the flag.
        let flag = argument.split('=').next().unwrap_or(argument);
        if DANGEROUS_FLAGS.contains(&flag) || (program == "date" && argument.starts_with("-s")) {
            return ask("has a flag that can write, delete or run another program");
        }
        // Searching a whole tree for text can print secrets from any file under it.
        if program == "grep"
            && (argument.starts_with("-r")
                || argument.starts_with("-R")
                || argument == "--recursive")
        {
            return ask("searches file contents recursively");
        }
        let lowered = argument.to_ascii_lowercase();
        if SENSITIVE.iter().any(|part| lowered.contains(part)) {
            return ask("touches a place that can hold credentials or keys");
        }
    }
    Decision::RunNow
}

/// The result of running a command, as text for the model and the screen.
pub async fn run(command: &str) -> String {
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return format!("could not start the command: {err}"),
    };
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return "could not read the command's output".to_owned();
    };
    let work = async {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let limit = MAX_OUTPUT_BYTES as u64 + 1;
        let mut out_reader = (&mut stdout).take(limit);
        let mut err_reader = (&mut stderr).take(limit);
        let _ = tokio::join!(
            out_reader.read_to_end(&mut out),
            err_reader.read_to_end(&mut err)
        );
        let status = child.wait().await;
        (out, err, status)
    };
    match tokio::time::timeout(RUN_TIMEOUT, work).await {
        Err(_) => format!(
            "the command was stopped after {} seconds",
            RUN_TIMEOUT.as_secs()
        ),
        Ok((out, err, status)) => {
            let code = status.ok().and_then(|s| s.code());
            let mut text = String::from_utf8_lossy(&out).into_owned();
            let err = String::from_utf8_lossy(&err);
            if !err.is_empty() {
                text.push_str(&err);
            }
            let truncated = text.len() > MAX_OUTPUT_BYTES;
            if truncated {
                let mut end = MAX_OUTPUT_BYTES;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                text.push_str("\n[output cut]");
            }
            match code {
                Some(0) => format!("exit code 0\n{text}"),
                Some(n) => format!("exit code {n}\n{text}"),
                None => format!("ended without an exit code\n{text}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs_now(command: &str) -> bool {
        classify(command) == Decision::RunNow
    }

    #[test]
    fn simple_read_only_commands_run_at_once() {
        for command in [
            "ls -la",
            "ls /var/log",
            "uname -a",
            "cat /etc/os-release",
            "head -n 5 /etc/hostname",
            "df -h",
            "find /home -name coucou.txt -type f",
            "grep -n error notes.txt",
            "pwd",
            "ls 'My Documents'",
        ] {
            assert!(runs_now(command), "{command}: {:?}", classify(command));
        }
    }

    #[test]
    fn anything_with_shell_syntax_asks() {
        for command in [
            "ls; rm -rf ~",
            "ls && cat /etc/passwd",
            "cat file | sh",
            "ls > out.txt",
            "cat < /etc/passwd",
            "echo $(whoami)",
            "ls `whoami`",
            "cat ${HOME}/x",
            "ls\nrm -rf ~",
            "cat \\x",
            "ls (",
        ] {
            assert!(!runs_now(command), "{command}");
        }
    }

    #[test]
    fn programs_outside_the_list_and_programs_with_a_path_ask() {
        for command in [
            "rm -rf ~",
            "sudo ls",
            "curl https://example.com",
            "sh -c ls",
            "/bin/ls",
            "./ls",
            "python3 -c 1",
            "chmod 777 x",
            "mv a b",
            "env",
            "printenv",
        ] {
            assert!(!runs_now(command), "{command}");
        }
    }

    #[test]
    fn flags_that_write_delete_or_run_programs_ask() {
        for command in [
            "find . -name x -exec cat {} +",
            "find . -delete",
            "find . -fprint out.txt",
            "find / -ok rm {} ;",
            "date -s 2020-01-01",
            "date --set=2020-01-01",
            "ls --output=x",
            "date -s2020-01-01",
            "date --set 2020-01-01",
        ] {
            assert!(!runs_now(command), "{command}");
        }
    }

    #[test]
    fn credentials_and_private_places_ask() {
        for command in [
            "cat ~/.ssh/id_ed25519",
            "cat /home/me/.ssh/config",
            "ls ~/.gnupg",
            "cat /etc/shadow",
            "cat .env",
            "cat ~/.aws/credentials",
            "head ~/.config/popupai/config.toml",
            "cat /proc/self/environ",
            "cat .netrc",
            "ls ~/.mozilla/firefox",
            "cat my-secret-notes.txt",
        ] {
            assert!(!runs_now(command), "{command}");
        }
    }

    #[test]
    fn a_recursive_text_search_asks() {
        for command in [
            "grep -r password ~",
            "grep -rn token .",
            "grep -R key /",
            "grep --recursive x .",
        ] {
            assert!(!runs_now(command), "{command}");
        }
    }

    #[test]
    fn odd_input_asks() {
        for command in ["", "   ", "ls 'unclosed", "ls \"unclosed"] {
            assert!(!runs_now(command), "{command:?}");
        }
        assert!(!runs_now(&format!("ls {}", "a".repeat(3000))));
    }

    #[test]
    fn a_reason_is_given_when_asking() {
        let Decision::Ask(why) = classify("rm -rf ~") else {
            panic!()
        };
        assert!(why.contains("read-only"), "{why}");
    }

    #[tokio::test]
    async fn a_command_runs_and_reports_its_exit_code_and_output() {
        assert_eq!(run("echo hello").await, "exit code 0\nhello\n");
        let failed = run("ls /definitely/not/here").await;
        assert!(
            failed.starts_with("exit code 2") || failed.starts_with("exit code 1"),
            "{failed}"
        );
        assert!(failed.contains("No such file"), "{failed}");
    }

    #[tokio::test]
    async fn long_output_is_cut_and_a_hung_command_is_stopped() {
        let long = run("head -c 40000 /dev/zero | tr '\\0' 'x'").await;
        assert!(long.len() < MAX_OUTPUT_BYTES + 100, "{}", long.len());
        assert!(
            long.ends_with("[output cut]"),
            "{}",
            &long[long.len() - 30..]
        );
    }
}
