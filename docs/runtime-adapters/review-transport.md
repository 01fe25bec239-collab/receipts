# Review capsule transport

Both `ClaudeAdapter` and `CodexAdapter` now accept `RuntimeCapsuleFamily::Review` through the frozen `RuntimeAdapter::start` signature, under these conditions only (otherwise `PolicyBlocked`, with no ordinary fallback):

- the policy is `read_only` and carries an explicit `ProtectedLaunch` (Workspace protected admission; reviewer controller channel for read-only);
- every mandatory policy field is supported (unchanged `supported()` refusals);
- the workspace is bound to the capsule's `task_id`, its base and verified head equal `implementation_sha`, and its branch is a Workspace review branch (`receipts/review/…`).

The capsule is sent as a bounded JSON projection with exactly the ReviewCapsule schema's top-level keys: the 13 required ones always, the 6 optional ones only when present; absent nested optionals are omitted (the schema allows no nulls), present-empty values are preserved and `context_epoch` is written as its exact unbounded digits. This is Runtime transport, not the canonical Review codec. A Review is never converted into a Task. Repair remains refused.

Read-only controller access, Codex read-only sandbox and Claude plan mode are not filesystem confinement; collected results keep reporting reduced isolation.

Not run: supported Review execution needs a Linux protected launcher, which this Mac cannot construct; portable tests prove the projection, binding checks and refusals only. Real-provider review eligibility is a separate gate.
