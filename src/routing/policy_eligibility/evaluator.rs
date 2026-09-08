use super::ProviderPolicyEligibility;

/// Deterministic provider-policy hard filter. A true result passes only this
/// gate; authentication, availability, entitlement, quality and safety remain
/// independent requirements.
pub struct PolicyEligibilityEvaluator;

impl PolicyEligibilityEvaluator {
    /// Requires an allowed status and exact membership of the requested context.
    /// An actual deadline requires trustworthy caller evidence that it has not
    /// passed. Missing or null deadlines imply no expiry. A passed deadline
    /// has NeedsReview behavior for this evaluation without changing the record.
    pub fn evaluate(
        record: &ProviderPolicyEligibility,
        requested_execution_context: Option<&str>,
        reverification_deadline_passed: Option<bool>,
    ) -> bool {
        if !record.policy_status().passes_policy_gate_by_default() {
            return false;
        }
        let (Some(requested), Some(allowed)) = (
            requested_execution_context,
            record.allowed_execution_contexts(),
        ) else {
            return false;
        };
        if !allowed.iter().any(|context| context == requested) {
            return false;
        }
        record.reverification_deadline().flatten().is_none()
            || reverification_deadline_passed == Some(false)
    }
}
