use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError, str_arg, truncate};
use crate::skills::{SkillTracker, StepOutcome, bundled};

const MAX_OUTPUT: usize = 60_000;

/// Schema shared by the skill tools that take no arguments.
fn no_params() -> Value {
    json!({"type": "object", "properties": {}})
}

pub struct ListSkills;

#[async_trait]
impl Tool for ListSkills {
    fn name(&self) -> &'static str {
        "list_skills"
    }
    fn description(&self) -> &'static str {
        "List the named skills available. Each skill is a reusable, step-by-step \
         procedure you can load with invoke_skill and then carry out using your \
         other tools."
    }
    fn parameters(&self) -> Value {
        no_params()
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let listing = bundled()
            .iter()
            .map(|s| format!("{} — {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n");
        if listing.is_empty() {
            Ok("No skills are available.".to_string())
        } else {
            Ok(truncate(listing, MAX_OUTPUT))
        }
    }
}

pub struct InvokeSkill {
    pub tracker: SkillTracker,
}

#[async_trait]
impl Tool for InvokeSkill {
    fn name(&self) -> &'static str {
        "invoke_skill"
    }
    fn description(&self) -> &'static str {
        "Load a skill's full step-by-step instructions by name and begin tracking \
         your progress through it. Returns the instructions for you to follow \
         using your other tools. Use list_skills first to see what is available."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Name of the skill to load"}
            },
            "required": ["name"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let name = str_arg(args, "name")?;
        match bundled().iter().find(|s| s.name == name) {
            Some(skill) => {
                // Begin tracking; the agent loop pins the live checklist into
                // context each turn, so we only need to return the instructions.
                self.tracker.start(&skill.name, &skill.steps);
                Ok(truncate(skill.body.clone(), MAX_OUTPUT))
            }
            None => {
                let available = bundled()
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(ToolError::Failed(format!(
                    "no skill named '{name}'. Available skills: {available}"
                )))
            }
        }
    }
}

pub struct CompleteSkillStep {
    pub tracker: SkillTracker,
}

#[async_trait]
impl Tool for CompleteSkillStep {
    fn name(&self) -> &'static str {
        "complete_skill_step"
    }
    fn description(&self) -> &'static str {
        "Mark the next step of the in-progress skill complete and advance. Call \
         this each time you finish a step so your progress stays tracked."
    }
    fn parameters(&self) -> Value {
        no_params()
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        match self.tracker.complete_next() {
            StepOutcome::NoActiveSkill => {
                Ok("No skill is in progress. Use invoke_skill to start one.".to_string())
            }
            StepOutcome::Completed {
                title,
                finished: true,
                ..
            } => Ok(format!(
                "Completed final step: {title}. The skill is now complete."
            )),
            StepOutcome::Completed {
                title, remaining, ..
            } => Ok(format!(
                "Completed step: {title}. {remaining} step(s) remaining."
            )),
        }
    }
}

pub struct AbandonSkill {
    pub tracker: SkillTracker,
}

#[async_trait]
impl Tool for AbandonSkill {
    fn name(&self) -> &'static str {
        "abandon_skill"
    }
    fn description(&self) -> &'static str {
        "Stop tracking the in-progress skill when you are no longer following it."
    }
    fn parameters(&self) -> Value {
        no_params()
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        if self.tracker.abandon() {
            Ok("Stopped tracking the skill.".to_string())
        } else {
            Ok("No skill was in progress.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn list_skills_lists_name_and_description() {
        let out = ListSkills
            .execute(&json!({}), Path::new("."), 5)
            .await
            .unwrap();
        assert!(out.contains("summarize-changes"));
        assert!(out.contains("Summarize the uncommitted changes"));
    }

    #[tokio::test]
    async fn invoke_skill_returns_body_for_known_name() {
        let tool = InvokeSkill {
            tracker: SkillTracker::default(),
        };
        let out = tool
            .execute(&json!({"name": "summarize-changes"}), Path::new("."), 5)
            .await
            .unwrap();
        assert!(out.contains("git diff"));
        // The frontmatter must not leak into the returned instructions.
        assert!(!out.contains("description:"));
    }

    #[tokio::test]
    async fn invoke_skill_unknown_name_errors_with_valid_names() {
        let tool = InvokeSkill {
            tracker: SkillTracker::default(),
        };
        let err = tool
            .execute(&json!({"name": "does-not-exist"}), Path::new("."), 5)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("does-not-exist"));
        assert!(msg.contains("summarize-changes"));
    }

    #[tokio::test]
    async fn invoke_skill_starts_tracking_with_steps() {
        let tracker = SkillTracker::default();
        let tool = InvokeSkill {
            tracker: tracker.clone(),
        };
        tool.execute(&json!({"name": "summarize-changes"}), Path::new("."), 5)
            .await
            .unwrap();
        let status = tracker
            .status_text()
            .expect("invoking starts a tracked run");
        assert!(status.contains("summarize-changes"));
    }

    #[tokio::test]
    async fn complete_skill_step_advances_and_finishes() {
        let tracker = SkillTracker::default();
        tracker.start("demo", &["only".to_string()]);
        let tool = CompleteSkillStep {
            tracker: tracker.clone(),
        };
        let out = tool.execute(&json!({}), Path::new("."), 5).await.unwrap();
        assert!(out.contains("only"));
        assert!(out.to_lowercase().contains("complete"));
        assert!(tracker.status_text().is_none(), "run cleared when finished");
    }

    #[tokio::test]
    async fn complete_skill_step_without_run_is_informative() {
        let tool = CompleteSkillStep {
            tracker: SkillTracker::default(),
        };
        let out = tool.execute(&json!({}), Path::new("."), 5).await.unwrap();
        assert!(out.to_lowercase().contains("no skill"));
    }

    #[tokio::test]
    async fn abandon_skill_clears_run() {
        let tracker = SkillTracker::default();
        tracker.start("demo", &["a".to_string(), "b".to_string()]);
        let tool = AbandonSkill {
            tracker: tracker.clone(),
        };
        let out = tool.execute(&json!({}), Path::new("."), 5).await.unwrap();
        assert!(out.to_lowercase().contains("stopped"));
        assert!(tracker.status_text().is_none());
    }
}
