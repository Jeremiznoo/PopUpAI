const PLACEHOLDER: &str = "[REDACTED]";

/// Headers whose values must never be logged.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "x-goog-api-key",
    "api-key",
    "cookie",
    "set-cookie",
];

/// Query-string keys that commonly carry credentials.
const QUERY_KEYS: &[&str] = &["key", "api_key", "apikey", "token", "access_token", "auth"];

/// Well-known API key prefixes. A match needs a long token after the prefix, so
/// ordinary words such as "risk-free" or "sk-learn" are left alone.
const KEY_PREFIXES: &[&str] = &[
    "sk-",
    "AIza",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "github_pat_",
    "xai-",
    "gsk_",
    "hf_",
    "glpat-",
];
const MIN_TOKEN_LEN: usize = 16;

/// Whether a header's value must be hidden when logging requests.
pub fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|h| h.eq_ignore_ascii_case(name))
}

/// Mask anything that looks like a credential in free text before it reaches a
/// log line or an error message.
///
/// This is a safety net, not a licence to log secrets: it recognises sensitive
/// header values, bearer tokens, credential query parameters and well-known key
/// prefixes. Prompts and answers must not be logged at all by default.
pub fn redact(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if at_boundary(input.as_bytes(), i)
            && let Some((value_start, value_end)) = find_secret(input, &lower, i)
        {
            out.push_str(&input[i..value_start]);
            out.push_str(PLACEHOLDER);
            i = value_end;
            continue;
        }
        let ch = input[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn at_boundary(bytes: &[u8], i: usize) -> bool {
    i == 0 || !bytes[i - 1].is_ascii_alphanumeric()
}

/// If a secret starts at `i`, return the byte range of the part to mask.
fn find_secret(input: &str, lower: &str, i: usize) -> Option<(usize, usize)> {
    header_value(input, lower, i)
        .or_else(|| bearer_token(input, lower, i))
        .or_else(|| query_value(input, lower, i))
        .or_else(|| prefixed_key(input, i))
}

fn header_value(input: &str, lower: &str, i: usize) -> Option<(usize, usize)> {
    let name = SENSITIVE_HEADERS
        .iter()
        .find(|h| lower[i..].starts_with(**h))?;
    let bytes = input.as_bytes();
    let mut j = i + name.len();
    j = skip(bytes, j, |b| b == b'"' || b == b'\'');
    j = skip(bytes, j, |b| b == b' ' || b == b'\t');
    if !matches!(bytes.get(j), Some(b':' | b'=')) {
        return None;
    }
    j += 1;
    j = skip(bytes, j, |b| b == b' ' || b == b'\t');
    j = skip(bytes, j, |b| b == b'"' || b == b'\'');
    let end = until(bytes, j, |b| {
        matches!(b, b'\r' | b'\n' | b'"' | b'\'' | b'}')
    });
    (end > j).then_some((j, end))
}

fn bearer_token(input: &str, lower: &str, i: usize) -> Option<(usize, usize)> {
    if !lower[i..].starts_with("bearer ") {
        return None;
    }
    let start = i + "bearer ".len();
    let end = until(input.as_bytes(), start, |b| {
        b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',')
    });
    (end > start).then_some((start, end))
}

fn query_value(input: &str, lower: &str, i: usize) -> Option<(usize, usize)> {
    let key = QUERY_KEYS.iter().find(|k| {
        lower[i..].starts_with(**k) && input.as_bytes().get(i + k.len()) == Some(&b'=')
    })?;
    let start = i + key.len() + 1;
    let end = until(input.as_bytes(), start, |b| {
        b.is_ascii_whitespace() || matches!(b, b'&' | b'"' | b'\'' | b'#')
    });
    (end > start).then_some((start, end))
}

fn prefixed_key(input: &str, i: usize) -> Option<(usize, usize)> {
    let prefix = KEY_PREFIXES.iter().find(|p| input[i..].starts_with(**p))?;
    let start = i + prefix.len();
    let end = until(input.as_bytes(), start, |b| {
        !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    (end - start >= MIN_TOKEN_LEN).then_some((i, end))
}

fn skip(bytes: &[u8], mut j: usize, pred: impl Fn(u8) -> bool) -> usize {
    while j < bytes.len() && pred(bytes[j]) {
        j += 1;
    }
    j
}

fn until(bytes: &[u8], mut j: usize, stop: impl Fn(u8) -> bool) -> usize {
    while j < bytes.len() && !stop(bytes[j]) {
        j += 1;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_authorization_and_bearer_values() {
        assert_eq!(
            redact("Authorization: Bearer abc123def"),
            "Authorization: [REDACTED]"
        );
        assert_eq!(
            redact("sent header with Bearer abc123def, retrying"),
            "sent header with Bearer [REDACTED], retrying"
        );
    }

    #[test]
    fn masks_header_values_in_debug_style_output() {
        let out =
            redact(r#"headers: {"x-api-key": "sk-ant-api03-abcdefghijklmnop", "accept": "text"}"#);
        assert!(!out.contains("sk-ant"), "{out}");
        assert!(out.contains(r#""accept": "text""#), "{out}");
        let out = redact("x-goog-api-key=AIzaSyA1234567890abcdefghijklmn\nnext line");
        assert!(!out.contains("AIza"), "{out}");
        assert!(out.ends_with("next line"));
    }

    #[test]
    fn masks_credential_query_parameters() {
        assert_eq!(
            redact("GET /v1beta/models?key=AIzaSyA1234567890abcdefghijklmn&alt=sse"),
            "GET /v1beta/models?key=[REDACTED]&alt=sse"
        );
        assert_eq!(
            redact("?access_token=abc&x=1"),
            "?access_token=[REDACTED]&x=1"
        );
    }

    #[test]
    fn masks_well_known_key_prefixes() {
        let out = redact("using sk-proj-abcdefghijklmnopqrstuvwxyz today");
        assert_eq!(out, "using [REDACTED] today");
        let out = redact("pat ghp_abcdefghijklmnopqrstuvwxyz0123 end");
        assert_eq!(out, "pat [REDACTED] end");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for text in [
            "risk-free sketch task-based desk-lamp",
            "sk-short and AIza too",
            "the authorization process, in general, is slow",
            "monkey=banana and hotkey=ctrl",
            "",
        ] {
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn is_safe_on_non_ascii_text() {
        assert_eq!(
            redact("héllo Bearer tök wörld"),
            "héllo Bearer [REDACTED] wörld"
        );
        // The value ends at whitespace, so the text after it is kept.
        assert_eq!(
            redact("日本語 key=値 終わり"),
            "日本語 key=[REDACTED] 終わり"
        );
    }

    #[test]
    fn recognises_sensitive_header_names_case_insensitively() {
        assert!(is_sensitive_header("Authorization"));
        assert!(is_sensitive_header("X-API-KEY"));
        assert!(is_sensitive_header("x-goog-api-key"));
        assert!(!is_sensitive_header("content-type"));
    }
}
