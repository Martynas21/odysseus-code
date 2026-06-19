use std::path::Path;

use crate::mode::Mode;

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

    pub fn system_prompt(&self, mode: Mode, specs: &[String]) -> String {
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
        if matches!(mode, Mode::Implement) {
            s.push_str(
                "\nYou also have skills: reusable, step-by-step procedures. Call list_skills \
                 to see what is available, then invoke_skill to load one and follow it.\n",
            );
        }
        s.push('\n');
        s.push_str(mode_instructions(mode));
        if !specs.is_empty() {
            s.push_str("\n\n## Available specifications\n\n");
            for path in specs {
                s.push_str(&format!("- {path}\n"));
            }
            s.push_str(
                "Read the relevant one(s) with read_file before implementing.\n",
            );
        }
        s
    }
}

fn mode_instructions(mode: Mode) -> &'static str {
    match mode {
        Mode::Spec => concat!(
            "You are in SPEC mode. Your only goal is to produce a clear specification ",
            "document for the feature the user wants — its core behaviour, requirements, ",
            "and relevant edge cases. Do NOT write or modify source code and do NOT run ",
            "mutating shell commands. Use the ask_user tool to ask the user EXACTLY ONE ",
            "clarifying question at a time, offering 2-3 concise options, and WAIT for the ",
            "answer before asking the next question. NEVER assume or fabricate answers the ",
            "user has not given. Save the specification to ",
            "docs/edds/<kebab-case-feature-title>.md with the write_file tool (the ONLY ",
            "files you may write are *.md files under docs/edds/). When the specification is ",
            "complete and there is nothing left to clarify, tell the user to press Shift+Tab ",
            "to switch to implement mode — do not start coding yourself.",
        ),
        Mode::Implement => concat!(
            "You are in IMPLEMENT mode. Build the feature the user asks for, editing code ",
            "as needed. If specifications are listed below, read the relevant one(s) with ",
            "read_file before implementing. Use the ask_user tool to clarify ambiguities, ",
            "asking one question at a time.",
        ),
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
        let sys = ctx.system_prompt(Mode::Implement, &[]);
        assert!(sys.contains("/proj"));
        assert!(sys.contains("rust"));
        assert!(sys.to_lowercase().contains("tool"));
    }

    #[test]
    fn system_prompt_mentions_skills() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let sys = ctx.system_prompt(Mode::Implement, &[]);
        assert!(sys.contains("list_skills"));
        // Spec mode has no skill tools, so it must not advertise them.
        assert!(!ctx.system_prompt(Mode::Spec, &[]).contains("list_skills"));
    }

    #[test]
    fn system_prompt_includes_current_file_when_present() {
        let ctx = PromptContext::build(
            Some(Path::new("/proj")),
            Some(Path::new("/proj/src/main.rs")),
            "rust",
        );
        let sys = ctx.system_prompt(Mode::Implement, &[]);
        assert!(sys.contains("/proj/src/main.rs"));
    }

    #[test]
    fn spec_prompt_forbids_code_edits_and_names_spec_path() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let sys = ctx.system_prompt(Mode::Spec, &[]);
        assert!(sys.contains("SPEC mode"));
        assert!(sys.contains("ask_user"));
        assert!(sys.contains("one"));
        assert!(sys.contains("docs/edds/"));
        assert!(sys.to_lowercase().contains("do not write or modify source code"));
    }

    #[test]
    fn implement_prompt_mentions_building_and_reading_specs() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let sys = ctx.system_prompt(Mode::Implement, &[]);
        assert!(sys.contains("IMPLEMENT mode"));
        assert!(sys.to_lowercase().contains("build the feature"));
        assert!(sys.contains("read_file"));
    }

    #[test]
    fn spec_section_only_appears_when_specs_present() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        assert!(
            !ctx.system_prompt(Mode::Implement, &[])
                .contains("## Available specifications")
        );
        let with_specs =
            ctx.system_prompt(Mode::Implement, &["docs/edds/foo.md".to_string()]);
        assert!(with_specs.contains("## Available specifications"));
        assert!(with_specs.contains("docs/edds/foo.md"));
    }
}
