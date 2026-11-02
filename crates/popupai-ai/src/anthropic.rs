//! Anthropic Messages API: `POST {base}/messages` with SSE streaming.

use futures_util::future::BoxFuture;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::decode::{SseDecoder, SseEvent};
use crate::driver::{ChunkParser, Out, build_client, into_stream, read_json, secret_header, send};
use crate::error::{stream_error, truncated};
use crate::provider::{Provider, ProviderConfig, collect_ids};
use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError};

const API_VERSION: &str = "2023-06-01";
/// The API requires an output limit; used when the user has not set one.
const DEFAULT_MAX_TOKENS: u32 = 4096;
const KEY_HEADER: HeaderName = HeaderName::from_static("x-api-key");
const VERSION_HEADER: HeaderName = HeaderName::from_static("anthropic-version");

pub struct AnthropicProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        Ok(Self {
            client: build_client(config.timeout)?,
            config,
        })
    }

    fn authorize(&self, builder: RequestBuilder) -> Result<RequestBuilder, ProviderError> {
        let key = self
            .config
            .api_key
            .as_ref()
            .ok_or(ProviderError::MissingApiKey)?;
        Ok(builder
            .header(KEY_HEADER, secret_header(key.expose())?)
            .header(VERSION_HEADER, HeaderValue::from_static(API_VERSION)))
    }
}

impl Provider for AnthropicProvider {
    fn kind(&self) -> &'static str {
        "anthropic"
    }

    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let builder = self
                .client
                .post(self.config.endpoint.join("messages"))
                .header(ACCEPT, "text/event-stream")
                .json(&chat_body(&request));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_stream(response, AnthropicParser::default()))
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let mut url = self.config.endpoint.join("models");
            url.query_pairs_mut().append_pair("limit", "1000");
            let response = send(self.authorize(self.client.get(url))?).await?;
            let body = read_json(response).await?;
            Ok(collect_ids(body.get("data"), "id"))
        })
    }
}

fn chat_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| json!({ "role": m.role.as_str(), "content": m.content }))
        .collect();
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "messages": messages,
        "stream": true,
    });
    if let Some(system) = &request.system_prompt {
        body["system"] = json!(system);
    }
    if let Some(temperature) = request.temperature_json() {
        body["temperature"] = json!(temperature);
    }
    body
}

/// Strict about the end: a stream that closes before `message_stop` is reported
/// as cut off, so a dropped connection is never shown as a finished answer.
#[derive(Default)]
struct AnthropicParser {
    sse: SseDecoder,
    done: bool,
}

impl AnthropicParser {
    fn handle(&mut self, event: SseEvent, out: &mut Out) {
        let Ok(value) = serde_json::from_str::<Value>(event.data.trim()) else {
            out.push_back(Err(ProviderError::Decode(
                "a streamed event was not valid JSON".into(),
            )));
            self.done = true;
            return;
        };
        if let Some(err) = stream_error(&value) {
            out.push_back(Err(err));
            self.done = true;
            return;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    out.push_back(Ok(text.to_owned()));
                }
            }
            Some("message_stop") => self.done = true,
            _ => {} // message_start, ping, content_block_start/stop, message_delta
        }
    }
}

impl ChunkParser for AnthropicParser {
    fn feed(&mut self, bytes: &[u8], out: &mut Out) {
        match self.sse.push(bytes) {
            Ok(events) => {
                for event in events {
                    if self.done {
                        break;
                    }
                    self.handle(event, out);
                }
            }
            Err(err) => {
                out.push_back(Err(err));
                self.done = true;
            }
        }
    }

    fn finish(&mut self, out: &mut Out) {
        for event in self.sse.finish() {
            self.handle(event, out);
        }
        if !self.done {
            out.push_back(Err(truncated()));
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChatMessage;
    use crate::driver::run_parser;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "claude-sonnet-5-5".into(),
            messages: vec![ChatMessage::user("hi")],
            system_prompt: Some("be brief".into()),
            temperature: Some(0.5),
            max_tokens: None,
        }
    }

    const STREAM: &str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );

    fn texts(items: Vec<Result<String, ProviderError>>) -> Vec<String> {
        items.into_iter().map(|r| r.unwrap()).collect()
    }

    #[test]
    fn body_has_required_output_limit_and_top_level_system_prompt() {
        let body = chat_body(&request());
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        let explicit = chat_body(&ChatRequest {
            max_tokens: Some(50),
            system_prompt: None,
            ..request()
        });
        assert_eq!(explicit["max_tokens"], 50);
        assert!(explicit.get("system").is_none());
    }

    #[test]
    fn parses_a_typical_stream_however_it_is_split() {
        assert_eq!(
            texts(run_parser(
                &mut AnthropicParser::default(),
                &[STREAM.as_bytes()]
            )),
            ["Hel", "lo"]
        );
        let pieces: Vec<&[u8]> = STREAM.as_bytes().chunks(5).collect();
        assert_eq!(
            texts(run_parser(&mut AnthropicParser::default(), &pieces)),
            ["Hel", "lo"]
        );
    }

    #[test]
    fn a_stream_cut_before_message_stop_is_reported() {
        let cut = &STREAM[..STREAM.find("event: message_stop").unwrap()];
        let items = run_parser(&mut AnthropicParser::default(), &[cut.as_bytes()]);
        assert_eq!(items.len(), 3);
        assert!(matches!(items.last(), Some(Err(ProviderError::Network(_)))));
    }

    #[test]
    fn in_stream_errors_keep_the_text_received_so_far() {
        let payload = concat!(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Par\"}}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        );
        let items = run_parser(&mut AnthropicParser::default(), &[payload.as_bytes()]);
        assert!(matches!(&items[0], Ok(t) if t == "Par"));
        assert!(matches!(&items[1], Err(ProviderError::Stream(m)) if m == "Overloaded"));
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn requires_a_key_before_any_request() {
        let endpoint =
            popupai_security::Endpoint::parse("https://api.anthropic.com/v1", Default::default())
                .unwrap();
        let provider = AnthropicProvider::new(ProviderConfig::new(endpoint)).unwrap();
        assert!(matches!(
            provider.stream_chat(request()).await,
            Err(ProviderError::MissingApiKey)
        ));
        assert!(matches!(
            provider.list_models().await,
            Err(ProviderError::MissingApiKey)
        ));
    }
}
