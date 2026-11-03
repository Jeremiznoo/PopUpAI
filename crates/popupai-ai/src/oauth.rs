//! Signing in with a ChatGPT account, by the OAuth *device code* flow.
//!
//! **Status: written from the flow published in OpenAI's open-source Codex CLI and
//! reproduced by opencode. It is not a documented public API of OpenAI, it can change or
//! be withdrawn without notice, and its use with third-party applications may not be
//! covered by OpenAI's terms. It has been tested against a scripted local server only; the
//! first real sign-in is the first real test.**
//!
//! The user is shown a short code and a page on `auth.openai.com`, signs in there in the
//! browser (the application never sees the password), and the application polls until the
//! sign-in is confirmed, then exchanges the result for tokens.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use popupai_security::Secret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroize;

use crate::ProviderError;
use crate::error::from_reqwest;

/// A client for the sign-in server: no redirects are followed, and every wait is bounded.
pub fn http_client(timeout: Duration) -> Result<Client, ProviderError> {
    crate::driver::build_client(timeout)
}

/// The public identifier of the Codex CLI's OAuth client. It is not a secret.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";

/// What the user has to do to confirm the sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_auth_id: String,
    /// Shown to the user, who types it on the verification page.
    pub user_code: String,
    pub verification_url: String,
    /// How long to wait between polls.
    pub interval: Duration,
}

pub enum Poll {
    /// Not confirmed yet.
    Pending,
    Confirmed {
        authorization_code: String,
        code_verifier: String,
    },
}

/// The result of a sign-in. Kept in the system secret store as one JSON secret.
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Sent as the `chatgpt-account-id` header.
    pub account_id: Option<String>,
    /// Seconds since the Unix epoch.
    pub expires_at: u64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("account_id", &self.account_id.is_some())
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Drop for Tokens {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Tokens {
    pub fn to_secret(&self) -> Secret {
        Secret::new(serde_json::to_string(self).unwrap_or_default())
    }

    pub fn from_secret(secret: &Secret) -> Result<Self, ProviderError> {
        serde_json::from_str(secret.expose())
            .map_err(|_| ProviderError::Decode("the stored sign-in is not readable".into()))
    }

    /// Whether the access token has expired, or will within `margin`.
    pub fn expires_within(&self, margin: Duration) -> bool {
        self.expires_at <= now() + margin.as_secs()
    }
}

/// The ChatGPT account id, read from the `https://api.openai.com/auth` claim of a token.
/// The token is only decoded, not verified: the id is a routing header, and the server
/// checks the token itself.
pub fn account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn issuer_url(issuer: &str, path: &str) -> String {
    format!(
        "{}/{}",
        issuer.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn bad_reply(what: &str) -> ProviderError {
    ProviderError::Decode(format!("the sign-in server's {what} was not understood"))
}

async fn json_body(response: reqwest::Response) -> Result<Value, ProviderError> {
    let bytes = response.bytes().await.map_err(from_reqwest)?;
    if bytes.len() > 256 * 1024 {
        return Err(bad_reply("answer"));
    }
    serde_json::from_slice(&bytes).map_err(|_| bad_reply("answer"))
}

/// A failed sign-in step as an error that never repeats the server's text.
fn rejected(status: reqwest::StatusCode) -> ProviderError {
    match status.as_u16() {
        401 | 403 | 400 => ProviderError::Auth {
            status: status.as_u16(),
            message: "the sign-in was refused".into(),
        },
        code => ProviderError::Api {
            status: code,
            message: "the sign-in server could not complete the request".into(),
        },
    }
}

/// Step 1: ask for a code to show the user.
pub async fn request_device_code(
    client: &Client,
    issuer: &str,
) -> Result<DeviceCode, ProviderError> {
    let response = client
        .post(issuer_url(issuer, "api/accounts/deviceauth/usercode"))
        .json(&json!({ "client_id": CLIENT_ID }))
        .send()
        .await
        .map_err(from_reqwest)?;
    if response.status().as_u16() == 404 {
        return Err(ProviderError::InvalidRequest(
            "signing in with a device code is not enabled for this account or server".into(),
        ));
    }
    if !response.status().is_success() {
        return Err(rejected(response.status()));
    }
    let body = json_body(response).await?;
    let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
    let device_auth_id = text("device_auth_id").ok_or_else(|| bad_reply("device id"))?;
    let user_code = text("user_code")
        .or_else(|| text("usercode"))
        .ok_or_else(|| bad_reply("code"))?;
    // The interval comes as a string or a number; never poll faster than once a second.
    let interval = body
        .get("interval")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(5)
        .clamp(1, 60);
    Ok(DeviceCode {
        device_auth_id,
        user_code,
        verification_url: issuer_url(issuer, "codex/device"),
        interval: Duration::from_secs(interval),
    })
}

/// Step 2 (repeated): has the user confirmed? A 403 or 404 means not yet.
pub async fn poll(client: &Client, issuer: &str, code: &DeviceCode) -> Result<Poll, ProviderError> {
    let response = client
        .post(issuer_url(issuer, "api/accounts/deviceauth/token"))
        .json(&json!({ "device_auth_id": code.device_auth_id, "user_code": code.user_code }))
        .send()
        .await
        .map_err(from_reqwest)?;
    match response.status().as_u16() {
        403 | 404 => Ok(Poll::Pending),
        200..=299 => {
            let body = json_body(response).await?;
            let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
            Ok(Poll::Confirmed {
                authorization_code: text("authorization_code").ok_or_else(|| bad_reply("code"))?,
                code_verifier: text("code_verifier").ok_or_else(|| bad_reply("verifier"))?,
            })
        }
        _ => Err(rejected(response.status())),
    }
}

fn tokens_from(body: &Value, previous_refresh: Option<&str>) -> Result<Tokens, ProviderError> {
    let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::to_owned);
    let access_token = text("access_token").ok_or_else(|| bad_reply("token"))?;
    let refresh_token = text("refresh_token")
        .or_else(|| previous_refresh.map(str::to_owned))
        .ok_or_else(|| bad_reply("refresh token"))?;
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let account_id = text("id_token")
        .as_deref()
        .and_then(account_id_from_jwt)
        .or_else(|| account_id_from_jwt(&access_token));
    Ok(Tokens {
        access_token,
        refresh_token,
        account_id,
        expires_at: now() + expires_in,
    })
}

fn form(fields: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish()
}

/// Step 3: trade the confirmed sign-in for tokens.
pub async fn exchange(
    client: &Client,
    issuer: &str,
    authorization_code: &str,
    code_verifier: &str,
) -> Result<Tokens, ProviderError> {
    let redirect = issuer_url(issuer, "deviceauth/callback");
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", authorization_code),
        ("redirect_uri", &redirect),
        ("client_id", CLIENT_ID),
        ("code_verifier", code_verifier),
    ]);
    let response = client
        .post(issuer_url(issuer, "oauth/token"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .map_err(from_reqwest)?;
    if !response.status().is_success() {
        return Err(rejected(response.status()));
    }
    tokens_from(&json_body(response).await?, None)
}

/// Get a new access token. The refresh token may be replaced by the server, so the caller
/// must store what comes back.
pub async fn refresh(
    client: &Client,
    issuer: &str,
    tokens: &Tokens,
) -> Result<Tokens, ProviderError> {
    let response = client
        .post(issuer_url(issuer, "oauth/token"))
        .json(&json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": tokens.refresh_token,
            "scope": "openid profile email",
        }))
        .send()
        .await
        .map_err(from_reqwest)?;
    if !response.status().is_success() {
        return Err(rejected(response.status()));
    }
    let mut renewed = tokens_from(&json_body(response).await?, Some(&tokens.refresh_token))?;
    if renewed.account_id.is_none() {
        renewed.account_id.clone_from(&tokens.account_id);
    }
    Ok(renewed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &Value) -> String {
        let encode = |v: &[u8]| URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            encode(br#"{"alg":"none"}"#),
            encode(payload.to_string().as_bytes())
        )
    }

    #[test]
    fn the_account_id_is_read_from_the_auth_claim() {
        let token =
            jwt(&json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acct_123"}}));
        assert_eq!(account_id_from_jwt(&token).as_deref(), Some("acct_123"));
        assert_eq!(account_id_from_jwt(&jwt(&json!({"sub": "x"}))), None);
        assert_eq!(account_id_from_jwt("not-a-jwt"), None);
        assert_eq!(account_id_from_jwt("a.%%%.c"), None);
    }

    #[test]
    fn tokens_round_trip_through_a_secret_and_never_show_themselves_in_debug() {
        let tokens = Tokens {
            access_token: "access-secret-value".into(),
            refresh_token: "refresh-secret-value".into(),
            account_id: Some("acct".into()),
            expires_at: now() + 3600,
        };
        let restored = Tokens::from_secret(&tokens.to_secret()).unwrap();
        assert_eq!(restored.access_token, "access-secret-value");
        let shown = format!("{tokens:?}");
        assert!(!shown.contains("secret-value"), "{shown}");
        assert!(Tokens::from_secret(&Secret::new("{broken")).is_err());
    }

    #[test]
    fn expiry_is_judged_with_a_margin() {
        let mut tokens = Tokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            account_id: None,
            expires_at: now() + 600,
        };
        assert!(!tokens.expires_within(Duration::from_secs(60)));
        assert!(tokens.expires_within(Duration::from_secs(900)));
        tokens.expires_at = now().saturating_sub(5);
        assert!(tokens.expires_within(Duration::ZERO));
    }

    #[test]
    fn the_form_body_is_encoded() {
        assert_eq!(form(&[("a", "b c"), ("d", "e&f")]), "a=b+c&d=e%26f");
    }
}
