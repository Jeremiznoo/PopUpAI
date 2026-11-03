//! The ChatGPT sign-in (device code flow) and the Responses call, against a scripted local
//! server. This shows that the client follows the flow as published by the Codex CLI; it
//! does not show that OpenAI's real servers behave the same way.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine;
use futures_util::StreamExt;
use popupai_ai::chatgpt_login::{self, Poll, Tokens};
use popupai_ai::{
    ChatGptProvider, ChatMessage, ChatRequest, Provider, ProviderConfig, ProviderError,
};
use popupai_security::{Endpoint, HttpPolicy};
use support::{Mock, Reply};

fn id_token() -> String {
    let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    format!(
        "{}.{}.sig",
        enc(br#"{"alg":"none"}"#),
        enc(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct_42"}}"#)
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn the_device_code_flow_gives_tokens_and_waits_while_the_user_has_not_confirmed() {
    let polls = Arc::new(AtomicUsize::new(0));
    let seen = polls.clone();
    let id = id_token();
    let mock = Mock::start(move |req| match req.target.as_str() {
        "/api/accounts/deviceauth/usercode" => Reply::json(
            r#"{"device_auth_id":"dev-1","user_code":"ABCD-1234","interval":"2"}"#,
        ),
        "/api/accounts/deviceauth/token" => {
            // Pending twice (403, then 404), then confirmed.
            match seen.fetch_add(1, Ordering::SeqCst) {
                0 => Reply::status(403, "{}"),
                1 => Reply::status(404, "{}"),
                _ => Reply::json(r#"{"authorization_code":"auth-code","code_challenge":"c","code_verifier":"verifier-1"}"#),
            }
        }
        "/oauth/token" => Reply::json(&format!(
            r#"{{"access_token":"access-1","refresh_token":"refresh-1","id_token":"{id}","expires_in":3600}}"#
        )),
        other => Reply::status(500, other),
    })
    .await;
    let issuer = mock.url();
    let http = client();

    let code = chatgpt_login::request_device_code(&http, &issuer)
        .await
        .unwrap();
    assert_eq!(code.user_code, "ABCD-1234");
    assert_eq!(code.interval, Duration::from_secs(2));
    assert_eq!(code.verification_url, format!("{issuer}/codex/device"));
    let first = &mock.requests()[0];
    assert_eq!(first.body_json()["client_id"], chatgpt_login::CLIENT_ID);

    assert!(matches!(
        chatgpt_login::poll(&http, &issuer, &code).await.unwrap(),
        Poll::Pending
    ));
    assert!(matches!(
        chatgpt_login::poll(&http, &issuer, &code).await.unwrap(),
        Poll::Pending
    ));
    let Poll::Confirmed {
        authorization_code,
        code_verifier,
    } = chatgpt_login::poll(&http, &issuer, &code).await.unwrap()
    else {
        panic!("expected a confirmation");
    };
    let poll_request = &mock.requests()[3];
    assert_eq!(poll_request.body_json()["device_auth_id"], "dev-1");
    assert_eq!(poll_request.body_json()["user_code"], "ABCD-1234");

    let tokens = chatgpt_login::exchange(&http, &issuer, &authorization_code, &code_verifier)
        .await
        .unwrap();
    assert_eq!(tokens.access_token, "access-1");
    assert_eq!(tokens.account_id.as_deref(), Some("acct_42"));
    let exchange = mock.requests().last().cloned().unwrap();
    assert_eq!(
        exchange.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    for expected in [
        "grant_type=authorization_code",
        "code=auth-code",
        "code_verifier=verifier-1",
        &format!("client_id={}", chatgpt_login::CLIENT_ID),
    ] {
        assert!(
            exchange.body.contains(expected),
            "{expected} in {}",
            exchange.body
        );
    }
    assert!(
        exchange
            .body
            .contains("redirect_uri=http%3A%2F%2F127.0.0.1"),
        "{}",
        exchange.body
    );
}

#[tokio::test]
async fn a_server_without_device_login_and_a_refused_sign_in_are_reported_plainly() {
    let mock = Mock::start(|req| match req.target.as_str() {
        "/api/accounts/deviceauth/usercode" => Reply::status(404, "{}"),
        _ => Reply::status(
            400,
            r#"{"error":"invalid_grant","error_description":"secret detail"}"#,
        ),
    })
    .await;
    let http = client();
    let error = chatgpt_login::request_device_code(&http, &mock.url())
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidRequest(_)),
        "{error:?}"
    );

    let error = chatgpt_login::exchange(&http, &mock.url(), "c", "v")
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Auth { .. }), "{error:?}");
    assert!(
        !error.to_string().contains("secret detail"),
        "the server's text is not repeated"
    );
}

#[tokio::test]
async fn refreshing_keeps_the_account_and_takes_a_rotated_refresh_token() {
    let mock = Mock::start(|_| {
        Reply::json(r#"{"access_token":"access-2","refresh_token":"refresh-2","expires_in":1800}"#)
    })
    .await;
    let old = Tokens {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        account_id: Some("acct_42".into()),
        expires_at: 0,
    };
    let renewed = chatgpt_login::refresh(&client(), &mock.url(), &old)
        .await
        .unwrap();
    assert_eq!(renewed.access_token, "access-2");
    assert_eq!(renewed.refresh_token, "refresh-2");
    assert_eq!(renewed.account_id.as_deref(), Some("acct_42"));
    let sent = mock.requests()[0].body_json();
    assert_eq!(sent["grant_type"], "refresh_token");
    assert_eq!(sent["refresh_token"], "refresh-1");

    let kept = Mock::start(|_| Reply::json(r#"{"access_token":"a3"}"#)).await;
    let renewed = chatgpt_login::refresh(&client(), &kept.url(), &old)
        .await
        .unwrap();
    assert_eq!(
        renewed.refresh_token, "refresh-1",
        "an unchanged refresh token is kept"
    );
}

#[tokio::test]
async fn the_provider_sends_the_tokens_and_streams_the_answer() {
    let sse = [
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
        "data: {\"type\":\"response.completed\"}\n\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&sse)).await;
    let tokens = Tokens {
        access_token: "access-token-value".into(),
        refresh_token: "refresh-token-value".into(),
        account_id: Some("acct_42".into()),
        expires_at: u64::MAX / 2,
    };
    let endpoint = Endpoint::parse(&mock.url(), HttpPolicy::default()).unwrap();
    let provider =
        ChatGptProvider::new(ProviderConfig::new(endpoint).with_api_key(tokens.to_secret()))
            .unwrap();
    let request = ChatRequest {
        model: "gpt-6-luna".into(),
        messages: vec![ChatMessage::user("Hello?")],
        ..Default::default()
    };
    let stream = provider.stream_chat(request).await.unwrap();
    let text: String = stream
        .map(|r| r.unwrap())
        .collect::<Vec<_>>()
        .await
        .concat();
    assert_eq!(text, "Hello");

    let req = &mock.requests()[0];
    assert_eq!(req.target, "/responses");
    assert_eq!(
        req.header("authorization"),
        Some("Bearer access-token-value")
    );
    assert_eq!(req.header("chatgpt-account-id"), Some("acct_42"));
    assert_eq!(req.body_json()["store"], false);
    assert!(
        !req.body.contains("refresh-token-value"),
        "the refresh token is never sent to the API"
    );
}

#[tokio::test]
async fn without_a_stored_sign_in_the_provider_asks_for_one() {
    let mock = Mock::start(|_| Reply::status(200, "{}")).await;
    let endpoint = Endpoint::parse(&mock.url(), HttpPolicy::default()).unwrap();
    let provider = ChatGptProvider::new(ProviderConfig::new(endpoint)).unwrap();
    let request = ChatRequest {
        model: "m".into(),
        messages: vec![ChatMessage::user("x")],
        ..Default::default()
    };
    let error = match provider.stream_chat(request).await {
        Err(e) => e,
        Ok(_) => panic!("expected an error"),
    };
    assert!(matches!(error, ProviderError::MissingApiKey), "{error:?}");
    assert!(mock.requests().is_empty());
}
