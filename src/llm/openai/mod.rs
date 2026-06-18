use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};

use crate::config::Config;
use crate::llm::sse::SseDecoder;
use crate::llm::{ChatRequest, Provider, ProviderError, StreamEvent};

mod wire;
use wire::parse_chunk;

const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

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
                        // Stream ended. Drain any final line that arrived
                        // without a trailing newline so its payload (e.g. the
                        // closing fragment of a tool call's arguments) isn't
                        // dropped, then synthesize Done.
                        if let Some(data) = st.decoder.flush()
                            && data != "[DONE]"
                        {
                            match parse_chunk(&data) {
                                Ok(events) => st.queue.extend(events.into_iter().map(Ok)),
                                Err(e) => st.queue.push_back(Err(e)),
                            }
                        }
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
mod tests;
