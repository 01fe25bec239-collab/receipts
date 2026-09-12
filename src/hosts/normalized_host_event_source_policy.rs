//! Frozen semantic source compatibility for normalized host event types.

use crate::{NormalizedHostEventSourceClass, NormalizedHostEventType};

/// Whether the source class is semantically permitted for the event type.
///
/// This policy establishes no observation, trust, confidence, capability, or
/// persistence. The mapping is frozen by `NORMALIZED_HOST_EVENTS.md` at
/// `f49d621ee510705939394f7df4996223a73fdcb7`.
pub fn source_class_allowed(
    event_type: NormalizedHostEventType,
    source_class: NormalizedHostEventSourceClass,
) -> bool {
    use NormalizedHostEventSourceClass::{CoreDriven, Elicitation, HostHook, WorkerDispatch};
    use NormalizedHostEventType::*;

    match event_type {
        HostSessionStarted | HostSessionEnding | ContextCompacted | HostError => {
            source_class == HostHook
        }
        UserGoalSubmitted | UserInputProvided => source_class == Elicitation,
        RoleExecutorStarted | RoleExecutorStopped | WorkspaceCreated | WorkspaceRemoved => {
            source_class == CoreDriven
        }
        TaskStarted | TaskCompleted | TaskFailed | WorkspaceChanged | ProviderSignal => {
            source_class == WorkerDispatch
        }
        ToolExecuted => matches!(source_class, HostHook | WorkerDispatch),
    }
}
