use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

use crate::config::Config;

/// How long to wait for a chat reply. Local models can be slow.
const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Cap on honoring a 429 Retry-After header.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(
        "not authenticated by Odysseus (HTTP 401).\n\
         Create an API token in the Odysseus web UI (Settings → Integrations → API Tokens,\n\
         admin only) and run:\n\
         odysseus-code config set api_key ody_...\n\
         (or export ODYSSEUS_API_TOKEN)"
    )]
    Unauthorized,
    #[error("rate limited by Odysseus (HTTP 429); gave up after one retry")]
    RateLimited,
    #[error("Odysseus returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("could not reach Odysseus at {url}: {source}")]
    Network { url: String, source: reqwest::Error },
    #[error("unexpected response from Odysseus: {0}")]
    BadResponse(String),
}

/// Client for the Odysseus REST API (`/api/chat`, `/api/session`, …).
#[derive(Clone)]
pub struct OdysseusClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

/// One endpoint entry from `GET /api/models`.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelEndpoint {
    #[serde(default)]
    pub endpoint_id: String,
    #[serde(default)]
    pub endpoint_name: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub models_extra: Vec<String>,
}

/// Session info from `POST /api/session` and `GET /api/sessions`.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub model: String,
}

/// One message from `GET /api/history/{sid}`.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    response: String,
}

#[derive(Debug, Deserialize)]
struct HistoryResponse {
    #[serde(default)]
    history: Vec<HistoryMessage>,
}

impl OdysseusClient {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(CHAT_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .expect("building HTTP client"),
            base: base.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    /// Build a client from config, rejecting a missing API token up front so
    /// the user gets the setup hint before any network call.
    pub fn from_config(cfg: &Config) -> Result<Self, ClientError> {
        if cfg.api_key.trim().is_empty() || cfg.api_key == "YOUR_KEY_HERE" {
            return Err(ClientError::Unauthorized);
        }
        Ok(Self::new(&cfg.endpoint, &cfg.api_key))
    }

    /// Send a chat message into an existing session. Returns the reply text.
    pub async fn chat(&self, session_id: &str, message: &str) -> Result<String, ClientError> {
        let url = format!("{}/api/chat", self.base);
        let body = serde_json::json!({ "message": message, "session": session_id });

        let mut response = self.send(self.http.post(&url).json(&body)).await?;
        if response.status().as_u16() == 429 {
            let wait = retry_after(&response).min(MAX_RETRY_AFTER);
            tokio::time::sleep(wait).await;
            response = self.send(self.http.post(&url).json(&body)).await?;
            if response.status().as_u16() == 429 {
                return Err(ClientError::RateLimited);
            }
        }
        let parsed: ChatResponse = self.parse(response).await?;
        Ok(parsed.response)
    }

    /// Create a server-side session. `endpoint_id` references a configured
    /// Odysseus model endpoint (raw endpoint URLs are admin-only).
    pub async fn create_session(
        &self,
        name: &str,
        endpoint_id: &str,
        model: &str,
    ) -> Result<SessionInfo, ClientError> {
        let url = format!("{}/api/session", self.base);
        let form = [
            ("name", name),
            ("endpoint_id", endpoint_id),
            ("model", model),
        ];
        let response = self.send(self.http.post(&url).form(&form)).await?;
        self.parse(response).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), ClientError> {
        let url = format!("{}/api/session/{session_id}", self.base);
        let response = self.send(self.http.delete(&url)).await?;
        self.check_status(response).await?;
        Ok(())
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>, ClientError> {
        let url = format!("{}/api/sessions", self.base);
        let response = self.send(self.http.get(&url)).await?;
        self.parse(response).await
    }

    pub async fn list_models(&self) -> Result<Vec<ModelEndpoint>, ClientError> {
        let url = format!("{}/api/models", self.base);
        let response = self.send(self.http.get(&url)).await?;
        self.parse(response).await
    }

    pub async fn history(&self, session_id: &str) -> Result<Vec<HistoryMessage>, ClientError> {
        let url = format!("{}/api/history/{session_id}", self.base);
        let response = self.send(self.http.get(&url)).await?;
        let parsed: HistoryResponse = self.parse(response).await?;
        Ok(parsed.history)
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, ClientError> {
        let req = req
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json");
        let built = req.build().map_err(|e| ClientError::Network {
            url: self.base.clone(),
            source: e,
        })?;
        let url = built.url().to_string();
        self.http
            .execute(built)
            .await
            .map_err(|source| ClientError::Network { url, source })
    }

    /// Map non-success statuses to errors and return the body text.
    async fn check_status(&self, response: reqwest::Response) -> Result<String, ClientError> {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        match status.as_u16() {
            200..=299 => Ok(body),
            401 => Err(ClientError::Unauthorized),
            429 => Err(ClientError::RateLimited),
            code => Err(ClientError::Http {
                status: code,
                body: snippet(&body),
            }),
        }
    }

    async fn parse<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, ClientError> {
        let body = self.check_status(response).await?;
        serde_json::from_str(&body)
            .map_err(|e| ClientError::BadResponse(format!("{e} — body: {}", snippet(&body))))
    }
}

fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(1))
}

fn snippet(body: &str) -> String {
    const MAX: usize = 300;
    if body.len() <= MAX {
        body.to_string()
    } else {
        let cut = body
            .char_indices()
            .take_while(|(i, _)| *i < MAX)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(MAX);
        format!("{}…", &body[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(server: &mockito::Server) -> OdysseusClient {
        OdysseusClient::new(server.url(), "ody_testtoken")
    }

    #[tokio::test]
    async fn chat_sends_message_and_returns_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_header("authorization", "Bearer ody_testtoken")
            .match_body(mockito::Matcher::JsonString(
                r#"{"message":"hi","session":"s1"}"#.into(),
            ))
            .with_status(200)
            .with_body(r#"{"response":"hello from odysseus"}"#)
            .create_async()
            .await;

        let reply = client(&server).chat("s1", "hi").await.unwrap();
        assert_eq!(reply, "hello from odysseus");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_401_maps_to_unauthorized_with_hint() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/chat")
            .with_status(401)
            .with_body(r#"{"error":"Not authenticated"}"#)
            .create_async()
            .await;

        let err = client(&server).chat("s1", "hi").await.unwrap_err();
        assert!(matches!(err, ClientError::Unauthorized));
        assert!(
            err.to_string()
                .contains("Settings → Integrations → API Tokens")
        );
    }

    #[tokio::test]
    async fn chat_retries_once_on_429_then_succeeds() {
        let mut server = mockito::Server::new_async().await;
        let limited = server
            .mock("POST", "/api/chat")
            .with_status(429)
            .with_header("retry-after", "0")
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"response":"ok"}"#)
            .expect(1)
            .create_async()
            .await;

        let reply = client(&server).chat("s1", "hi").await.unwrap();
        assert_eq!(reply, "ok");
        limited.assert_async().await;
        ok.assert_async().await;
    }

    #[tokio::test]
    async fn chat_500_maps_to_http_error_with_body() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;

        let err = client(&server).chat("s1", "hi").await.unwrap_err();
        match err {
            ClientError::Http { status, body } => {
                assert_eq!(status, 500);
                assert_eq!(body, "boom");
            }
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn chat_bad_json_maps_to_bad_response() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body("<html>oops</html>")
            .create_async()
            .await;

        let err = client(&server).chat("s1", "hi").await.unwrap_err();
        assert!(matches!(err, ClientError::BadResponse(_)));
        assert!(err.to_string().contains("<html>oops</html>"));
    }

    #[tokio::test]
    async fn create_session_posts_form_and_parses_id() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/session")
            .match_header("content-type", "application/x-www-form-urlencoded")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("name".into(), "odysseus-code".into()),
                mockito::Matcher::UrlEncoded("endpoint_id".into(), "ep1".into()),
                mockito::Matcher::UrlEncoded("model".into(), "qwen3".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"id":"abc123","name":"odysseus-code","model":"qwen3","rag":false,"archived":false}"#)
            .create_async()
            .await;

        let info = client(&server)
            .create_session("odysseus-code", "ep1", "qwen3")
            .await
            .unwrap();
        assert_eq!(info.id, "abc123");
        assert_eq!(info.model, "qwen3");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn delete_session_hits_session_path() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/api/session/abc123")
            .with_status(200)
            .with_body(r#"{"status":"deleted"}"#)
            .create_async()
            .await;

        client(&server).delete_session("abc123").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn list_models_parses_endpoints() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/models")
            .with_status(200)
            .with_body(
                r#"[{"host":"custom","port":0,"url":"http://x/v1/chat/completions",
                     "models":["qwen3","gpt-oss"],"models_extra":[],
                     "endpoint_id":"ep1","endpoint_name":"local llama"}]"#,
            )
            .create_async()
            .await;

        let endpoints = client(&server).list_models().await.unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].endpoint_id, "ep1");
        assert_eq!(endpoints[0].models, vec!["qwen3", "gpt-oss"]);
    }

    #[tokio::test]
    async fn history_unwraps_messages() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/history/s1")
            .with_status(200)
            .with_body(r#"{"history":[{"role":"user","content":"hi"},{"role":"assistant","content":"hey"}]}"#)
            .create_async()
            .await;

        let history = client(&server).history("s1").await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, "hey");
    }

    #[test]
    fn from_config_rejects_missing_token() {
        let cfg = Config::default();
        assert!(matches!(
            OdysseusClient::from_config(&cfg),
            Err(ClientError::Unauthorized)
        ));
        let placeholder = Config {
            api_key: "YOUR_KEY_HERE".into(),
            ..Config::default()
        };
        assert!(OdysseusClient::from_config(&placeholder).is_err());
    }
}
