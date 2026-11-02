//! A first check of what the user pasted in the API key field, before it goes to the
//! secret store. It catches the usual paste accidents; it cannot know whether a key is
//! valid, only the provider can.

const MAX_LEN: usize = 512;

/// `Ok` with the key trimmed of surrounding whitespace, or a message for the user.
/// The message never contains any part of the input.
pub fn check(input: &str) -> Result<&str, &'static str> {
    let key = input.trim();
    if key.is_empty() {
        Err("Paste the API key first.")
    } else if key.len() > MAX_LEN {
        Err("That is too long to be an API key.")
    } else if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Err("That does not look like an API key: it contains spaces or line breaks.")
    } else {
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_key_is_trimmed() {
        assert_eq!(check("  sk-ant-abc123\n"), Ok("sk-ant-abc123"));
    }

    #[test]
    fn empty_input_is_refused() {
        assert!(check("").is_err());
        assert!(check("  \n ").is_err());
    }

    #[test]
    fn something_that_is_not_one_token_is_refused() {
        assert!(check("sk-abc def").is_err());
        assert!(check("sk-abc\ndef").is_err());
        assert!(check("sk-abc\u{0}def").is_err());
    }

    #[test]
    fn an_oversized_paste_is_refused() {
        assert!(check(&"a".repeat(MAX_LEN + 1)).is_err());
        assert!(check(&"a".repeat(MAX_LEN)).is_ok());
    }

    #[test]
    fn the_messages_never_echo_the_input() {
        for input in ["secret value", "", &"s".repeat(600)] {
            let message = check(input).unwrap_err();
            assert!(!message.contains("secret"), "{message}");
        }
    }
}
