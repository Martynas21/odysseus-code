//! Tracking the progress of a skill while it is being executed.
//!
//! A [`SkillTracker`] holds the single, currently-active [`SkillRun`] (if any).
//! It is shared (cheaply cloned) between the skill tools, which mutate it, and
//! the agent loop, which renders its status into the model's context every turn
//! so progress survives conversational tangents.

use std::sync::{Arc, Mutex};

/// One step of an in-progress skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepState {
    pub title: String,
    pub done: bool,
}

/// The currently-executing skill and the state of its steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRun {
    pub skill: String,
    pub steps: Vec<StepState>,
}

/// What happened when the model marked a step complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// No skill was active, so there was nothing to advance.
    NoActiveSkill,
    /// A step was marked done. `finished` is true when it was the last step
    /// (the run is now cleared).
    Completed {
        title: String,
        remaining: usize,
        finished: bool,
    },
}

/// Shared handle to the single active skill run.
#[derive(Clone, Default)]
pub struct SkillTracker(Arc<Mutex<Option<SkillRun>>>);

impl SkillTracker {
    /// Begin tracking `skill` with the given ordered step titles, replacing any
    /// run already in progress.
    pub fn start(&self, skill: &str, steps: &[String]) {
        let run = SkillRun {
            skill: skill.to_string(),
            steps: steps
                .iter()
                .map(|title| StepState {
                    title: title.clone(),
                    done: false,
                })
                .collect(),
        };
        *self.0.lock().unwrap() = Some(run);
    }

    /// Mark the first not-yet-done step complete and advance. Clears the run
    /// when the final step is completed.
    pub fn complete_next(&self) -> StepOutcome {
        let mut guard = self.0.lock().unwrap();
        let Some(run) = guard.as_mut() else {
            return StepOutcome::NoActiveSkill;
        };
        let Some(step) = run.steps.iter_mut().find(|s| !s.done) else {
            // No pending steps left; treat the run as finished and clear it.
            *guard = None;
            return StepOutcome::NoActiveSkill;
        };
        step.done = true;
        let title = step.title.clone();
        let remaining = run.steps.iter().filter(|s| !s.done).count();
        let finished = remaining == 0;
        if finished {
            *guard = None;
        }
        StepOutcome::Completed {
            title,
            remaining,
            finished,
        }
    }

    /// Abandon the active run. Returns whether a run was actually active.
    pub fn abandon(&self) -> bool {
        self.0.lock().unwrap().take().is_some()
    }

    /// A rendered checklist for the active run, or `None` if nothing is active.
    pub fn status_text(&self) -> Option<String> {
        let guard = self.0.lock().unwrap();
        let run = guard.as_ref()?;
        let mut out = format!("[Skill in progress: {}]\n", run.skill);
        let next = run.steps.iter().position(|s| !s.done);
        for (i, step) in run.steps.iter().enumerate() {
            let mark = if step.done { "x" } else { " " };
            let cursor = if Some(i) == next { "  ← next" } else { "" };
            out.push_str(&format!(" [{mark}] {}{cursor}\n", step.title));
        }
        out.push_str("Call complete_skill_step as you finish each step, or abandon_skill to stop.");
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<String> {
        vec!["one".to_string(), "two".to_string(), "three".to_string()]
    }

    #[test]
    fn start_then_status_shows_all_pending() {
        let t = SkillTracker::default();
        t.start("demo", &steps());
        let status = t.status_text().expect("a run is active");
        assert!(status.contains("demo"));
        assert!(status.contains("one"));
        assert!(status.contains("three"));
    }

    #[test]
    fn complete_next_advances_in_order() {
        let t = SkillTracker::default();
        t.start("demo", &steps());
        let first = t.complete_next();
        assert_eq!(
            first,
            StepOutcome::Completed {
                title: "one".to_string(),
                remaining: 2,
                finished: false,
            }
        );
        let second = t.complete_next();
        assert_eq!(
            second,
            StepOutcome::Completed {
                title: "two".to_string(),
                remaining: 1,
                finished: false,
            }
        );
    }

    #[test]
    fn completing_last_step_finishes_and_clears_run() {
        let t = SkillTracker::default();
        t.start("demo", &["only".to_string()]);
        let outcome = t.complete_next();
        assert_eq!(
            outcome,
            StepOutcome::Completed {
                title: "only".to_string(),
                remaining: 0,
                finished: true,
            }
        );
        assert!(t.status_text().is_none(), "run should be cleared when done");
    }

    #[test]
    fn complete_next_without_active_run_reports_none() {
        let t = SkillTracker::default();
        assert_eq!(t.complete_next(), StepOutcome::NoActiveSkill);
    }

    #[test]
    fn abandon_clears_active_run() {
        let t = SkillTracker::default();
        t.start("demo", &steps());
        assert!(t.abandon(), "abandon reports a run was active");
        assert!(t.status_text().is_none());
        assert!(!t.abandon(), "second abandon reports nothing active");
    }

    #[test]
    fn starting_replaces_previous_run() {
        let t = SkillTracker::default();
        t.start("first", &steps());
        t.complete_next();
        t.start("second", &["fresh".to_string()]);
        let status = t.status_text().unwrap();
        assert!(status.contains("second"));
        assert!(status.contains("fresh"));
        assert!(!status.contains("first"));
    }
}
