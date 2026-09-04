//! Structural provider-auth axis evidence with optional open strings.

use super::dispatch_admission_vocabulary::DispatchAdmissionAxisResult;

/// Stores the supplied result and optional strings verbatim, without validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionProviderAuthAxisResult {
    result: DispatchAdmissionAxisResult,
    provider_id: Option<String>,
    technical_status: Option<String>,
}

impl DispatchAdmissionProviderAuthAxisResult {
    pub fn new(
        result: DispatchAdmissionAxisResult,
        provider_id: Option<String>,
        technical_status: Option<String>,
    ) -> Self {
        Self {
            result,
            provider_id,
            technical_status,
        }
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }

    pub fn technical_status(&self) -> Option<&str> {
        self.technical_status.as_deref()
    }
}
