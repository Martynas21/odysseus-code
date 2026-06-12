use std::path::Path;

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::client::OdysseusClient;
use crate::config::Config;
use crate::context::PromptContext;
use crate::session::SessionStore;

pub async fn handle(
    lang: &str,
    description: &str,
    format: OutputFormat,
    session_id: Option<&str>,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
) -> Result<()> {
    let cfg = Config::load()?;
    let client = OdysseusClient::from_config(&cfg)?;
    let mut store = SessionStore::load()?;

    let session = super::resolve_session(&client, &cfg, &mut store, session_id).await?;
    let mut ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
    ctx.language = lang.to_lowercase();

    let instruction = format!(
        "Generate {lang} code: {description}\n\
         Reply with ONLY the code in a single fenced code block. \
         No explanation before or after."
    );
    let reply = client.chat(&session, &ctx.wrap(&instruction)).await?;
    let code = extract_code(&reply);

    match format {
        OutputFormat::Pretty => println!("```{}\n{}\n```", ctx.language, code),
        OutputFormat::Compact => println!("{code}"),
    }
    Ok(())
}

/// Pull the code out of the model reply: the first fenced block if present,
/// otherwise the whole reply trimmed.
fn extract_code(reply: &str) -> String {
    let trimmed = reply.trim();
    if let Some(open) = trimmed.find("```") {
        let after_fence = &trimmed[open + 3..];
        // Skip the info string (e.g. "rust") up to the first newline.
        let body_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &after_fence[body_start..];
        let body_end = body.find("```").unwrap_or(body.len());
        return body[..body_end].trim_end().to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fenced_block_with_language_tag() {
        let reply = "Here you go:\n```rust\nfn main() {}\n```\nEnjoy!";
        assert_eq!(extract_code(reply), "fn main() {}");
    }

    #[test]
    fn extracts_fenced_block_without_language_tag() {
        let reply = "```\nprint('hi')\n```";
        assert_eq!(extract_code(reply), "print('hi')");
    }

    #[test]
    fn unfenced_reply_is_returned_trimmed() {
        assert_eq!(extract_code("  fn main() {}\n"), "fn main() {}");
    }

    #[test]
    fn unterminated_fence_takes_rest_of_reply() {
        let reply = "```rust\nfn main() {}";
        assert_eq!(extract_code(reply), "fn main() {}");
    }
}
