//! Incremental decoders for the two streaming formats the providers use:
//! Server-Sent Events (OpenAI, Anthropic, Gemini) and newline-delimited JSON
//! (Ollama). Network chunks can end anywhere, so both keep a buffer and only
//! decode complete lines.

use crate::ProviderError;

/// A single line or event larger than this is treated as a protocol error
/// instead of growing memory without bound.
const MAX_BUFFER: usize = 8 * 1024 * 1024;

fn too_large() -> ProviderError {
    ProviderError::Decode("a streamed line exceeded the size limit".into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ProviderError> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((end, skip)) = line_end(&self.buf) {
            let line = String::from_utf8_lossy(&self.buf[..end]).into_owned();
            self.buf.drain(..end + skip);
            self.process_line(&line, &mut events);
        }
        if self.buf.len() > MAX_BUFFER {
            return Err(too_large());
        }
        Ok(events)
    }

    /// Flush at end of stream. A final event without its blank line is still
    /// delivered, because some servers close the connection right after it.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&self.buf).into_owned();
            self.buf.clear();
            self.process_line(line.trim_end_matches('\r'), &mut events);
        }
        self.dispatch(&mut events);
        events
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            self.dispatch(events);
            return;
        }
        if line.starts_with(':') {
            return; // comment / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_owned()),
            "event" => self.event = Some(value.to_owned()),
            _ => {} // id, retry and unknown fields are not needed
        }
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        if self.data.is_empty() {
            self.event = None;
        } else {
            events.push(SseEvent {
                event: self.event.take(),
                data: self.data.join("\n"),
            });
            self.data.clear();
        }
    }
}

/// End of the first line in `buf`: `(content_len, terminator_len)`. A lone
/// trailing `\r` waits for the next chunk, because it may be half of `\r\n`.
fn line_end(buf: &[u8]) -> Option<(usize, usize)> {
    for (i, &b) in buf.iter().enumerate() {
        match b {
            b'\n' => return Some((i, 1)),
            b'\r' => {
                return match buf.get(i + 1) {
                    Some(b'\n') => Some((i, 2)),
                    Some(_) => Some((i, 1)),
                    None => None,
                };
            }
            _ => {}
        }
    }
    None
}

/// Newline-delimited JSON: one document per line.
#[derive(Debug, Default)]
pub struct LineDecoder {
    buf: Vec<u8>,
}

impl LineDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ProviderError> {
        self.buf.extend_from_slice(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw[..pos]).trim().to_owned();
            if !line.is_empty() {
                lines.push(line);
            }
        }
        if self.buf.len() > MAX_BUFFER {
            return Err(too_large());
        }
        Ok(lines)
    }

    pub fn finish(&mut self) -> Option<String> {
        let line = String::from_utf8_lossy(&self.buf).trim().to_owned();
        self.buf.clear();
        (!line.is_empty()).then_some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut dec = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(dec.push(chunk).unwrap());
        }
        events.extend(dec.finish());
        events
    }

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            event: event.map(str::to_owned),
            data: data.to_owned(),
        }
    }

    #[test]
    fn decodes_events_with_names_and_multiline_data() {
        let events = decode_all(&[b"event: greet\ndata: hello\ndata: world\n\ndata: second\n\n"]);
        assert_eq!(
            events,
            [ev(Some("greet"), "hello\nworld"), ev(None, "second")]
        );
    }

    #[test]
    fn handles_crlf_cr_and_comments() {
        let events = decode_all(&[b": keep-alive\r\ndata: a\r\n\r\ndata: b\r\rdata: c\n\n"]);
        assert_eq!(events, [ev(None, "a"), ev(None, "b"), ev(None, "c")]);
    }

    #[test]
    fn result_is_identical_however_the_bytes_are_split() {
        let payload = "event: x\ndata: {\"t\":\"héllo 🌍\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
        let whole = decode_all(&[payload]);
        let bytewise: Vec<&[u8]> = payload.chunks(1).collect();
        assert_eq!(decode_all(&bytewise), whole);
        assert_eq!(whole.len(), 2);
        assert_eq!(whole[0].data, "{\"t\":\"héllo 🌍\"}");
    }

    #[test]
    fn a_split_crlf_does_not_produce_a_spurious_event() {
        let events = decode_all(&[b"data: a\r", b"\n\r", b"\ndata: b\n\n"]);
        assert_eq!(events, [ev(None, "a"), ev(None, "b")]);
    }

    #[test]
    fn finish_delivers_an_unterminated_last_event() {
        assert_eq!(decode_all(&[b"data: tail"]), [ev(None, "tail")]);
        assert_eq!(decode_all(&[b"data: tail\n"]), [ev(None, "tail")]);
    }

    #[test]
    fn ignores_events_without_data_and_unknown_fields() {
        let events = decode_all(&[b"event: ping\n\nid: 7\nretry: 100\ndata: x\n\n"]);
        assert_eq!(events, [ev(None, "x")]);
    }

    #[test]
    fn oversized_lines_are_rejected() {
        let mut dec = SseDecoder::default();
        let big = vec![b'a'; MAX_BUFFER + 1];
        assert!(matches!(dec.push(&big), Err(ProviderError::Decode(_))));
        let mut lines = LineDecoder::default();
        assert!(matches!(lines.push(&big), Err(ProviderError::Decode(_))));
    }

    #[test]
    fn line_decoder_splits_trims_and_flushes() {
        let mut dec = LineDecoder::default();
        assert_eq!(dec.push(b"{\"a\":1}\r\n{\"b\"").unwrap(), ["{\"a\":1}"]);
        assert_eq!(dec.push(b":2}\n\n").unwrap(), ["{\"b\":2}"]);
        assert_eq!(dec.push(b"tail").unwrap(), Vec::<String>::new());
        assert_eq!(dec.finish().as_deref(), Some("tail"));
        assert_eq!(dec.finish(), None);
    }
}
