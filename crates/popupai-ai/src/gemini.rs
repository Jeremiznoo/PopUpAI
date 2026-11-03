//! Google Gemini API: `POST {base}/models/{model}:streamGenerateContent?alt=sse`.
//!
//! The API key goes in the `x-goog-api-key` header, never in the URL query.

use futures_util::future::BoxFuture;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, HeaderName};
use serde_json::{Value, json};
use url::Url;

use crate::decode::{SseDecoder, SseEvent};
use crate::driver::{ChunkParser, Out, build_client, into_stream, read_json, secret_header, send};
use crate::error::{stream_error, truncated};
use crate::provider::{Provider, ProviderConfig};
use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError, Role};

const KEY_HEADER: HeaderName = HeaderName::from_static("x-goog-api-key");

pub struct GeminiProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}

impl GeminiProvider {
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
        Ok(builder.header(KEY_HEADER, secret_header(key.expose())?))
    }

    fn stream_url(&self, model: &str) -> Result<Url, ProviderError> {
        let model = model_name(model)?;
        let mut url = self
            .config
            .endpoint
            .join(&format!("models/{model}:streamGenerateContent"));
        url.query_pairs_mut().append_pair("alt", "sse");
        Ok(url)
    }
}

impl Provider for GeminiProvider {
    fn kind(&self) -> &'static str {
        "gemini"
    }

    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let builder = self
                .client
                .post(self.stream_url(&request.model)?)
                .header(ACCEPT, "text/event-stream")
                .json(&chat_body(&request));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_stream(response, GeminiParser::default()))
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async move {
            let mut url = self.config.endpoint.join("models");
            url.query_pairs_mut().append_pair("pageSize", "1000");
            let response = send(self.authorize(self.client.get(url))?).await?;
            let body = read_json(response).await?;
            Ok(chat_models(&body))
        })
    }
}

/// `models/gemini-2.0-flash` and `gemini-2.0-flash` both mean the same model.
/// The name ends up in a URL path, so it is restricted to plain characters.
fn model_name(model: &str) -> Result<&str, ProviderError> {
    let name = model.strip_prefix("models/").unwrap_or(model);
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(name)
    } else {
        Err(ProviderError::InvalidRequest(
            "a Gemini model name may only contain letters, digits, '.', '_' and '-'".into(),
        ))
    }
}

fn chat_body(request: &ChatRequest) -> Value {
    let contents: Vec<Value> = request
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "model",
            };
            json!({ "role": role, "parts": [{ "text": m.content }] })
        })
        .collect();
    let mut body = json!({ "contents": contents });
    if let Some(system) = &request.system_prompt {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    let mut generation = serde_json::Map::new();
    if let Some(temperature) = request.temperature_json() {
        generation.insert("temperature".into(), json!(temperature));
    }
    if let Some(max_tokens) = request.max_tokens {
        generation.insert("maxOutputTokens".into(), json!(max_tokens));
    }
    if !generation.is_empty() {
        body["generationConfig"] = Value::Object(generation);
    }
    body
}

/// Models that can generate text. Entries without the capability list are kept.
fn chat_models(body: &Value) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = body
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| {
            m.get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_none_or(|methods| methods.iter().any(|x| x == "generateContent"))
        })
        .filter_map(|m| m.get("name").and_then(Value::as_str))
        .map(|name| ModelInfo {
            id: name.strip_prefix("models/").unwrap_or(name).to_owned(),
        })
        .collect();
    models.sort();
    models.dedup();
    models
}

/// Gemini sends no end marker. The stream is complete once a candidate reports a
/// `finishReason`; closing before that is reported as cut off.
#[derive(Default)]
struct GeminiParser {
    sse: SseDecoder,
    done: bool,
}

impl GeminiParser {
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
        if let Some(reason) = value
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
        {
            out.push_back(Err(ProviderError::Stream(format!(
                "the prompt was blocked by the provider ({reason})"
            ))));
            self.done = true;
            return;
        }
        let text: String = value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            // A part marked `thought` is the model's reasoning, not its answer.
            .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect();
        if !text.is_empty() {
            out.push_back(Ok(text));
        }
        if let Some(reason) = value
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
        {
            if matches!(
                reason,
                "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII"
            ) {
                out.push_back(Err(ProviderError::Stream(format!(
                    "the response was stopped by the provider ({reason})"
                ))));
            }
            self.done = true;
        }
    }
}

impl ChunkParser for GeminiParser {
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
    use popupai_security::{Endpoint, HttpPolicy};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gemini-2.0-flash".into(),
            messages: vec![
                ChatMessage::user("hi"),
                ChatMessage::assistant("hello"),
                ChatMessage::user("more"),
            ],
            system_prompt: Some("be brief".into()),
            temperature: Some(1.0),
            max_tokens: Some(100),
        }
    }

    fn provider() -> GeminiProvider {
        let endpoint = Endpoint::parse(
            "https://generativelanguage.googleapis.com/v1beta",
            HttpPolicy::default(),
        )
        .unwrap();
        GeminiProvider::new(ProviderConfig::new(endpoint)).unwrap()
    }

    const STREAM: &str = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}],\"role\":\"model\"},\"index\":0}]}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"totalTokenCount\":9}}\r\n\r\n",
    );

    fn texts(items: Vec<Result<String, ProviderError>>) -> Vec<String> {
        items.into_iter().map(|r| r.unwrap()).collect()
    }

    #[test]
    fn reasoning_parts_are_not_shown_as_the_answer() {
        let stream = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Let me think about it.\",\"thought\":true},{\"text\":\"Answer.\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"more thinking\",\"thought\":true}]},\"finishReason\":\"STOP\"}]}\n\n",
        );
        let items = run_parser(&mut GeminiParser::default(), &[stream.as_bytes()]);
        assert_eq!(texts(items), ["Answer."]);
    }

    #[test]
    fn url_has_the_model_in_the_path_and_no_key_anywhere() {
        let url = provider().stream_url("gemini-2.0-flash").unwrap();
        assert_eq!(
            url.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
        let prefixed = provider().stream_url("models/gemini-2.0-flash").unwrap();
        assert_eq!(prefixed, url);
    }

    #[test]
    fn model_names_that_could_alter_the_url_are_rejected() {
        for bad in ["", "a/b", "a?key=1", "a b", "a:b", "../x", "models/"] {
            assert!(model_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn body_maps_roles_system_prompt_and_generation_settings() {
        let body = chat_body(&request());
        let roles: Vec<_> = body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "model", "user"]);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be brief");
        assert_eq!(body["generationConfig"]["temperature"], 1.0);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 100);
        let bare = chat_body(&ChatRequest {
            temperature: None,
            max_tokens: None,
            system_prompt: None,
            ..request()
        });
        assert!(bare.get("generationConfig").is_none());
        assert!(bare.get("systemInstruction").is_none());
    }

    #[test]
    fn parses_a_typical_stream_and_stops_at_the_finish_reason() {
        assert_eq!(
            texts(run_parser(
                &mut GeminiParser::default(),
                &[STREAM.as_bytes()]
            )),
            ["Hel", "lo"]
        );
        let pieces: Vec<&[u8]> = STREAM.as_bytes().chunks(9).collect();
        assert_eq!(
            texts(run_parser(&mut GeminiParser::default(), &pieces)),
            ["Hel", "lo"]
        );
    }

    #[test]
    fn blocked_prompts_safety_stops_and_cut_streams_are_errors() {
        let blocked = run_parser(
            &mut GeminiParser::default(),
            &[b"data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"}}\n\n"],
        );
        assert!(matches!(&blocked[..], [Err(ProviderError::Stream(m))] if m.contains("blocked")));
        let safety = run_parser(
            &mut GeminiParser::default(),
            &[b"data: {\"candidates\":[{\"finishReason\":\"SAFETY\"}]}\n\n"],
        );
        assert!(matches!(&safety[..], [Err(ProviderError::Stream(_))]));
        let cut = run_parser(
            &mut GeminiParser::default(),
            &[b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"x\"}]}}]}\n\n"],
        );
        assert!(matches!(&cut[..], [Ok(_), Err(ProviderError::Network(_))]));
    }

    #[test]
    fn model_list_keeps_only_text_generation_models() {
        let body = json!({ "models": [
            { "name": "models/gemini-2.0-flash", "supportedGenerationMethods": ["generateContent", "countTokens"] },
            { "name": "models/embedding-001", "supportedGenerationMethods": ["embedContent"] },
            { "name": "models/legacy" },
        ]});
        let ids: Vec<_> = chat_models(&body).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["gemini-2.0-flash", "legacy"]);
    }

    #[tokio::test]
    async fn requires_a_key_before_any_request() {
        assert!(matches!(
            provider().stream_chat(request()).await,
            Err(ProviderError::MissingApiKey)
        ));
    }
}
