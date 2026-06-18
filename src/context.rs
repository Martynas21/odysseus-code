use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub project_path: String,
    pub current_file: Option<String>,
    pub language: String,
}

impl PromptContext {
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

    pub fn system_prompt(&self) -> String {
        let mut s = String::new();
        s.push_str(
            "You are a coding agent operating directly on the user's local repository. \
             You can call tools to read and modify files and run shell commands. \
             Call one or more tools when useful, then wait for their results before \
             continuing. Prefer minimal, targeted changes.\n\n",
        );
        s.push_str(&format!("Workspace: {}\n", self.project_path));
        if let Some(file) = &self.current_file {
            s.push_str(&format!("Current file: {file}\n"));
        }
        s.push_str(&format!("Primary language: {}\n", self.language));
        s
    }
}

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
    fn system_prompt_describes_workspace_and_tools() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let sys = ctx.system_prompt();
        assert!(sys.contains("/proj"));
        assert!(sys.contains("rust"));
        assert!(sys.to_lowercase().contains("tool"));
    }

    #[test]
    fn system_prompt_includes_current_file_when_present() {
        let ctx = PromptContext::build(
            Some(Path::new("/proj")),
            Some(Path::new("/proj/src/main.rs")),
            "rust",
        );
        let sys = ctx.system_prompt();
        assert!(sys.contains("/proj/src/main.rs"));
    }
}
