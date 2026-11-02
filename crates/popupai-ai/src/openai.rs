//! OpenAI and OpenAI-compatible endpoints (LM Studio, LocalAI, vLLM,
//! OpenRouter, and so on): `POST {base}/chat/completions` with SSE streaming.

use futures_util::future::BoxFuture;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde_json::{Value, json};

use crate::decode::{SseDecoder, SseEvent};
use crate::driver::{ChunkParser, Out, build_client, into_stream, read_json, secret_header, send};
use crate::error::stream_error;
use crate::provider::{Provider, ProviderConfig, collect_ids};
use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiFlavor {
    /// api.openai.com: a key is required and the output limit is sent as
    /// `max_completion_tokens`.
    OpenAi,
    /// Any other compatible server: the key is optional and the limit is sent
    /// as `max_tokens`, which is what most of them expect.
    Compatible,
}

pub struct OpenAiProvider {
    client: reqwest::Client,
    config: ProviderConfig,
    flavor: OpenAiFlavor,
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig, flavor: OpenAiFlavor) -> Result<Self, ProviderError> {
        Ok(Self {
            client: build_client(config.timeout)?,
            config,
            flavor,
        })
    }

    fn authorize(&self, builder: RequestBuilder) -> Result<RequestBuilder, ProviderError> {
        match &self.config.api_key {
            Some(key) => {
                let value = secret_header(&format!("Bearer {}", key.expose()))?;
                Ok(builder.header(AUTHORIZATION, value))
            }
            None if self.flavor == OpenAiFlavor::OpenAi => Err(ProviderError::MissingApiKey),
            None => Ok(builder),
        }
    }
}

impl Provider for OpenAiProvider {
    fn kind(&self) -> &'static str {
        match self.flavor {
            OpenAiFlavor::OpenAi => "openai",
            OpenAiFlavor::Compatible => "openai-compatible",
        }
    }

    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let builder = self
                .client
                .post(self.config.endpoint.join("chat/completions"))
                .header(ACCEPT, "text/event-stream")
                .json(&chat_body(&request, self.flavor));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_stream(response, OpenAiParser::default()))
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let builder = self.client.get(self.config.endpoint.join("models"));
            let response = send(self.authorize(builder)?).await?;
            let body = read_json(response).await?;
            Ok(collect_ids(body.get("data"), "id"))
        })
    }
}

fn chat_body(request: &ChatRequest, flavor: OpenAiFlavor) -> Value {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system) = &request.system_prompt {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.push(json!({ "role": message.role.as_str(), "content": message.content }));
    }
    let mut body = json!({ "model": request.model, "messages": messages, "stream": true });
    if let Some(temperature) = request.temperature_json() {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        let field = match flavor {
            OpenAiFlavor::OpenAi => "max_completion_tokens",
            OpenAiFlavor::Compatible => "max_tokens",
        };
        body[field] = json!(max_tokens);
    }
    body
}

/// Lenient about the end of the stream: some compatible servers close the
/// connection without sending `[DONE]`.
#[derive(Default)]
struct OpenAiParser {
    sse: SseDecoder,
    done: bool,
}

impl OpenAiParser {
    fn handle(&mut self, event: SseEvent, out: &mut Out) {
        let data = event.data.trim();
        if data == "[DONE]" {
            self.done = true;
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
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
        if let Some(text) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            && !text.is_empty()
        {
            out.push_back(Ok(text.to_owned()));
        }
    }
}

impl ChunkParser for OpenAiParser {
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
            model: "gpt-4o-mini".into(),
            messages: vec![
                ChatMessage::user("hi"),
                ChatMessage::assistant("hello"),
                ChatMessage::user("again"),
            ],
            system_prompt: Some("be brief".into()),
            temperature: Some(0.7),
            max_tokens: Some(256),
        }
    }

    fn texts(items: Vec<Result<String, ProviderError>>) -> Vec<String> {
        items.into_iter().map(|r| r.unwrap()).collect()
    }

    const STREAM: &str = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hel\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    #[test]
    fn body_puts_the_system_prompt_first_and_uses_flavor_specific_limits() {
        let body = chat_body(&request(), OpenAiFlavor::OpenAi);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["max_completion_tokens"], 256);
        assert!(body.get("max_tokens").is_none());
        let roles: Vec<_> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);

        let compat = chat_body(&request(), OpenAiFlavor::Compatible);
        assert_eq!(compat["max_tokens"], 256);
        assert!(compat.get("max_completion_tokens").is_none());
    }

    #[test]
    fn optional_parameters_are_omitted_when_unset() {
        let req = ChatRequest {
            temperature: None,
            max_tokens: None,
            system_prompt: None,
            ..request()
        };
        let body = chat_body(&req, OpenAiFlavor::Compatible);
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn parses_a_typical_stream_however_it_is_split() {
        let whole = texts(run_parser(
            &mut OpenAiParser::default(),
            &[STREAM.as_bytes()],
        ));
        assert_eq!(whole, ["Hel", "lo"]);
        let pieces: Vec<&[u8]> = STREAM.as_bytes().chunks(7).collect();
        assert_eq!(
            texts(run_parser(&mut OpenAiParser::default(), &pieces)),
            ["Hel", "lo"]
        );
    }

    #[test]
    fn stops_at_done_and_ignores_anything_after_it() {
        let payload =
            format!("{STREAM}data: {{\"choices\":[{{\"delta\":{{\"content\":\"late\"}}}}]}}\n\n");
        assert_eq!(
            texts(run_parser(
                &mut OpenAiParser::default(),
                &[payload.as_bytes()]
            )),
            ["Hel", "lo"]
        );
    }

    #[test]
    fn a_missing_done_marker_is_tolerated() {
        let partial = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n";
        assert_eq!(
            texts(run_parser(
                &mut OpenAiParser::default(),
                &[partial.as_bytes()]
            )),
            ["ok"]
        );
    }

    #[test]
    fn provider_errors_and_garbage_end_the_stream_with_an_error() {
        let err = run_parser(
            &mut OpenAiParser::default(),
            &[b"data: {\"error\":{\"message\":\"quota exceeded\"}}\n\n"],
        );
        assert!(matches!(&err[..], [Err(ProviderError::Stream(m))] if m == "quota exceeded"));
        let junk = run_parser(&mut OpenAiParser::default(), &[b"data: not json\n\n"]);
        assert!(matches!(&junk[..], [Err(ProviderError::Decode(_))]));
    }

    #[tokio::test]
    async fn the_real_openai_endpoint_requires_a_key_before_any_request() {
        let endpoint =
            popupai_security::Endpoint::parse("https://api.openai.com/v1", Default::default())
                .unwrap();
        let provider =
            OpenAiProvider::new(ProviderConfig::new(endpoint), OpenAiFlavor::OpenAi).unwrap();
        let result = provider.stream_chat(request()).await;
        assert!(matches!(result, Err(ProviderError::MissingApiKey)));
    }
}
