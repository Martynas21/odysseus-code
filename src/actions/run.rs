use std::io::Read;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::sandbox;

/// Returns the snippet's exit code so main can propagate it.
pub fn handle(code: Option<&str>, lang: Option<&str>) -> Result<i32> {
    let cfg = Config::load()?;

    let snippet = match code {
        Some("-") | None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading code snippet from stdin")?;
            buf
        }
        Some(arg) => arg.to_string(),
    };
    if snippet.trim().is_empty() {
        anyhow::bail!("no code to run (pass a snippet or pipe it on stdin)");
    }

    let lang = lang
        .map(str::to_lowercase)
        .unwrap_or_else(|| cfg.default_language.clone());
    let image = sandbox::image_for(&lang, &cfg.sandbox_image)?;

    eprintln!("sandbox: {lang} in {image} (no network)");
    let result = sandbox::run(&lang, &snippet, &image)?;

    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    if result.exit_code != 0 {
        eprintln!("exit code: {}", result.exit_code);
    }
    Ok(result.exit_code)
}
