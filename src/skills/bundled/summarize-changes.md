---
name: summarize-changes
description: Summarize the uncommitted changes in the working tree.
steps:
  - Inspect the working tree with git status and git diff
  - Read changed files that need more context
  - Write a concise, grouped summary of the changes
---

Follow these steps to summarize the uncommitted changes in the repository.

1. **Inspect the working tree.** Run `git status --short` and `git diff` with the
   shell tool to see what has changed (include staged changes with
   `git diff --cached`).
2. **Read changed files for context.** For any change that needs more context,
   open the affected file with read_file to understand the surrounding code.
3. **Write the summary.** Produce a concise, high-signal summary: group related
   changes, state what each change does and why it matters, and call out anything
   risky or incomplete. Prefer a short bulleted list over prose.

As you finish each step, call complete_skill_step so your progress is tracked.
