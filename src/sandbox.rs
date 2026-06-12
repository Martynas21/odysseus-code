use std::process::Command;

use anyhow::{Context, Result, bail};

/// Outcome of running a snippet in the sandbox container.
#[derive(Debug)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// How to compile/run a language inside its container.
struct LangSpec {
    /// File the snippet is written to inside the workdir.
    filename: &'static str,
    /// Command executed in the container (cwd = /work).
    command: &'static str,
    /// Image used when the config doesn't apply (non-rust languages).
    default_image: &'static str,
}

/// Extend this table to support more languages.
fn spec_for(lang: &str) -> Option<LangSpec> {
    Some(match lang {
        "rust" => LangSpec {
            filename: "main.rs",
            command: "rustc main.rs -o main && ./main",
            default_image: "rust:slim",
        },
        "python" => LangSpec {
            filename: "main.py",
            command: "python3 main.py",
            default_image: "python:3-slim",
        },
        "sh" => LangSpec {
            filename: "main.sh",
            command: "sh main.sh",
            default_image: "alpine:3",
        },
        _ => return None,
    })
}

pub fn supported_languages() -> &'static [&'static str] {
    &["rust", "python", "sh"]
}

/// Resolve the container image for a language: the configured `sandbox_image`
/// is the Rust toolchain image (per the product spec); other languages fall
/// back to their own defaults so they work out of the box.
pub fn image_for(lang: &str, configured: &str) -> Result<String> {
    let spec = spec_for(lang).with_context(|| {
        format!(
            "unsupported language '{lang}' (supported: {}); extend the table in src/sandbox.rs",
            supported_languages().join(", ")
        )
    })?;
    Ok(if lang == "rust" && !configured.trim().is_empty() {
        configured.to_string()
    } else {
        spec.default_image.to_string()
    })
}

/// Compile and execute a snippet in an ephemeral Docker container:
/// temp dir mounted at /work, no network, resource-limited, removed on exit.
pub fn run(lang: &str, code: &str, image: &str) -> Result<SandboxResult> {
    let spec = spec_for(lang).with_context(|| {
        format!(
            "unsupported language '{lang}' (supported: {})",
            supported_languages().join(", ")
        )
    })?;

    let workdir = tempfile::tempdir().context("creating sandbox temp dir")?;
    std::fs::write(workdir.path().join(spec.filename), code)
        .context("writing snippet to sandbox dir")?;

    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network=none",
            "--memory=512m",
            "--pids-limit=256",
            "-v",
            &format!("{}:/work", workdir.path().display()),
            "-w",
            "/work",
            image,
            "sh",
            "-c",
            // Hard cap so infinite loops can't wedge the CLI.
            &format!("timeout 120 sh -c '{}'", spec.command),
        ])
        .output()
        .context("running docker — is Docker installed and the daemon running?")?;

    let result = SandboxResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    };

    // 125 = docker itself failed (bad image, daemon error) — that's our error,
    // not the snippet's.
    if result.exit_code == 125 {
        bail!(
            "docker failed to start the sandbox: {}",
            result.stderr.trim()
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_is_rejected() {
        let err = run("cobol", "DISPLAY 'HI'.", "alpine:3").unwrap_err();
        assert!(err.to_string().contains("unsupported language 'cobol'"));
        assert!(err.to_string().contains("rust, python, sh"));
    }

    #[test]
    fn configured_image_applies_to_rust_only() {
        assert_eq!(image_for("rust", "my/rust:1").unwrap(), "my/rust:1");
        assert_eq!(image_for("rust", "").unwrap(), "rust:slim");
        assert_eq!(image_for("python", "my/rust:1").unwrap(), "python:3-slim");
        assert_eq!(image_for("sh", "my/rust:1").unwrap(), "alpine:3");
        assert!(image_for("cobol", "x").is_err());
    }

    /// Needs Docker + the alpine:3 image; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires docker and network to pull alpine:3"]
    fn sh_snippet_runs_in_sandbox() {
        let result = run("sh", "echo hello-from-sandbox; exit 7", "alpine:3").unwrap();
        assert!(result.stdout.contains("hello-from-sandbox"));
        assert_eq!(result.exit_code, 7);
    }
}
