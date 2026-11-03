//! Tool use: a conversation in which the model may ask for a tool to be run, and is given
//! the result. Only providers that implement [`crate::Provider::stream_agent`] support it.

use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;

use futures_util::{Stream, StreamExt, stream};
use reqwest::Response;
use serde_json::Value;

use crate::ProviderError;
use crate::error::from_reqwest;

/// A tool the model is allowed to call, described as JSON Schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// The model asks for `name` to be run with `arguments` (a JSON object, as text).
#[derive(Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Arguments can hold what the user asked about, so `Debug` shows sizes only.
impl fmt::Debug for ToolCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ToolCall({}, {} bytes)", self.name, self.arguments.len())
    }
}

/// One step of a conversation that uses tools.
#[derive(Clone, PartialEq)]
pub enum AgentItem {
    User(String),
    Assistant(String),
    Call(ToolCall),
    Output {
        call_id: String,
        output: String,
    },
    /// An item the provider sent that must be sent back unchanged to continue (for example
    /// encrypted reasoning).
    Raw(Value),
}

impl fmt::Debug for AgentItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(t) => write!(f, "User({} bytes)", t.len()),
            Self::Assistant(t) => write!(f, "Assistant({} bytes)", t.len()),
            Self::Call(c) => write!(f, "{c:?}"),
            Self::Output { output, .. } => write!(f, "Output({} bytes)", output.len()),
            Self::Raw(_) => f.write_str("Raw"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub model: String,
    pub instructions: Option<String>,
    pub items: Vec<AgentItem>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    Text(String),
    Call(ToolCall),
    Raw(Value),
}

pub type AgentStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, ProviderError>> + Send>>;

pub(crate) type AgentOut = VecDeque<Result<AgentEvent, ProviderError>>;

/// Turns response bytes into [`AgentEvent`]s for one provider's wire format.
pub(crate) trait AgentParser: Send {
    fn feed(&mut self, bytes: &[u8], out: &mut AgentOut);
    fn finish(&mut self, out: &mut AgentOut);
    fn is_done(&self) -> bool;
}

struct Pump<P> {
    bytes: Pin<Box<dyn Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    parser: P,
    pending: AgentOut,
    finished: bool,
}

/// Same contract as the text stream: the first error ends the stream, and dropping it
/// closes the connection.
pub(crate) fn into_agent_stream<P: AgentParser + 'static>(
    response: Response,
    parser: P,
) -> AgentStream {
    let pump = Pump {
        bytes: Box::pin(response.bytes_stream()),
        parser,
        pending: VecDeque::new(),
        finished: false,
    };
    Box::pin(stream::unfold(pump, |mut p| async move {
        loop {
            if let Some(item) = p.pending.pop_front() {
                if item.is_err() {
                    p.pending.clear();
                    p.finished = true;
                }
                return Some((item, p));
            }
            if p.finished {
                return None;
            }
            if p.parser.is_done() {
                p.finished = true;
                continue;
            }
            match p.bytes.next().await {
                Some(Ok(chunk)) => p.parser.feed(&chunk, &mut p.pending),
                Some(Err(err)) => p.pending.push_back(Err(from_reqwest(err))),
                None => {
                    p.parser.finish(&mut p.pending);
                    p.finished = true;
                }
            }
        }
    }))
}

#[cfg(test)]
pub(crate) fn run_agent_parser(
    parser: &mut impl AgentParser,
    chunks: &[&[u8]],
) -> Vec<Result<AgentEvent, ProviderError>> {
    let mut out = AgentOut::new();
    for chunk in chunks {
        if parser.is_done() {
            break;
        }
        parser.feed(chunk, &mut out);
    }
    if !parser.is_done() {
        parser.finish(&mut out);
    }
    out.into_iter().collect()
}
