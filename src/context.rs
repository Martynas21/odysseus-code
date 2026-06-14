use std::path::Path;

/// Code-centric metadata wrapped around every prompt so the model behaves
/// like a coding companion rather than a generic chat.
///
/// The Odysseus chat API has no custom metadata fields, so the context is
/// embedded as a small JSON block prefixed to the message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    /// Absolute path of the workspace (defaults to ".").
    pub project_path: String,
    /// Full path of the file being edited, if any.
    pub current_file: Option<String>,
    /// Programming-language context.
    pub language: String,
}

impl PromptContext {
    /// Build the context from CLI flags and config. Language is inferred from
    /// the current file's extension, falling back to `default_language`.
    pub fn build(
        project_path: Option<&Path>,
        current_file: Option<&Path>,
        default_language: &str,
    ) -> Self {
        let language = current_file
            .and_then(|f| f.extension())
            .and_then(|e| e.to_str())
            .and_then(language_for_extension)
            .unwrap_or(default_language)
            .to_string();
        Self {
            project_path: project_path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ".".into()),
            current_file: current_file.map(|p| p.display().to_string()),
            language,
        }
    }

    /// Wrap a prompt with the context block.
    pub fn wrap(&self, prompt: &str) -> String {
        let mut meta = serde_json::Map::new();
        meta.insert("project_path".into(), self.project_path.clone().into());
        if let Some(file) = &self.current_file {
            meta.insert("current_file".into(), file.clone().into());
        }
        meta.insert("language".into(), self.language.clone().into());
        format!(
            "[context] {} [/context]\n\n{prompt}",
            serde_json::Value::Object(meta)
        )
    }
}

/// Map a file extension to a language name understood by the model.
/// Extend this table to support more languages.
pub fn language_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "rb" => "ruby",
        "sh" | "bash" | "zsh" => "sh",
        "md" => "markdown",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "toml" => "toml",
        "sql" => "sql",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn defaults_when_nothing_given() {
        let ctx = PromptContext::build(None, None, "rust");
        assert_eq!(ctx.project_path, ".");
        assert_eq!(ctx.current_file, None);
        assert_eq!(ctx.language, "rust");
    }

    #[test]
    fn language_inferred_from_current_file() {
        let file = PathBuf::from("/src/app.py");
        let ctx = PromptContext::build(None, Some(&file), "rust");
        assert_eq!(ctx.language, "python");
        assert_eq!(ctx.current_file.as_deref(), Some("/src/app.py"));
    }

    #[test]
    fn unknown_extension_falls_back_to_default() {
        let file = PathBuf::from("/data/file.xyz");
        let ctx = PromptContext::build(None, Some(&file), "rust");
        assert_eq!(ctx.language, "rust");
    }

    #[test]
    fn wrap_embeds_metadata_and_prompt() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let wrapped = ctx.wrap("Explain borrowing");
        assert!(wrapped.starts_with("[context] {"));
        assert!(wrapped.contains("\"project_path\":\"/proj\""));
        assert!(wrapped.contains("\"language\":\"rust\""));
        assert!(wrapped.ends_with("\n\nExplain borrowing"));
        // no current_file key when none given
        assert!(!wrapped.contains("current_file"));
    }
}
