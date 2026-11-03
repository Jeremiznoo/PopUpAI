//! ChatGPT subscription access: `POST {base}/responses` on the ChatGPT backend, with the
//! tokens from [`crate::oauth`] and the OpenAI Responses event stream.
//!
//! **Status: written from what the Codex CLI and opencode do. This is not a documented,
//! supported API of OpenAI and its exact contract (headers, accepted models, body fields)
//! could not be checked without an account. Tested against a scripted local server only.**

use futures_util::future::BoxFuture;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderName};
use serde_json::{Value, json};

use crate::agent::{
    AgentEvent, AgentItem, AgentOut, AgentParser, AgentRequest, AgentStream, ToolCall,
    into_agent_stream,
};
use crate::decode::{SseDecoder, SseEvent};
use crate::driver::{ChunkParser, Out, build_client, into_stream, secret_header, send};
use crate::error::stream_error;
use crate::oauth::Tokens;
use crate::provider::{Provider, ProviderConfig};
use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError, Role};

const ACCOUNT_HEADER: HeaderName = HeaderName::from_static("chatgpt-account-id");
const ORIGINATOR_HEADER: HeaderName = HeaderName::from_static("originator");
const BETA_HEADER: HeaderName = HeaderName::from_static("openai-beta");

/// The backend insists on instructions; this is used when the user set none.
const DEFAULT_INSTRUCTIONS: &str = "You are a helpful assistant. Answer concisely.";

pub struct ChatGptProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}

impl ChatGptProvider {
    /// `config.api_key` holds the stored sign-in (see [`Tokens::to_secret`]).
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        Ok(Self {
            client: build_client(config.timeout)?,
            config,
        })
    }

    fn authorize(&self, builder: RequestBuilder) -> Result<RequestBuilder, ProviderError> {
        let secret = self
            .config
            .api_key
            .as_ref()
            .ok_or(ProviderError::MissingApiKey)?;
        let tokens = Tokens::from_secret(secret)?;
        let mut builder = builder
            .header(
                AUTHORIZATION,
                secret_header(&format!("Bearer {}", tokens.access_token))?,
            )
            .header(ORIGINATOR_HEADER, "popupai")
            .header(BETA_HEADER, "responses=experimental");
        if let Some(account) = &tokens.account_id {
            builder = builder.header(ACCOUNT_HEADER, secret_header(account)?);
        }
        Ok(builder)
    }
}

impl Provider for ChatGptProvider {
    fn kind(&self) -> &'static str {
        "chatgpt"
    }

    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            request.validate()?;
            let builder = self
                .client
                .post(self.config.endpoint.join("responses"))
                .header(ACCEPT, "text/event-stream")
                .json(&body(&request));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_stream(response, ResponsesParser::default()))
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn stream_agent(
        &self,
        request: AgentRequest,
    ) -> BoxFuture<'_, Result<AgentStream, ProviderError>> {
        Box::pin(async move {
            if request.model.trim().is_empty() || request.items.is_empty() {
                return Err(ProviderError::InvalidRequest(
                    "a model and a message are needed".into(),
                ));
            }
            let builder = self
                .client
                .post(self.config.endpoint.join("responses"))
                .header(ACCEPT, "text/event-stream")
                .json(&agent_body(&request));
            let response = send(self.authorize(builder)?).await?;
            Ok(into_agent_stream(response, AgentResponsesParser::default()))
        })
    }

    /// The backend has no public model list; models are picked by name.
    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn body(request: &ChatRequest) -> Value {
    let input: Vec<Value> = request
        .messages
        .iter()
        .map(|m| {
            let kind = match m.role {
                Role::User => "input_text",
                Role::Assistant => "output_text",
            };
            json!({
                "type": "message",
                "role": m.role.as_str(),
                "content": [{ "type": kind, "text": m.content }],
            })
        })
        .collect();
    json!({
        "model": request.model,
        "instructions": request.system_prompt.as_deref().unwrap_or(DEFAULT_INSTRUCTIONS),
        "input": input,
        "stream": true,
        "store": false,
    })
}

fn agent_body(request: &AgentRequest) -> Value {
    let input: Vec<Value> = request
        .items
        .iter()
        .map(|item| match item {
            AgentItem::User(text) => json!({
                "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": text }],
            }),
            AgentItem::Assistant(text) => json!({
                "type": "message", "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            }),
            AgentItem::Call(call) => json!({
                "type": "function_call", "call_id": call.call_id,
                "name": call.name, "arguments": call.arguments,
            }),
            AgentItem::Output { call_id, output } => json!({
                "type": "function_call_output", "call_id": call_id, "output": output,
            }),
            AgentItem::Raw(value) => value.clone(),
        })
        .collect();
    let tools: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function", "name": tool.name,
                "description": tool.description, "parameters": tool.parameters,
            })
        })
        .collect();
    json!({
        "model": request.model,
        "instructions": request.instructions.as_deref().unwrap_or(DEFAULT_INSTRUCTIONS),
        "input": input,
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
    })
}

/// Text, tool calls and the items that must be echoed back, from the Responses stream.
#[derive(Default)]
struct AgentResponsesParser {
    sse: SseDecoder,
    done: bool,
}

impl AgentResponsesParser {
    fn handle(&mut self, event: SseEvent, out: &mut AgentOut) {
        let Ok(value) = serde_json::from_str::<Value>(event.data.trim()) else {
            return;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(text) = value.get("delta").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    out.push_back(Ok(AgentEvent::Text(text.to_owned())));
                }
            }
            Some("response.output_item.done") => {
                let Some(item) = value.get("item") else {
                    return;
                };
                match item.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        let text =
                            |key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
                        if let (Some(call_id), Some(name)) = (text("call_id"), text("name")) {
                            out.push_back(Ok(AgentEvent::Call(ToolCall {
                                call_id,
                                name,
                                arguments: text("arguments").unwrap_or_else(|| "{}".into()),
                            })));
                        }
                    }
                    Some("reasoning") => out.push_back(Ok(AgentEvent::Raw(item.clone()))),
                    _ => {}
                }
            }
            Some("response.completed") => self.done = true,
            Some("response.failed" | "response.incomplete") => {
                let message = value
                    .pointer("/response/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the response did not complete");
                out.push_back(Err(ProviderError::Stream(crate::error::error_message(
                    &json!({ "error": { "message": message } }).to_string(),
                ))));
                self.done = true;
            }
            Some("error") => {
                out.push_back(Err(stream_error(&value).unwrap_or_else(|| {
                    ProviderError::Stream("the server reported an error".into())
                })));
                self.done = true;
            }
            _ => {}
        }
    }
}

impl AgentParser for AgentResponsesParser {
    fn feed(&mut self, bytes: &[u8], out: &mut AgentOut) {
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

    fn finish(&mut self, out: &mut AgentOut) {
        for event in self.sse.finish() {
            self.handle(event, out);
        }
        if !self.done {
            out.push_back(Err(crate::error::truncated()));
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

/// Strict about the end: a stream that closes before `response.completed` is a cut
/// connection, never a finished answer.
#[derive(Default)]
struct ResponsesParser {
    sse: SseDecoder,
    done: bool,
}

impl ResponsesParser {
    fn handle(&mut self, event: SseEvent, out: &mut Out) {
        let Ok(value) = serde_json::from_str::<Value>(event.data.trim()) else {
            return; // keep-alives and unknown frames
        };
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(text) = value.get("delta").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    out.push_back(Ok(text.to_owned()));
                }
            }
            Some("response.completed") => self.done = true,
            Some("response.failed" | "response.incomplete") => {
                let message = value
                    .pointer("/response/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the response did not complete");
                out.push_back(Err(ProviderError::Stream(crate::error::error_message(
                    &json!({ "error": { "message": message } }).to_string(),
                ))));
                self.done = true;
            }
            Some("error") => {
                if let Some(err) = stream_error(&value) {
                    out.push_back(Err(err));
                } else {
                    out.push_back(Err(ProviderError::Stream(
                        "the server reported an error".into(),
                    )));
                }
                self.done = true;
            }
            _ => {}
        }
    }
}

impl ChunkParser for ResponsesParser {
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
            out.push_back(Err(crate::error::truncated()));
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
            model: "gpt-6-luna".into(),
            messages: vec![
                ChatMessage::user("Hello"),
                ChatMessage {
                    role: Role::Assistant,
                    content: "Hi".into(),
                },
                ChatMessage::user("And you?"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn the_body_uses_responses_items_and_never_stores_the_conversation() {
        let b = body(&request());
        assert_eq!(b["model"], "gpt-6-luna");
        assert_eq!(b["stream"], true);
        assert_eq!(b["store"], false);
        assert_eq!(b["instructions"], DEFAULT_INSTRUCTIONS);
        assert_eq!(b["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(b["input"][1]["role"], "assistant");
        assert_eq!(b["input"][1]["content"][0]["type"], "output_text");
        let with_system = body(&ChatRequest {
            system_prompt: Some("Be brief.".into()),
            ..request()
        });
        assert_eq!(with_system["instructions"], "Be brief.");
    }

    const STREAM: &str = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
    );

    #[test]
    fn deltas_are_streamed_and_completion_ends_the_answer() {
        let items = run_parser(&mut ResponsesParser::default(), &[STREAM.as_bytes()]);
        let texts: Vec<String> = items.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(texts, ["Hel", "lo"]);
    }

    #[test]
    fn a_stream_cut_before_completion_is_an_error() {
        let cut = &STREAM[..STREAM.rfind("event: response.completed").unwrap()];
        let items = run_parser(&mut ResponsesParser::default(), &[cut.as_bytes()]);
        assert!(matches!(items.last(), Some(Err(ProviderError::Network(_)))));
    }

    #[test]
    fn a_failed_response_reports_its_message() {
        let failed = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"model not available\"}}}\n\n";
        let items = run_parser(&mut ResponsesParser::default(), &[failed.as_bytes()]);
        assert!(
            matches!(&items[..], [Err(ProviderError::Stream(m))] if m.contains("model not available"))
        );
    }

    use crate::agent::{ToolSpec, run_agent_parser};

    #[test]
    fn the_agent_body_carries_tools_and_echoes_calls_and_outputs() {
        let request = AgentRequest {
            model: "gpt-6-luna".into(),
            instructions: Some("Be brief.".into()),
            items: vec![
                AgentItem::User("List my files".into()),
                AgentItem::Raw(json!({"type": "reasoning", "encrypted_content": "abc"})),
                AgentItem::Call(ToolCall {
                    call_id: "c1".into(),
                    name: "run_command".into(),
                    arguments: "{\"command\":\"ls\"}".into(),
                }),
                AgentItem::Output {
                    call_id: "c1".into(),
                    output: "a.txt".into(),
                },
            ],
            tools: vec![ToolSpec {
                name: "run_command".into(),
                description: "Run a command".into(),
                parameters: json!({"type": "object"}),
            }],
        };
        let b = agent_body(&request);
        assert_eq!(b["store"], false);
        assert_eq!(b["parallel_tool_calls"], false);
        assert_eq!(b["tools"][0]["type"], "function");
        assert_eq!(b["tools"][0]["name"], "run_command");
        assert_eq!(b["input"][1]["type"], "reasoning");
        assert_eq!(b["input"][2]["type"], "function_call");
        assert_eq!(b["input"][3]["type"], "function_call_output");
        assert_eq!(b["input"][3]["output"], "a.txt");
    }

    #[test]
    fn the_agent_parser_returns_text_calls_and_reasoning_items() {
        let stream = concat!(
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"reasoning\",\"encrypted_content\":\"abc\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Let me look.\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c1\",\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"ls\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\"}\n\n",
        );
        let events: Vec<AgentEvent> =
            run_agent_parser(&mut AgentResponsesParser::default(), &[stream.as_bytes()])
                .into_iter()
                .map(|e| e.unwrap())
                .collect();
        assert!(matches!(&events[0], AgentEvent::Raw(v) if v["type"] == "reasoning"));
        assert_eq!(events[1], AgentEvent::Text("Let me look.".into()));
        let AgentEvent::Call(call) = &events[2] else {
            panic!("expected a call")
        };
        assert_eq!(
            (call.call_id.as_str(), call.name.as_str()),
            ("c1", "run_command")
        );
        assert_eq!(call.arguments, "{\"command\":\"ls\"}");
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn an_agent_stream_cut_before_completion_is_an_error() {
        let cut = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n";
        let items = run_agent_parser(&mut AgentResponsesParser::default(), &[cut.as_bytes()]);
        assert!(matches!(items.last(), Some(Err(ProviderError::Network(_)))));
    }
}
