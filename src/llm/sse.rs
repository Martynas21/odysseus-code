/// Parse one SSE line. Returns the payload of a `data:` line (one optional
/// leading space stripped), or `None` for comments, `event:`/`id:` lines and
/// blanks.
pub fn parse_sse_line(line: &str) -> Option<String> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.strip_prefix("data:")?;
    Some(rest.strip_prefix(' ').unwrap_or(rest).to_string())
}

/// Line-buffered SSE reader. `feed` may be called with arbitrary byte splits;
/// it returns the `data:` payloads for every line completed so far, holding any
/// trailing partial line (raw bytes, safe across UTF-8 boundaries) until the
/// next call.
#[derive(Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            if let Some(data) = parse_sse_line(&line) {
                out.push(data);
            }
        }
        out
    }

    pub fn flush(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let line = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).into_owned();
        parse_sse_line(&line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_line_extracts_data_payload() {
        assert_eq!(parse_sse_line("data: hello"), Some("hello".to_string()));
        assert_eq!(parse_sse_line("data:hello"), Some("hello".to_string()));
        assert_eq!(parse_sse_line(": comment"), None);
        assert_eq!(parse_sse_line("event: foo"), None);
        assert_eq!(parse_sse_line(""), None);
    }

    #[test]
    fn decoder_emits_complete_data_lines() {
        let mut d = SseDecoder::new();
        let out = d.feed(b"data: a\n\ndata: b\n\n");
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn decoder_buffers_across_chunk_boundaries() {
        let mut d = SseDecoder::new();
        // A data line split mid-JSON across three feeds.
        assert_eq!(d.feed(b"data: {\"x\":"), Vec::<String>::new());
        assert_eq!(d.feed(b"1}"), Vec::<String>::new());
        assert_eq!(d.feed(b"\n"), vec![r#"{"x":1}"#.to_string()]);
    }

    #[test]
    fn decoder_handles_crlf_and_done() {
        let mut d = SseDecoder::new();
        let out = d.feed(b"data: chunk\r\ndata: [DONE]\r\n");
        assert_eq!(out, vec!["chunk".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn flush_emits_unterminated_final_line() {
        let mut d = SseDecoder::new();
        assert_eq!(d.feed(b"data: {\"x\":1}"), Vec::<String>::new());
        assert_eq!(d.flush(), Some(r#"{"x":1}"#.to_string()));
        assert_eq!(d.flush(), None);
    }

    #[test]
    fn flush_is_none_when_buffer_empty() {
        let mut d = SseDecoder::new();
        assert_eq!(d.feed(b"data: a\n"), vec!["a".to_string()]);
        assert_eq!(d.flush(), None);
    }

    #[test]
    fn decoder_splits_multibyte_utf8_across_chunks() {
        // The whole reason for buffering raw bytes: a codepoint may straddle a
        // chunk boundary. 'é' is 0xC3 0xA9 — feed it split down the middle.
        let mut d = SseDecoder::new();
        let bytes = "data: é\n".as_bytes();
        let split = bytes.len() - 2; // between the two bytes of 'é'
        assert!(d.feed(&bytes[..split]).is_empty());
        assert_eq!(d.feed(&bytes[split..]), vec!["é".to_string()]);
    }
}
