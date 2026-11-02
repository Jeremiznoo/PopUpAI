//! Ollama: `POST {base}/api/chat` with newline-delimited JSON streaming. The
//! key is optional (a reverse proxy in front of Ollama may require one).

use futures_util::future::BoxFuture;
use reqwest::RequestBuilder;
use reqwest::header::AUTHORIZATION;
use serde_json::{Value, json};

use crate::decode::LineDecoder;
use crate::driver::{ChunkParser, Out, build_client, into_stream, read_json, secret_header, send};
use crate::error::{stream_error, truncated};
use crate::provider::{Provider, ProviderConfig, collect_ids};
use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError};

pub struct OllamaProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        Ok(Self {
            client: build_client(config.timeout)?,
            config,
        })
    }

    fn authorize(&self, builder: RequestBuilder) -> Result<RequestBuilder, ProviderError> {
        match &self.config.api_key {
            Some(key) => Ok(builder.header(
                AUTHORIZATION,
                secret_header(&format!("Bearer {}", key.expose()))?,
            )),
            None => Ok(builder),
        }
    }
}

impl Provider for OllamaProvider {
    fn kind(&self) -> &'static str {
        "ollama"
    }

    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let builder = self
                .client
                .post(self.config.endpoint.join("api/chat"))
                .json(&chat_body(&request));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_stream(response, OllamaParser::default()))
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let builder = self.client.get(self.config.endpoint.join("api/tags"));
            let response = send(self.authorize(builder)?).await?;
            let body = read_json(response).await?;
            Ok(collect_ids(body.get("models"), "name"))
        })
    }
}

fn chat_body(request: &ChatRequest) -> Value {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system) = &request.system_prompt {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.push(json!({ "role": message.role.as_str(), "content": message.content }));
    }
    let mut body = json!({ "model": request.model, "messages": messages, "stream": true });
    let mut options = serde_json::Map::new();
    if let Some(temperature) = request.temperature_json() {
        options.insert("temperature".into(), json!(temperature));
    }
    if let Some(max_tokens) = request.max_tokens {
        options.insert("num_predict".into(), json!(max_tokens));
    }
    if !options.is_empty() {
        body["options"] = Value::Object(options);
    }
    body
}

/// Strict about the end: Ollama always sends a final `"done": true` line.
#[derive(Default)]
struct OllamaParser {
    lines: LineDecoder,
    done: bool,
}

impl OllamaParser {
    fn handle(&mut self, line: &str, out: &mut Out) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            out.push_back(Err(ProviderError::Decode(
                "a streamed line was not valid JSON".into(),
            )));
            self.done = true;
            return;
        };
        if let Some(err) = stream_error(&value) {
            out.push_back(Err(err));
            self.done = true;
            return;
        }
        if let Some(text) = value.pointer("/message/content").and_then(Value::as_str)
            && !text.is_empty()
        {
            out.push_back(Ok(text.to_owned()));
        }
        if value.get("done").and_then(Value::as_bool) == Some(true) {
            self.done = true;
        }
    }
}

impl ChunkParser for OllamaParser {
    fn feed(&mut self, bytes: &[u8], out: &mut Out) {
        match self.lines.push(bytes) {
            Ok(lines) => {
                for line in lines {
                    if self.done {
                        break;
                    }
                    self.handle(&line, out);
                }
            }
            Err(err) => {
                out.push_back(Err(err));
                self.done = true;
            }
        }
    }

    fn finish(&mut self, out: &mut Out) {
        // Ollama ends every line with a newline, so an unterminated tail that is
        // not valid JSON means the connection was cut, not that the data is bad.
        if let Some(line) = self.lines.finish()
            && serde_json::from_str::<Value>(&line).is_ok()
        {
            self.handle(&line, out);
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
            model: "llama3.2:latest".into(),
            messages: vec![ChatMessage::user("hi")],
            system_prompt: Some("be brief".into()),
            temperature: Some(0.2),
            max_tokens: Some(64),
        }
    }

    const STREAM: &str = concat!(
        "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"done\":false}\n",
        "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n",
        "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
    );

    fn texts(items: Vec<Result<String, ProviderError>>) -> Vec<String> {
        items.into_iter().map(|r| r.unwrap()).collect()
    }

    #[test]
    fn body_uses_ollama_option_names_and_omits_empty_options() {
        let body = chat_body(&request());
        assert_eq!(body["options"]["temperature"], 0.2);
        assert_eq!(body["options"]["num_predict"], 64);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["stream"], true);
        let bare = chat_body(&ChatRequest {
            temperature: None,
            max_tokens: None,
            ..request()
        });
        assert!(bare.get("options").is_none());
    }

    #[test]
    fn parses_ndjson_however_it_is_split() {
        assert_eq!(
            texts(run_parser(
                &mut OllamaParser::default(),
                &[STREAM.as_bytes()]
            )),
            ["Hel", "lo"]
        );
        let pieces: Vec<&[u8]> = STREAM.as_bytes().chunks(11).collect();
        assert_eq!(
            texts(run_parser(&mut OllamaParser::default(), &pieces)),
            ["Hel", "lo"]
        );
    }

    #[test]
    fn errors_and_cut_streams_are_reported() {
        let err = run_parser(
            &mut OllamaParser::default(),
            &[b"{\"error\":\"model 'x' not found\"}\n"],
        );
        assert!(matches!(&err[..], [Err(ProviderError::Stream(m))] if m.contains("not found")));
        // The connection closes before the final `"done": true` line.
        let last_line = STREAM.trim_end().rfind('\n').unwrap() + 1;
        let items = run_parser(
            &mut OllamaParser::default(),
            &[&STREAM.as_bytes()[..last_line]],
        );
        assert!(matches!(items.last(), Some(Err(ProviderError::Network(_)))));
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn a_line_cut_in_half_counts_as_a_cut_connection() {
        let items = run_parser(
            &mut OllamaParser::default(),
            &[b"{\"message\":{\"content\":\"a\"},\"done\":false}\n{\"message\":{\"con"],
        );
        assert!(matches!(&items[..], [Ok(a), Err(ProviderError::Network(_))] if a == "a"));
    }

    #[test]
    fn a_final_line_without_a_newline_is_still_honoured() {
        let items = run_parser(
            &mut OllamaParser::default(),
            &[b"{\"message\":{\"content\":\"a\"},\"done\":true}"],
        );
        assert!(matches!(&items[..], [Ok(a)] if a == "a"));
    }
}
