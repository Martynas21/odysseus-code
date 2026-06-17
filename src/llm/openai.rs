use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};
use serde::Deserialize;

use crate::config::Config;
use crate::llm::sse::SseDecoder;
use crate::llm::{ChatRequest, Provider, ProviderError, StreamEvent};

const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    /// Chain-of-thought from reasoning models. LM Studio uses
    /// `reasoning_content`; some servers use `reasoning`.
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

#[derive(Deserialize)]
struct ToolCallChunk {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FnChunk>,
}

#[derive(Deserialize)]
struct FnChunk {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// Parse one non-`[DONE]` SSE data payload into zero or more events.
pub(crate) fn parse_chunk(data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
    let chunk: ChatChunk = serde_json::from_str(data)
        .map_err(|e| ProviderError::BadStream(format!("{e} — payload: {data}")))?;
    let mut events = Vec::new();
    for choice in chunk.choices {
        if let Some(reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            events.push(StreamEvent::ReasoningDelta(reasoning));
        }
        if let Some(text) = choice.delta.content
            && !text.is_empty()
        {
            events.push(StreamEvent::TextDelta(text));
        }
        for call in choice.delta.tool_calls.unwrap_or_default() {
            let (name, arguments) = match call.function {
                Some(f) => (f.name, f.arguments.unwrap_or_default()),
                None => (None, String::new()),
            };
            events.push(StreamEvent::ToolCallDelta {
                index: call.index,
                id: call.id,
                name,
                arguments,
            });
        }
    }
    if let Some(u) = chunk.usage {
        events.push(StreamEvent::Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        });
    }
    Ok(events)
}

pub struct OpenAiProvider {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(CHAT_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .expect("building HTTP client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub fn from_config(cfg: &Config) -> Self {
        Self::new(&cfg.base_url, &cfg.api_key)
    }

    /// Open the SSE response, retrying once on 429/5xx at open time only.
    async fn open(&self, body: &serde_json::Value) -> Result<reqwest::Response, ProviderError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        for attempt in 0..2 {
            let mut rb = self
                .http
                .post(&url)
                .json(body)
                .header("Accept", "text/event-stream");
            if !self.api_key.is_empty() {
                rb = rb.header("Authorization", format!("Bearer {}", self.api_key));
            }
            let resp = rb.send().await.map_err(|source| ProviderError::Network {
                url: url.clone(),
                source,
            })?;
            let status = resp.status().as_u16();
            match status {
                200..=299 => return Ok(resp),
                401 => return Err(ProviderError::Unauthorized),
                429 | 500..=599 if attempt == 0 => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                429 => return Err(ProviderError::RateLimited),
                code => {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ProviderError::Http { status: code, body });
                }
            }
        }
        Err(ProviderError::RateLimited)
    }
}

/// Streaming state threaded through `stream::unfold`.
struct StreamState {
    bytes: BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    decoder: SseDecoder,
    queue: std::collections::VecDeque<Result<StreamEvent, ProviderError>>,
    finished: bool,
    /// Endpoint URL, kept so a mid-stream transport error reports where it failed.
    url: String,
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        let resp = self.open(&req.to_body()).await?;
        let state = StreamState {
            bytes: resp.bytes_stream().boxed(),
            decoder: SseDecoder::new(),
            queue: std::collections::VecDeque::new(),
            finished: false,
            url: format!("{}/v1/chat/completions", self.base_url),
        };
        let stream = stream::unfold(state, |mut st| async move {
            loop {
                if let Some(item) = st.queue.pop_front() {
                    return Some((item, st));
                }
                if st.finished {
                    return None;
                }
                match st.bytes.next().await {
                    Some(Ok(chunk)) => {
                        for data in st.decoder.feed(&chunk) {
                            if data == "[DONE]" {
                                st.queue.push_back(Ok(StreamEvent::Done));
                                st.finished = true;
                                break;
                            }
                            match parse_chunk(&data) {
                                Ok(events) => st.queue.extend(events.into_iter().map(Ok)),
                                Err(e) => {
                                    st.queue.push_back(Err(e));
                                    st.finished = true;
                                    break;
                                }
                            }
                        }
                    }
                    Some(Err(source)) => {
                        st.queue.push_back(Err(ProviderError::Network {
                            url: st.url.clone(),
                            source,
                        }));
                        st.finished = true;
                    }
                    None => {
                        // Stream ended without [DONE]; synthesize Done.
                        st.queue.push_back(Ok(StreamEvent::Done));
                        st.finished = true;
                    }
                }
            }
        });
        Ok(stream.boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::message::ChatMessage;
    use crate::llm::{ChatRequest, Provider, StreamEvent};
    use futures_util::StreamExt;

    fn req() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("hi")],
            tools: vec![],
            temperature: 0.0,
            max_tokens: 16,
            think: true,
        }
    }

    #[test]
    fn parse_chunk_reasoning_delta() {
        let events = parse_chunk(r#"{"choices":[{"delta":{"reasoning_content":"Hmm"}}]}"#).unwrap();
        assert_eq!(events, vec![StreamEvent::ReasoningDelta("Hmm".into())]);
    }

    #[tokio::test]
    async fn chat_stream_yields_text_then_done() {
        let mut server = mockito::Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                    data: [DONE]\n\n";
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let provider = OpenAiProvider::new(server.url(), "");
        let mut stream = provider.chat_stream(req()).await.unwrap();
        let mut events = Vec::new();
        while let Some(ev) = stream.next().await {
            events.push(ev.unwrap());
        }
        assert_eq!(
            events.first(),
            Some(&StreamEvent::TextDelta("Hello".into()))
        );
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[tokio::test]
    async fn chat_stream_maps_401() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(401)
            .with_body("nope")
            .create_async()
            .await;
        let provider = OpenAiProvider::new(server.url(), "");
        // Note: the Ok variant (BoxStream) is not Debug, so `unwrap_err()` won't
        // compile here; match on the result instead.
        let err = match provider.chat_stream(req()).await {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(matches!(err, ProviderError::Unauthorized));
    }

    #[test]
    fn parse_chunk_text_delta() {
        let events = parse_chunk(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#).unwrap();
        assert_eq!(events, vec![StreamEvent::TextDelta("Hel".into())]);
    }

    #[test]
    fn parse_chunk_tool_call_fragment() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_1","function":{"name":"shell","arguments":"{\"cmd\":"}}]}}]}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("shell".into()),
                arguments: "{\"cmd\":".into(),
            }]
        );
    }

    #[test]
    fn parse_chunk_usage() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7}}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::Usage {
                prompt_tokens: 12,
                completion_tokens: 7
            }]
        );
    }

    #[test]
    fn parse_chunk_empty_delta_is_no_events() {
        let events = parse_chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn parse_chunk_malformed_is_bad_stream() {
        assert!(parse_chunk("not json").is_err());
    }

    #[test]
    fn parse_chunk_tool_call_continuation_fragment() {
        // The dominant live shape: after the first fragment, continuation chunks
        // carry only `index` + more `arguments` (no `id`, no `name`).
        let data = r#"{"choices":[{"delta":{"tool_calls":[
            {"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments: "\"ls\"}".into(),
            }]
        );
    }
}
