//! End-to-end tests of the adapters against a scripted local server. They cover
//! what the unit tests cannot: real sockets, the headers that are sent, error
//! mapping, redirect handling, timeouts and cancellation.

mod support;

use std::time::Duration;

use futures_util::StreamExt;
use popupai_ai::{
    AnthropicProvider, ChatMessage, ChatRequest, ChatStream, GeminiProvider, OllamaProvider,
    OpenAiFlavor, OpenAiProvider, Provider, ProviderConfig, ProviderError,
};
use popupai_security::{Endpoint, HttpPolicy, Secret};
use support::{Mock, Reply};

const KEY: &str = "test-key-0123456789abcdef";

fn config(mock: &Mock, path: &str) -> ProviderConfig {
    let endpoint =
        Endpoint::parse(&format!("{}{path}", mock.url()), HttpPolicy::default()).unwrap();
    ProviderConfig::new(endpoint)
}

fn keyed(mock: &Mock, path: &str) -> ProviderConfig {
    config(mock, path).with_api_key(Secret::new(KEY))
}

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        messages: vec![ChatMessage::user("Hello?")],
        ..Default::default()
    }
}

fn ok<T>(result: Result<T, ProviderError>) -> T {
    result.unwrap_or_else(|e| panic!("expected success, got: {e}"))
}

fn err<T>(result: Result<T, ProviderError>) -> ProviderError {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

async fn collect(stream: ChatStream) -> Vec<Result<String, ProviderError>> {
    stream.collect().await
}

async fn text(stream: ChatStream) -> String {
    collect(stream).await.into_iter().map(ok).collect()
}

async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    for _ in 0..150 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for: {what}");
}

const OPENAI_SSE: [&str; 2] = [
    "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\nda",
    "ta: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n",
];

// ---------------------------------------------------------------- adapters

#[tokio::test]
async fn openai_streams_text_and_keeps_the_key_in_the_header_only() {
    let mock = Mock::start(|_| Reply::stream(&OPENAI_SSE)).await;
    let provider = OpenAiProvider::new(keyed(&mock, "/v1"), OpenAiFlavor::OpenAi).unwrap();

    let stream = ok(provider.stream_chat(request("gpt-4o-mini")).await);
    assert_eq!(text(stream).await, "Hello");

    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(
        (req.method.as_str(), req.target.as_str()),
        ("POST", "/v1/chat/completions")
    );
    assert_eq!(
        req.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert!(req.header("content-type").unwrap().contains("json"));
    assert!(req.header("user-agent").unwrap().starts_with("PopUpAI/"));
    assert!(
        !req.target.contains("test-key"),
        "the key must never be in the URL"
    );
    assert!(
        !req.body.contains("test-key"),
        "the key must never be in the body"
    );
    let body = req.body_json();
    assert_eq!(body["model"], "gpt-4o-mini");
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn openai_compatible_server_works_without_a_key() {
    let mock = Mock::start(|_| Reply::stream(&OPENAI_SSE)).await;
    let mut req = request("local-model");
    req.max_tokens = Some(32);
    let provider = OpenAiProvider::new(config(&mock, "/v1"), OpenAiFlavor::Compatible).unwrap();

    assert_eq!(text(ok(provider.stream_chat(req).await)).await, "Hello");

    let recorded = &mock.requests()[0];
    assert!(recorded.header("authorization").is_none());
    assert_eq!(recorded.body_json()["max_tokens"], 32);
}

#[tokio::test]
async fn anthropic_sends_its_headers_and_streams_until_message_stop() {
    let sse = [
        "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&sse)).await;
    let provider = AnthropicProvider::new(keyed(&mock, "/v1")).unwrap();

    assert_eq!(
        text(ok(provider.stream_chat(request("claude-sonnet-5-5")).await)).await,
        "Hello"
    );

    let req = &mock.requests()[0];
    assert_eq!(req.target, "/v1/messages");
    assert_eq!(req.header("x-api-key"), Some(KEY));
    assert_eq!(req.header("anthropic-version"), Some("2023-06-01"));
    assert!(req.header("authorization").is_none());
    assert!(!req.target.contains("test-key"));
    assert_eq!(req.body_json()["max_tokens"], 4096);
}

#[tokio::test]
async fn gemini_puts_the_model_in_the_path_and_the_key_in_a_header() {
    let sse = [
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}]}\r\n\r\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&sse)).await;
    let provider = GeminiProvider::new(keyed(&mock, "/v1beta")).unwrap();

    assert_eq!(
        text(ok(provider.stream_chat(request("gemini-2.0-flash")).await)).await,
        "Hello"
    );

    let req = &mock.requests()[0];
    assert_eq!(
        req.target,
        "/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
    );
    assert_eq!(req.header("x-goog-api-key"), Some(KEY));
    assert!(!req.target.contains("key"), "no key parameter in the URL");
}

#[tokio::test]
async fn ollama_streams_ndjson_without_authentication() {
    let ndjson = [
        "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n{\"message\":{\"con",
        "tent\":\"lo\"},\"done\":false}\n{\"message\":{\"content\":\"\"},\"done\":true}\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&ndjson)).await;
    let provider = OllamaProvider::new(config(&mock, "")).unwrap();

    assert_eq!(
        text(ok(provider.stream_chat(request("llama3.2:latest")).await)).await,
        "Hello"
    );

    let req = &mock.requests()[0];
    assert_eq!(req.target, "/api/chat");
    assert!(req.header("authorization").is_none());
    assert_eq!(req.body_json()["model"], "llama3.2:latest");
}

#[tokio::test]
async fn every_adapter_lists_models() {
    let mock = Mock::start(|req| match req.target.split('?').next().unwrap() {
        "/api/tags" => Reply::json(r#"{"models":[{"name":"qwen2.5:3b"},{"name":"llama3.2:latest"}]}"#),
        "/v1/models" => Reply::json(r#"{"data":[{"id":"gpt-b"},{"id":"gpt-a"}]}"#),
        "/v1beta/models" => Reply::json(
            r#"{"models":[{"name":"models/gemini-x","supportedGenerationMethods":["generateContent"]},{"name":"models/embed","supportedGenerationMethods":["embedContent"]}]}"#,
        ),
        other => Reply::status(404, &format!("{{\"error\":\"unexpected {other}\"}}")),
    })
    .await;
    let ids = |models: Vec<popupai_ai::ModelInfo>| -> Vec<String> {
        models.into_iter().map(|m| m.id).collect()
    };

    let ollama = OllamaProvider::new(config(&mock, "")).unwrap();
    assert_eq!(
        ids(ok(ollama.list_models().await)),
        ["llama3.2:latest", "qwen2.5:3b"]
    );

    let openai = OpenAiProvider::new(keyed(&mock, "/v1"), OpenAiFlavor::OpenAi).unwrap();
    assert_eq!(ids(ok(openai.list_models().await)), ["gpt-a", "gpt-b"]);

    let anthropic = AnthropicProvider::new(keyed(&mock, "/v1")).unwrap();
    assert_eq!(ids(ok(anthropic.list_models().await)), ["gpt-a", "gpt-b"]);
    let anthropic_req = mock
        .requests()
        .into_iter()
        .find(|r| r.header("x-api-key").is_some())
        .unwrap();
    assert_eq!(anthropic_req.target, "/v1/models?limit=1000");

    let gemini = GeminiProvider::new(keyed(&mock, "/v1beta")).unwrap();
    assert_eq!(ids(ok(gemini.list_models().await)), ["gemini-x"]);
}

// ------------------------------------------------------------- error paths

#[tokio::test]
async fn authentication_failures_are_classified_and_never_echo_the_key() {
    let mock = Mock::start(|_| {
        Reply::status(
            401,
            r#"{"error":{"message":"Incorrect API key provided: sk-proj-abcdefghijklmnopqrstuvwx"}}"#,
        )
    })
    .await;
    let provider = OpenAiProvider::new(keyed(&mock, "/v1"), OpenAiFlavor::OpenAi).unwrap();

    let error = err(provider.stream_chat(request("gpt-4o-mini")).await);
    assert!(
        matches!(error, ProviderError::Auth { status: 401, .. }),
        "{error}"
    );
    let shown = format!("{error} / {error:?}");
    assert!(!shown.contains("sk-proj"), "{shown}");
    assert!(!shown.contains(KEY), "{shown}");
}

#[tokio::test]
async fn rate_limits_carry_retry_after_and_other_statuses_become_api_errors() {
    let mock = Mock::start(|req| match req.body_json()["model"].as_str().unwrap() {
        "limited" => Reply::Fixed {
            status: 429,
            headers: vec![("retry-after", "7".into())],
            body: "{}".into(),
        },
        "missing" => Reply::status(404, r#"{"error":"model 'missing' not found"}"#),
        _ => Reply::status(500, "upstream exploded"),
    })
    .await;
    let provider = OllamaProvider::new(config(&mock, "")).unwrap();

    let limited = err(provider.stream_chat(request("limited")).await);
    assert!(
        matches!(limited, ProviderError::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(7))
    );

    let missing = err(provider.stream_chat(request("missing")).await);
    assert!(
        matches!(&missing, ProviderError::Api { status: 404, message } if message.contains("not found")),
        "{missing}"
    );

    let broken = err(provider.stream_chat(request("boom")).await);
    assert!(
        matches!(&broken, ProviderError::Api { status: 500, message } if message == "upstream exploded"),
        "{broken}"
    );
}

#[tokio::test]
async fn redirects_are_refused_and_credentials_never_reach_the_target() {
    let target = Mock::start(|_| Reply::json("{}")).await;
    let port = target.addr.port();
    let origin = Mock::start(move |_| Reply::Fixed {
        status: 302,
        headers: vec![("location", format!("http://127.0.0.1:{port}/steal"))],
        body: String::new(),
    })
    .await;
    let provider = AnthropicProvider::new(keyed(&origin, "/v1")).unwrap();

    let error = err(provider.stream_chat(request("claude-sonnet-5-5")).await);

    assert!(
        matches!(error, ProviderError::RedirectRefused { status: 302 }),
        "{error}"
    );
    assert_eq!(origin.requests().len(), 1);
    assert!(
        target.requests().is_empty(),
        "the redirect target must never be contacted"
    );
}

#[tokio::test]
async fn invalid_requests_and_unusable_keys_fail_before_any_network_call() {
    let mock = Mock::start(|_| Reply::json("{}")).await;
    let provider = OpenAiProvider::new(keyed(&mock, "/v1"), OpenAiFlavor::Compatible).unwrap();
    let bad_model = err(provider.stream_chat(request("no way?key=1")).await);
    assert!(matches!(bad_model, ProviderError::InvalidRequest(_)));

    let bad_key = ProviderConfig::new(
        Endpoint::parse(&format!("{}/v1", mock.url()), HttpPolicy::default()).unwrap(),
    )
    .with_api_key(Secret::new("bad\nkey-with-newline"));
    let provider = OpenAiProvider::new(bad_key, OpenAiFlavor::Compatible).unwrap();
    let error = err(provider.stream_chat(request("m")).await);
    assert!(matches!(error, ProviderError::InvalidRequest(_)));
    assert!(
        !format!("{error}").contains("bad"),
        "the key must not appear in the message"
    );

    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn a_server_that_is_not_running_is_reported_as_unreachable() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let endpoint =
        Endpoint::parse(&format!("http://127.0.0.1:{port}"), HttpPolicy::default()).unwrap();
    let provider = OllamaProvider::new(ProviderConfig::new(endpoint)).unwrap();

    let error = err(provider.list_models().await);
    assert!(matches!(error, ProviderError::Unreachable), "{error}");
}

// ------------------------------------------------- streaming failure modes

#[tokio::test]
async fn a_silent_server_times_out() {
    let mock = Mock::start(|_| Reply::Silent).await;
    let config = config(&mock, "").with_timeout(Duration::from_millis(200));
    let provider = OllamaProvider::new(config).unwrap();

    let started = std::time::Instant::now();
    let error = err(provider.stream_chat(request("m")).await);

    assert!(matches!(error, ProviderError::Timeout), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn a_stalled_stream_times_out_after_the_text_already_received() {
    let mock = Mock::start(|_| Reply::Stream {
        chunks: vec![
            (
                Duration::from_millis(5),
                "{\"message\":{\"content\":\"a\"},\"done\":false}\n".into(),
            ),
            (
                Duration::from_secs(3),
                "{\"message\":{\"content\":\"b\"},\"done\":true}\n".into(),
            ),
        ],
    })
    .await;
    let provider =
        OllamaProvider::new(config(&mock, "").with_timeout(Duration::from_millis(300))).unwrap();

    let items = collect(ok(provider.stream_chat(request("m")).await)).await;

    assert!(
        matches!(&items[..], [Ok(a), Err(ProviderError::Timeout)] if a == "a"),
        "{items:?}"
    );
}

#[tokio::test]
async fn a_provider_error_in_the_middle_of_a_stream_ends_it() {
    let sse = [
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Par\"}}\n\n",
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"Overloaded\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"never\"}}\n\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&sse)).await;
    let provider = AnthropicProvider::new(keyed(&mock, "/v1")).unwrap();

    let items = collect(ok(provider.stream_chat(request("claude-sonnet-5-5")).await)).await;

    assert!(
        matches!(&items[..], [Ok(a), Err(ProviderError::Stream(m))] if a == "Par" && m == "Overloaded"),
        "{items:?}"
    );
}

#[tokio::test]
async fn a_connection_closed_before_the_end_marker_is_reported_as_cut_off() {
    let sse = [
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Par\"}}\n\n",
    ];
    let mock = Mock::start(move |_| Reply::stream(&sse)).await;
    let provider = AnthropicProvider::new(keyed(&mock, "/v1")).unwrap();

    let items = collect(ok(provider.stream_chat(request("claude-sonnet-5-5")).await)).await;

    assert!(
        matches!(&items[..], [Ok(a), Err(ProviderError::Network(_))] if a == "Par"),
        "{items:?}"
    );
}

// ------------------------------------------------------------ cancellation

#[tokio::test]
async fn dropping_the_stream_aborts_the_request() {
    let mock = Mock::start(|_| Reply::Endless {
        chunk: "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n".into(),
        interval: Duration::from_millis(10),
    })
    .await;
    let provider = OpenAiProvider::new(config(&mock, "/v1"), OpenAiFlavor::Compatible).unwrap();

    let mut stream = ok(provider.stream_chat(request("m")).await);
    assert_eq!(stream.next().await.unwrap().unwrap(), "x");
    assert_eq!(mock.disconnects(), 0, "the server is still streaming");

    drop(stream);

    wait_until("the server to notice the client left", || {
        mock.disconnects() == 1
    })
    .await;
}

#[tokio::test]
async fn abandoning_the_request_before_any_answer_also_aborts_it() {
    let mock = Mock::start(|_| Reply::Silent).await;
    let provider = OllamaProvider::new(config(&mock, "")).unwrap();

    let attempt = tokio::time::timeout(
        Duration::from_millis(150),
        provider.stream_chat(request("m")),
    )
    .await;
    assert!(
        attempt.is_err(),
        "the server never answers, so the caller gave up"
    );

    wait_until("the server to notice the client left", || {
        mock.disconnects() == 1
    })
    .await;
}
