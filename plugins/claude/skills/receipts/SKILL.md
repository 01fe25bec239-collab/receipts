---
name: receipts
description: Use when the user asks about Receipts in this project, its host capability mode, or the Receipts hook event spool.
---

# Receipts host bridge

Receipts runs a local core; this plugin only forwards Claude Code lifecycle
hooks to it and handles worktree creation.

- The plugin and its hooks have no authority. They never write Receipts
  State; observed hook events are appended to
  `.receipts/inbox/host-events.jsonl` in the project for the core to
  validate later.
- Never edit, delete or rewrite anything under `.receipts/`, and never add
  events to the spool yourself.
- To show the recorded capability report, use `/receipts:capabilities`.
- Worktrees created through Claude Code are placed under
  `.claude/worktrees/<name>` on branch `worktree-<name>`. The standalone CLI
  retains them on exit. Cleanup requires trusted composition with live proof
  of fresh plain creation and verified repository, identity and cleanliness;
  hook paths cannot supply that authority. Keep ambiguous or dirty worktrees.
