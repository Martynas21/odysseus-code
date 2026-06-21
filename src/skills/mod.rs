//! Skills: named, reusable procedures the model can discover and load on demand.
//!
//! A skill is a markdown document with a YAML frontmatter block (`name`,
//! `description`) followed by an instruction body. For the MVP, skills are
//! bundled into the binary at compile time and contain instructions only — the
//! model carries them out using the existing tools.

use std::sync::OnceLock;

use serde::Deserialize;

pub mod run;
pub use run::{SkillTracker, StepOutcome};

/// A reusable procedure the model can load and follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Ordered, authored checklist of the skill's steps. May be empty for
    /// skills that are too small or free-form to track step by step.
    pub steps: Vec<String>,
    pub body: String,
}

#[derive(Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default)]
    steps: Vec<String>,
}

/// Markdown sources for the skills bundled into the binary.
const BUNDLED_SOURCES: &[&str] = &[
    include_str!("bundled/summarize-changes.md"),
    include_str!("bundled/setup-searxng.md"),
];

/// Parse a skill from its markdown-with-frontmatter source.
///
/// The source must start with a `---` fenced YAML block holding `name` and
/// `description`; everything after the closing `---` is the instruction body.
pub fn parse_skill(md: &str) -> Skill {
    let rest = md
        .strip_prefix("---\n")
        .or_else(|| md.strip_prefix("---\r\n"))
        .unwrap_or_else(|| panic!("skill is missing a frontmatter block"));
    let (front, body) = rest
        .split_once("\n---")
        .unwrap_or_else(|| panic!("skill frontmatter is not terminated by ---"));
    let fm: Frontmatter =
        serde_yaml::from_str(front).unwrap_or_else(|e| panic!("invalid skill frontmatter: {e}"));
    Skill {
        name: fm.name,
        description: fm.description,
        steps: fm.steps,
        body: body.trim().to_string(),
    }
}

/// All skills bundled into the binary.
pub fn bundled() -> &'static [Skill] {
    static SKILLS: OnceLock<Vec<Skill>> = OnceLock::new();
    SKILLS.get_or_init(|| BUNDLED_SOURCES.iter().map(|src| parse_skill(src)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skill_extracts_frontmatter_and_body() {
        let md = "---\nname: demo\ndescription: A demo skill.\n---\n\nStep one.\nStep two.\n";
        let skill = parse_skill(md);
        assert_eq!(skill.name, "demo");
        assert_eq!(skill.description, "A demo skill.");
        assert_eq!(skill.body, "Step one.\nStep two.");
    }

    #[test]
    fn parse_skill_extracts_steps() {
        let md = "---\nname: demo\ndescription: A demo skill.\nsteps:\n  - First step\n  - Second step\n---\n\nDo the thing.\n";
        let skill = parse_skill(md);
        assert_eq!(skill.steps, vec!["First step", "Second step"]);
    }

    #[test]
    fn parse_skill_without_steps_yields_empty() {
        let md = "---\nname: demo\ndescription: A demo skill.\n---\n\nDo the thing.\n";
        let skill = parse_skill(md);
        assert!(skill.steps.is_empty());
    }

    #[test]
    fn bundled_summarize_changes_has_steps() {
        let skill = bundled()
            .iter()
            .find(|s| s.name == "summarize-changes")
            .unwrap();
        assert!(
            !skill.steps.is_empty(),
            "summarize-changes should declare steps"
        );
    }

    #[test]
    fn bundled_includes_summarize_changes() {
        let names: Vec<&str> = bundled().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"summarize-changes"));
    }

    #[test]
    fn bundled_skills_are_well_formed() {
        for skill in bundled() {
            assert!(!skill.name.is_empty(), "skill name must not be empty");
            assert!(
                !skill.description.is_empty(),
                "{} description must not be empty",
                skill.name
            );
            assert!(
                !skill.body.trim().is_empty(),
                "{} body must not be empty",
                skill.name
            );
        }
    }

    #[test]
    fn bundled_includes_setup_searxng() {
        let names: Vec<&str> = bundled().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"setup-searxng"));
    }

    #[test]
    fn bundled_setup_searxng_has_steps() {
        let skill = bundled()
            .iter()
            .find(|s| s.name == "setup-searxng")
            .unwrap();
        assert!(
            !skill.steps.is_empty(),
            "setup-searxng should declare steps"
        );
    }
}
