# Codex native delivery dependency

Native delivery remains unavailable; CodexHostAdapter continues to require a
SUPERVISED report and accepts only INFERRED events. The CLI supports only
`receipts hook claude <Event>`. No native Codex success is claimed.

Q01's sanitized observations establish compatible shapes for SessionStart,
SessionEnd, PostToolUse, PreCompact and PostCompact. The two compact records
report `manual`; automatic compaction and managed policy remain unverified.
The synthetic fixtures under src/hosts/codex/fixtures exercise the existing
bounded normalizer. They do not establish delivery or trusted provenance.

The production blocker is the missing admitted trust/enablement supplier.
The existing observation contract explicitly allows local configuration to
refute permission only. Neither a deserialized capability report, hook JSON,
matching path, environment marker nor a historical positive capture can prove
that this invocation came through an effectively enabled, trusted hook.
Exposing `receipts hook codex` with any of those as authorization would defeat
that boundary. A1 must settle the producer and transport contract before
production delivery is wired. This is a contract dependency, not a request to
change the frozen normalized-event schema.

The required contract must bind current project/session, installed hook and
launcher identity, effective trust/enablement/admin policy and freshness to the
invocation, provide invalidation on disablement or drift, and deny replay and
caller-forged claims. Host can then normalize admitted input and append through
its existing source validator and spool. Generic adapter emit must continue to
refuse caller-created OBSERVED events without that binding.

Remaining operator sequence, after implementation and independent review:

1. Use the Q01 runbook's fresh private root, disposable Git repository and
   isolated CODEX_HOME. Verify installed CLI and Receipts binary paths/hashes,
   package hashes, effective settings and zero unrelated hooks. Authenticate
   directly if necessary; never copy credentials.
2. Install the actual Receipts native plugin, inspect the cached launcher bytes
   and hook definitions, and record the admitted delivery identity. Do not use
   the observer fixture as a substitute for Receipts.
3. With hooks enabled but untrusted, complete a no-tool control turn and exit;
   verify the Receipts inbox was not created or changed.
4. Trust only the installed Receipts hooks. In a fresh session complete one
   shell control turn, invoke the supported manual compaction control and
   confirm completion, then exit normally. Inspect sanitized normalized inbox
   records for all five hooks; record phase and actual trigger separately.
5. With the same installation trusted but hooks disabled, complete a fresh
   no-tool control turn and exit. Verify the inbox was not created or changed.
   Repeat mismatched project/session and malformed/oversized input refusals.
6. Qualify automatic compaction and isolated managed policy separately if
   available; otherwise retain UNKNOWN for each. Record executable/package
   hashes, UTC, completed controls and per-event delivery counts, never raw
   stdin, tool inputs/responses, transcripts or credentials.
7. Remove only the isolated plugin/marketplace, verify settings and residue,
   retain sanitized evidence outside the disposable root, then remove that root.

No Computer Use/desktop Terminal capability was exposed to this A2 session.
The installed-host run has not occurred. The sequence above depends on the
missing production path and cannot currently qualify this checkout.
