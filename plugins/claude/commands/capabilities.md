---
description: Show the Receipts host capability report recorded at session start
---

Read the JSON file `${CLAUDE_PLUGIN_DATA}/host-capability-report.json` and
present it to the user as a short table: `selected_mode`, `inactive_reason`,
`probe_status`, `stale_reason`, `host_version`, `hook_events`, `probed_at`.

This is display only. Do not edit, create or delete the file, and do not
infer missing values. If the file is missing or unreadable, say that no
capability report has been recorded yet (it is written at session start only
when `RECEIPTS_CLAUDE_EXECUTABLE` names the absolute `claude` executable).
