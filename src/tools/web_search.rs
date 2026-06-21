use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError, str_arg, truncate};

const DEFAULT_COUNT: u64 = 5;
const MAX_OUTPUT: usize = 40_000;

/// Web search backed by a local SearXNG instance (JSON API). The endpoint is
/// injected when the tool registry is built; `None` means SearXNG has not been
/// configured yet.
pub struct WebSearch {
    pub endpoint: Option<String>,
}

#[async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn description(&self) -> &'static str {
        "Search the web via a local SearXNG instance. Returns ranked results as title / url / snippet."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The search query"},
                "count": {"type": "integer", "description": "Maximum number of results to return (default 5)"}
            },
            "required": ["query"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let base = match self.endpoint.as_deref() {
            Some(u) if !u.is_empty() => u.trim_end_matches('/'),
            _ => {
                return Err(ToolError::Failed(
                    "no SearXNG configured — run the setup-searxng skill, then restart odysseus-code"
                        .into(),
                ));
            }
        };
        let query = str_arg(args, "query")?;
        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_COUNT) as usize;

        let resp = reqwest::Client::new()
            .get(format!("{base}/search"))
            .query(&[("q", query), ("format", "json"), ("categories", "general")])
            .send()
            .await
            .map_err(|e| ToolError::Failed(format!("searxng request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(ToolError::Failed(format!(
                "searxng returned status {}",
                resp.status()
            )));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| ToolError::Failed(format!("searxng response was not valid JSON: {e}")))?;
        let results = body
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if results.is_empty() {
            return Ok(format!("no results for '{query}'"));
        }
        let formatted: Vec<String> = results
            .iter()
            .take(count)
            .map(|r| {
                let title = r
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("(no title)");
                let link = r.get("url").and_then(Value::as_str).unwrap_or("");
                let content = r.get("content").and_then(Value::as_str).unwrap_or("");
                format!("{title}\n{link}\n{content}")
            })
            .collect();
        Ok(truncate(formatted.join("\n\n"), MAX_OUTPUT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn unconfigured_endpoint_errors_with_guidance() {
        let tool = WebSearch { endpoint: None };
        let err = tool
            .execute(&json!({"query": "rust"}), Path::new("."), 5)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("setup-searxng"));
    }

    #[tokio::test]
    async fn parses_searxng_results() {
        let mut server = mockito::Server::new_async().await;
        let body = json!({
            "results": [
                {"title": "Rust", "url": "https://rust-lang.org", "content": "A language"},
                {"title": "The Book", "url": "https://doc.rust-lang.org", "content": "Docs"}
            ]
        });
        let _m = server
            .mock("GET", "/search")
            .match_query(mockito::Matcher::UrlEncoded("format".into(), "json".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await;

        let tool = WebSearch {
            endpoint: Some(server.url()),
        };
        let out = tool
            .execute(&json!({"query": "rust"}), Path::new("."), 5)
            .await
            .unwrap();
        assert!(out.contains("Rust"));
        assert!(out.contains("https://rust-lang.org"));
        assert!(out.contains("The Book"));
    }

    #[tokio::test]
    async fn count_limits_results() {
        let mut server = mockito::Server::new_async().await;
        let body = json!({
            "results": [
                {"title": "first", "url": "u1", "content": "c"},
                {"title": "second", "url": "u2", "content": "c"}
            ]
        });
        let _m = server
            .mock("GET", "/search")
            .match_query(mockito::Matcher::UrlEncoded("format".into(), "json".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await;

        let tool = WebSearch {
            endpoint: Some(server.url()),
        };
        let out = tool
            .execute(&json!({"query": "x", "count": 1}), Path::new("."), 5)
            .await
            .unwrap();
        assert!(out.contains("first"));
        assert!(!out.contains("u2"));
    }

    #[tokio::test]
    async fn non_success_status_errors() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/search")
            .match_query(mockito::Matcher::Any)
            .with_status(502)
            .create_async()
            .await;
        let tool = WebSearch {
            endpoint: Some(server.url()),
        };
        let err = tool
            .execute(&json!({"query": "x"}), Path::new("."), 5)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("502"));
    }
}
