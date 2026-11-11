use super::*;
use crate::{NormalizedHostEventId, NormalizedHostEventInputs, NormalizedHostEventType};
use receipts_orchestration::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};
use receipts_orchestration::{
    GraphNodeState, GraphSnapshot, GraphSnapshotNodeState, GraphVersionV1,
};
fn at() -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new("2026-11-11T00:00:00Z").unwrap()
}
#[test]
fn headless_goal_provenance_and_view_fixture() {
    let root = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("receipts-headless-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let spool = HostEventSpool::for_project(&root).unwrap();
    let adapter = HeadlessAdapter::new(spool.clone(), crate::adapter_tests::canonical_report());
    struct Port;
    impl GoalEntrypointPort for Port {
        type Request = &'static str;
        type Outcome = Result<&'static str, &'static str>;
        fn start_goal(&mut self, request: Self::Request) -> Self::Outcome {
            assert_eq!(request, "verbatim\n goal");
            Ok("goal-1")
        }
    }
    assert_eq!(
        adapter.start_goal(&mut Port, "verbatim\n goal"),
        Ok("goal-1")
    );
    assert!(adapter.detect());
    adapter.install(&());
    assert!(!root.join(".receipts").exists());
    let event = |confidence, host| {
        NormalizedHostEvent::try_new(NormalizedHostEventInputs {
            event_id: NormalizedHostEventId::try_new("01J9ZZZZZZZZZZZZZZZZZZZZZZ").unwrap(),
            event_type: NormalizedHostEventType::UserGoalSubmitted,
            host,
            host_session_id: "ci".into(),
            project_id: None,
            occurred_at: at(),
            payload: OrchestrationJsonObjectV1::new(Default::default()),
            raw_ref: None,
            confidence,
        })
        .unwrap()
    };
    let inferred = event(
        NormalizedHostEventConfidence::Inferred,
        HostId::Headless.into(),
    );
    assert_eq!(
        adapter.emit_with_provenance(
            &inferred,
            NormalizedHostEventSourceClass::HostHook,
            EventProvenance::Inferred
        ),
        Err(HeadlessEventError::InvalidSource)
    );
    for provenance in [EventProvenance::Inferred, EventProvenance::CliSupplied] {
        assert_eq!(
            adapter.emit_with_provenance(
                &inferred,
                NormalizedHostEventSourceClass::Elicitation,
                provenance
            ),
            Ok(provenance)
        );
    }
    let bytes = std::fs::read(spool.path()).unwrap();
    assert_eq!(
        adapter.emit(&event(
            NormalizedHostEventConfidence::Observed,
            HostId::Headless.into()
        )),
        Err(HeadlessEventError::ObservedConfidence)
    );
    assert_eq!(
        adapter.emit(&event(
            NormalizedHostEventConfidence::Inferred,
            HostId::Codex.into()
        )),
        Err(HeadlessEventError::WrongHost)
    );
    assert_eq!(std::fs::read(spool.path()).unwrap(), bytes);
    let view = ClaudeCoreView {
        view_type: crate::claude::ClaudeCoreViewType::Dag,
        content: OrchestrationJsonObjectV1::new(Default::default()),
    };
    let claude = crate::ClaudeHostAdapter::new(
        Default::default(),
        spool,
        crate::adapter_tests::canonical_report(),
    );
    assert_eq!(
        adapter.present(&view).as_bytes(),
        claude.present(&view).as_bytes()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn economy_exact_counts_identity_and_escaping() {
    let snapshot = GraphSnapshot::try_new(
        "g\u{202e}\n",
        GraphVersionV1::try_new("1").unwrap(),
        at(),
        vec![
            GraphSnapshotNodeState::try_new("n", GraphNodeState::Running, None, None, None)
                .unwrap(),
        ],
        None,
        None,
    )
    .unwrap();
    let view = crate::EconomyRenderer::new(&snapshot);
    assert!(std::ptr::eq(view.snapshot(), &snapshot));
    assert_eq!(
        view.render(),
        "graph \"g\\u{202e}\\n\" version \"1\" digest null\nRUNNING: 1\n"
    );
}
