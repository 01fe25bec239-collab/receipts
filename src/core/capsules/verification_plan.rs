//! Planned argv and expectation only; never executed evidence.

use super::CapsuleError;

/// Explicit argv and a signed 64-bit expected exit code, without process-specific
/// exit-code bounds. Later argv entries may be empty and are never interpreted.
///
/// Construction cannot bypass argv validation:
/// ```compile_fail
/// use receipts_orchestration::capsules::VerificationPlanEntryV1;
/// let invalid = VerificationPlanEntryV1 { command: vec![], expected_exit_code: 0 };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPlanEntryV1 {
    command: Vec<String>,
    expected_exit_code: i64,
}

impl VerificationPlanEntryV1 {
    pub fn try_new(command: Vec<String>, expected_exit_code: i64) -> Result<Self, CapsuleError> {
        match command.first() {
            None => return Err(CapsuleError::VerificationArgvEmpty),
            Some(first) if first.is_empty() => return Err(CapsuleError::VerificationArgv0Empty),
            Some(_) => {}
        }
        Ok(Self {
            command,
            expected_exit_code,
        })
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn expected_exit_code(&self) -> i64 {
        self.expected_exit_code
    }
}
