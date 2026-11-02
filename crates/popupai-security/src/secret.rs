use std::fmt;

use zeroize::Zeroizing;

/// A secret such as an API key.
///
/// It never appears in `Debug` output, has no `Display` or `Serialize`
/// implementation, and its memory is overwritten when dropped. The only way to
/// read it is the explicit [`Secret::expose`], so every use is easy to grep for.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The raw value. Never log or format the result.
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_value() {
        let secret = Secret::new("sk-super-secret-value");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(Secret(***))");
        assert_eq!(secret.expose(), "sk-super-secret-value");
    }

    #[test]
    fn clones_are_independent_and_equally_redacted() {
        let secret = Secret::new("abc");
        let copy = secret.clone();
        drop(secret);
        assert_eq!(copy.expose(), "abc");
        assert!(!copy.is_empty());
        assert!(Secret::new("").is_empty());
    }
}
