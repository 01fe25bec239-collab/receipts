---
description: Show the Receipts host capability report recorded at session start
---

Read the JSON file `${CLAUDE_PLUGIN_DATA}/host-capability-report.json` and
present it to the user as a short table: `selected_mode`, `inactive_reason`,
`probe_status`, `stale_reason`, `host_version`, `hook_events`, `probed_at`.

Label these as recorded observations, not current admission authority. If
session refresh failed, invalidation failed, or this session's successful
refresh cannot be confirmed, report currency as UNKNOWN even if the stored
file claims EMBEDDED or stale_reason=NONE. Do not follow a redirected data
directory or repair unsafe storage to display a report.

This is display only. Do not edit, create or delete the file, and do not
infer missing values. If the file is missing or unreadable, say that no
readable capability report is available. SessionStart records failed currency
before probing; a successful refresh requires an admitted absolute executable.
