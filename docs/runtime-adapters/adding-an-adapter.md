# Adding a runtime adapter

A new runtime needs no edit to a core Runtime file. Implement it in its own
file using these public extension points:

1. Implement `RuntimeAdapter` with the vendor-neutral bindings:
   `HealthReport = Models = RuntimeObservation`,
   `ExecutionPolicy = RuntimeExecutionPolicy` and `CancelReason = CancelReason`.
   Refuse anything you cannot enforce: `RuntimeExecutionPolicy::supported()`
   is false when a mandatory requirement is present, and that is
   `POLICY_BLOCKED`.
2. Start processes only through the Workspace runner
   (`start_live_process_attempt`). Workspace owns spawning, environment,
   timeout, cancellation, collection and cleanup.
3. Return capability observations as
   `RuntimeCapabilities::Extension(ExtensionCapabilityReport)`.
4. Make the event stream implement `registry::RuntimeEventStream`. It must
   wake the task when progress is possible. Convert events into
   `registry::DynAttemptEvent`.
5. Make the collected result implement `registry::AttemptFailure`.
   `classify_live_outcome` gives the lifecycle classes. Record the cancel
   reason only when Workspace accepts the cancellation.
6. Build failures with `RawFailure::live_start`, `live_snapshot` and
   `live_collect` under your `RuntimeId`. These carry no payload and choose no
   class from output.
7. Register a boxed adapter with `registry::RuntimeAdapterRegistry`. Empty and
   duplicate runtime IDs are refused.

Registration grants no authority. Dispatch admission, routing, provider policy
and credentials stay outside the adapter. Add a provider-specific failure
mapping only with a real recorded capture and its provenance; otherwise it
stays `UNKNOWN` (see `failure-classes.md`).

`src/adapters/runtime/third_adapter_lifecycle_tests.rs` is a working example.
