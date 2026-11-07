//! WorkspaceCheckpoint JSON data (decoder + encoder), separate from captured
//! operational checkpoint values.
//!
//! The wire record preserves exactly what the schema admits: optional-field
//! absence versus presence (including present-but-empty arrays), strings
//! byte-for-byte, and `exit_code` as its exact JSON number token (any
//! mathematically integral number, of any magnitude, e.g. `1e2` or
//! `-0`). `date-time` is a schema annotation here, so timestamp strings are
//! preserved without validation; converting to the operational value applies
//! the existing [`WorkspaceDateTimeV1`] validator. Conversion is explicitly
//! fallible and never changes the admitted wire value. Decoding observes no
//! Git, grants no authority and cannot move a [`crate::WorkspaceHandle`].

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

use crate::{
    CommitSha, WorkspaceCheckpointCaptureCore, WorkspaceCheckpointCheckSource,
    WorkspaceCheckpointCrashClassification, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointExecutedCheckResult, WorkspaceCheckpointKind, WorkspaceCheckpointRef,
    WorkspaceCheckpointRefType, WorkspaceCheckpointTemporalCaptureCore, WorkspaceDateTimeV1,
    WorkspaceRecoveryDecision, workspace_checkpoint_to_json,
};

/// Validated `WorkspaceCheckpoint` wire data.
///
/// Decoded data cannot become a handle or authorize any operation:
/// ```compile_fail
/// use receipts_workspace_execution::{WorkspaceCheckpointWire, WorkspaceHandle};
/// fn import(data: WorkspaceCheckpointWire) -> WorkspaceHandle { data.into() }
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct WorkspaceCheckpointWire(Fields);

impl PartialEq for WorkspaceCheckpointWire {
    fn eq(&self, other: &Self) -> bool {
        self.to_json().ok() == other.to_json().ok()
    }
}
impl Eq for WorkspaceCheckpointWire {}

// Field order equals the exporter's, so encoding exported data reproduces
// the exporter bytes exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    checkpoint_id: String,
    workspace_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    task_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    attempt_id: Option<String>,
    kind: String,
    head_sha: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    base_sha: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_object"
    )]
    dirty_diff_ref: Option<RefFields>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    modified_files: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    untracked_files: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_objects"
    )]
    executed_checks: Option<Vec<CheckFields>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    crash_classification: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    recovery_decision: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    recovery_rationale: Option<String>,
    captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckFields {
    source: String,
    command: Vec<String>,
    exit_code: Box<RawValue>,
    code_sha: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    timed_out: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    started_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    finished_at: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_object"
    )]
    output_ref: Option<RefFields>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefFields {
    ref_type: String,
    target: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    digest: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    section: Option<String>,
}

// Missing uses the default; a present value (null included) must satisfy T.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

// Nested records are objects too: a present value (null included) must be a
// JSON object, never a positional sequence a struct derive would also accept.
fn present_object<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Obj::<T>::deserialize(deserializer).map(|Obj(value)| Some(value))
}

fn present_objects<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Vec::<Obj<T>>::deserialize(deserializer)
        .map(|items| Some(items.into_iter().map(|Obj(value)| value).collect()))
}

/// Deserializes `T` only from a JSON object (see [`object`]).
struct Obj<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Obj<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        object(deserializer).map(Self)
    }
}

fn one_of<T: Copy>(value: &str, all: &[T], as_str: fn(T) -> &'static str) -> Option<T> {
    all.iter().copied().find(|item| as_str(*item) == value)
}

fn id_ok(value: &str) -> bool {
    (1..=200).contains(&value.chars().count())
}

impl RefFields {
    fn validate(&self) -> Result<(), &'static str> {
        one_of(
            &self.ref_type,
            &WorkspaceCheckpointRefType::ALL,
            WorkspaceCheckpointRefType::as_str,
        )
        .ok_or("invalid ref_type")?;
        if self.target.is_empty() {
            return Err("ref target must be nonempty");
        }
        Ok(())
    }
}

impl CheckFields {
    fn validate(&self) -> Result<(), &'static str> {
        one_of(
            &self.source,
            &WorkspaceCheckpointCheckSource::ALL,
            WorkspaceCheckpointCheckSource::as_str,
        )
        .ok_or("invalid executed check source")?;
        if self.command.is_empty() {
            return Err("command requires at least one argv element");
        }
        if !is_json_integer(self.exit_code.get()) {
            return Err("exit_code must be an integral JSON number");
        }
        CommitSha::parse(&self.code_sha).map_err(|_| "invalid code_sha")?;
        if let Some(reference) = &self.output_ref {
            reference.validate()?;
        }
        if let Some(result) = &self.result {
            one_of(
                result,
                &WorkspaceCheckpointExecutedCheckResult::ALL,
                WorkspaceCheckpointExecutedCheckResult::as_str,
            )
            .ok_or("invalid executed check result")?;
        }
        Ok(())
    }
}

impl Fields {
    fn validate(&self) -> Result<(), &'static str> {
        for (id, message) in [
            (
                Some(&self.checkpoint_id),
                "checkpoint_id must contain 1–200 characters",
            ),
            (
                Some(&self.workspace_id),
                "workspace_id must contain 1–200 characters",
            ),
            (
                self.task_id.as_ref(),
                "task_id must contain 1–200 characters",
            ),
            (
                self.attempt_id.as_ref(),
                "attempt_id must contain 1–200 characters",
            ),
        ] {
            if id.is_some_and(|id| !id_ok(id)) {
                return Err(message);
            }
        }
        one_of(
            &self.kind,
            &WorkspaceCheckpointKind::ALL,
            WorkspaceCheckpointKind::as_str,
        )
        .ok_or("invalid kind")?;
        CommitSha::parse(&self.head_sha).map_err(|_| "invalid head_sha")?;
        if let Some(sha) = &self.base_sha {
            CommitSha::parse(sha).map_err(|_| "invalid base_sha")?;
        }
        if let Some(reference) = &self.dirty_diff_ref {
            reference.validate()?;
        }
        for check in self.executed_checks.iter().flatten() {
            check.validate()?;
        }
        if let Some(value) = &self.crash_classification {
            one_of(
                value,
                &WorkspaceCheckpointCrashClassification::ALL,
                WorkspaceCheckpointCrashClassification::as_str,
            )
            .ok_or("invalid crash_classification")?;
        }
        if let Some(value) = &self.recovery_decision {
            one_of(
                value,
                &WorkspaceRecoveryDecision::ALL,
                WorkspaceRecoveryDecision::as_str,
            )
            .ok_or("invalid recovery_decision")?;
        }
        Ok(())
    }
}

// Serde struct derives also admit sequences; the schema requires objects.
fn object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(std::marker::PhantomData))
}

impl<'de> Deserialize<'de> for WorkspaceCheckpointWire {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: Fields = object(deserializer)?;
        fields.validate().map_err(serde::de::Error::custom)?;
        Ok(Self(fields))
    }
}

/// Exact wire value that has no faithful operational representation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkspaceCheckpointWireConversionError {
    /// `executed_checks[index].exit_code` is integral but outside `i64`.
    ExitCodeOutOfRange { index: usize },
    /// A timestamp string is outside the operational `WorkspaceDateTimeV1` domain.
    TimestampUnsupported { field: &'static str },
    /// An absent array would silently become present-empty operationally.
    ArrayAbsent { field: &'static str },
}

impl fmt::Display for WorkspaceCheckpointWireConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExitCodeOutOfRange { index } => {
                write!(f, "executed_checks[{index}].exit_code is outside i64")
            }
            Self::TimestampUnsupported { field } => {
                write!(f, "{field} is outside the operational timestamp domain")
            }
            Self::ArrayAbsent { field } => {
                write!(
                    f,
                    "{field} is absent; the operational value requires presence"
                )
            }
        }
    }
}
impl std::error::Error for WorkspaceCheckpointWireConversionError {}

/// An exact integer exit code token; [`Self::as_i64`] is the fallible
/// operational view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceCheckpointWireExitCode<'a>(&'a str);

impl<'a> WorkspaceCheckpointWireExitCode<'a> {
    /// The exact admitted JSON number token.
    pub fn as_json(&self) -> &'a str {
        self.0
    }
    /// The exact value when it fits `i64`; `None` otherwise (never rounded).
    pub fn as_i64(&self) -> Option<i64> {
        integral_to_i64(self.0)
    }
}

impl WorkspaceCheckpointWire {
    /// Decode strict JSON; unknown, duplicate, null, mistyped or invalid
    /// fields refuse.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
    /// Encode the validated record, preserving values, presence and the
    /// exact exit-code tokens.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
    pub fn checkpoint_id(&self) -> &str {
        &self.0.checkpoint_id
    }
    pub fn workspace_id(&self) -> &str {
        &self.0.workspace_id
    }
    pub fn task_id(&self) -> Option<&str> {
        self.0.task_id.as_deref()
    }
    pub fn attempt_id(&self) -> Option<&str> {
        self.0.attempt_id.as_deref()
    }
    pub fn kind(&self) -> &str {
        &self.0.kind
    }
    pub fn head_sha(&self) -> &str {
        &self.0.head_sha
    }
    pub fn base_sha(&self) -> Option<&str> {
        self.0.base_sha.as_deref()
    }
    /// `None` = absent; `Some(&[])` = present and empty.
    pub fn modified_files(&self) -> Option<&[String]> {
        self.0.modified_files.as_deref()
    }
    /// `None` = absent; `Some(&[])` = present and empty.
    pub fn untracked_files(&self) -> Option<&[String]> {
        self.0.untracked_files.as_deref()
    }
    /// Count of executed checks; `None` = the array is absent.
    pub fn executed_check_count(&self) -> Option<usize> {
        self.0.executed_checks.as_ref().map(Vec::len)
    }
    /// The exact exit code token of one executed check.
    pub fn executed_check_exit_code(
        &self,
        index: usize,
    ) -> Option<WorkspaceCheckpointWireExitCode<'_>> {
        self.0
            .executed_checks
            .as_ref()?
            .get(index)
            .map(|check| WorkspaceCheckpointWireExitCode(check.exit_code.get()))
    }
    pub fn crash_classification(&self) -> Option<&str> {
        self.0.crash_classification.as_deref()
    }
    pub fn recovery_decision(&self) -> Option<&str> {
        self.0.recovery_decision.as_deref()
    }
    pub fn recovery_rationale(&self) -> Option<&str> {
        self.0.recovery_rationale.as_deref()
    }
    pub fn captured_at(&self) -> &str {
        &self.0.captured_at
    }

    /// Converts to the operational checkpoint value when every field lies in
    /// the operational domain; otherwise reports the first unsupported value.
    /// The wire value is unchanged either way.
    pub fn to_temporal_capture_core(
        &self,
    ) -> Result<WorkspaceCheckpointTemporalCaptureCore, WorkspaceCheckpointWireConversionError>
    {
        use WorkspaceCheckpointWireConversionError as E;
        let fields = &self.0;
        let time = |value: &str, field| {
            WorkspaceDateTimeV1::try_new(value).map_err(|_| E::TimestampUnsupported { field })
        };
        let array =
            |value: &Option<Vec<String>>, field| value.clone().ok_or(E::ArrayAbsent { field });
        let mut checks = Vec::new();
        for (index, check) in fields
            .executed_checks
            .as_ref()
            .ok_or(E::ArrayAbsent {
                field: "executed_checks",
            })?
            .iter()
            .enumerate()
        {
            let exit_code =
                integral_to_i64(check.exit_code.get()).ok_or(E::ExitCodeOutOfRange { index })?;
            let started = check
                .started_at
                .as_deref()
                .map(|value| time(value, "executed_checks.started_at"))
                .transpose()?;
            let finished = check
                .finished_at
                .as_deref()
                .map(|value| time(value, "executed_checks.finished_at"))
                .transpose()?;
            let core = WorkspaceCheckpointExecutedCheckCore::new(
                one_of(
                    &check.source,
                    &WorkspaceCheckpointCheckSource::ALL,
                    WorkspaceCheckpointCheckSource::as_str,
                )
                .expect("validated"),
                check.command.clone(),
                exit_code,
                CommitSha::parse(&check.code_sha).expect("validated"),
                check.timed_out,
                check.output_ref.as_ref().map(RefFields::operational),
            )
            .expect("validated nonempty command")
            .with_result(check.result.as_deref().map(|result| {
                one_of(
                    result,
                    &WorkspaceCheckpointExecutedCheckResult::ALL,
                    WorkspaceCheckpointExecutedCheckResult::as_str,
                )
                .expect("validated")
            }))
            .with_started_at(started)
            .with_finished_at(finished);
            checks.push(core);
        }
        let captured_at = time(&fields.captured_at, "captured_at")?;
        let core = WorkspaceCheckpointCaptureCore::new(
            fields.checkpoint_id.clone(),
            fields.workspace_id.clone(),
            fields.task_id.clone(),
            fields.attempt_id.clone(),
            one_of(
                &fields.kind,
                &WorkspaceCheckpointKind::ALL,
                WorkspaceCheckpointKind::as_str,
            )
            .expect("validated"),
            CommitSha::parse(&fields.head_sha).expect("validated"),
            fields
                .base_sha
                .as_deref()
                .map(|sha| CommitSha::parse(sha).expect("validated")),
            fields.dirty_diff_ref.as_ref().map(RefFields::operational),
            array(&fields.modified_files, "modified_files")?,
            array(&fields.untracked_files, "untracked_files")?,
            checks,
            fields.recovery_decision.as_deref().map(|value| {
                one_of(
                    value,
                    &WorkspaceRecoveryDecision::ALL,
                    WorkspaceRecoveryDecision::as_str,
                )
                .expect("validated")
            }),
            fields.recovery_rationale.clone(),
        )
        .expect("validated identifiers")
        .with_crash_classification(fields.crash_classification.as_deref().map(|value| {
            one_of(
                value,
                &WorkspaceCheckpointCrashClassification::ALL,
                WorkspaceCheckpointCrashClassification::as_str,
            )
            .expect("validated")
        }));
        Ok(WorkspaceCheckpointTemporalCaptureCore::new(
            core,
            captured_at,
        ))
    }
}

impl RefFields {
    fn operational(&self) -> WorkspaceCheckpointRef {
        WorkspaceCheckpointRef::new(
            one_of(
                &self.ref_type,
                &WorkspaceCheckpointRefType::ALL,
                WorkspaceCheckpointRefType::as_str,
            )
            .expect("validated"),
            self.target.clone(),
            self.digest.clone(),
            self.section.clone(),
        )
        .expect("validated nonempty target")
    }
}

impl TryFrom<&WorkspaceCheckpointTemporalCaptureCore> for WorkspaceCheckpointWire {
    type Error = crate::WorkspaceCheckpointJsonExportError;

    /// Lossless export of a captured value (all three arrays present).
    fn try_from(checkpoint: &WorkspaceCheckpointTemporalCaptureCore) -> Result<Self, Self::Error> {
        let json = workspace_checkpoint_to_json(checkpoint)?;
        Self::from_json(&json).map_err(|_| crate::WorkspaceCheckpointJsonExportError)
    }
}

/// Whether one serde-admitted JSON token is a number with an integral value.
/// Local copy of the State crate's private algorithm (same mathematics,
/// independent tests); no State dependency is introduced.
pub(crate) fn is_json_integer(text: &str) -> bool {
    use std::cmp::Ordering;
    if !matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return false;
    }
    let (coefficient, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let coefficient = coefficient.strip_prefix('-').unwrap_or(coefficient);
    if coefficient.bytes().all(|b| matches!(b, b'0' | b'.')) {
        return true;
    }
    let fractional = coefficient.split_once('.').map_or(0, |(_, f)| f.len());
    let zeros = coefficient
        .bytes()
        .rev()
        .filter(|b| *b != b'.')
        .take_while(|b| *b == b'0')
        .count();
    let threshold_negative = zeros > fractional;
    let magnitude = fractional.abs_diff(zeros).to_string();
    let exponent_negative = exponent.starts_with('-');
    let digits = exponent
        .trim_start_matches(['+', '-'])
        .trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let exponent_negative = exponent_negative && digits != "0";
    if exponent_negative != threshold_negative {
        return !exponent_negative;
    }
    let order = digits
        .len()
        .cmp(&magnitude.len())
        .then_with(|| digits.cmp(&magnitude));
    if exponent_negative {
        order != Ordering::Greater
    } else {
        order != Ordering::Less
    }
}

/// Exact `i64` value of an integral JSON number token, or `None` when it is
/// not integral or does not fit. No floating point is involved.
pub(crate) fn integral_to_i64(text: &str) -> Option<i64> {
    if !is_json_integer(text) {
        return None;
    }
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (coefficient, exponent) = rest.split_once(['e', 'E']).unwrap_or((rest, "0"));
    let (int, frac) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    let digits: String = int.chars().chain(frac.chars()).collect();
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    let exponent_negative = exponent.starts_with('-');
    let exponent_digits = exponent
        .trim_start_matches(['+', '-'])
        .trim_start_matches('0');
    // A nonzero coefficient with a 20+ digit exponent can never fit in i64.
    if exponent_digits.len() > 19 {
        return None;
    }
    let exponent: i128 =
        exponent_digits.parse::<i128>().unwrap_or(0) * if exponent_negative { -1 } else { 1 };
    let scale = exponent - frac.len() as i128;
    let integer_digits: String = if scale >= 0 {
        if digits.len() as i128 + scale > 20 {
            return None;
        }
        let mut owned = digits.to_owned();
        owned.extend(std::iter::repeat_n('0', scale as usize));
        owned
    } else {
        // Integrality guarantees the dropped low digits are all zeros.
        let keep = digits.len() as i128 + scale;
        if keep <= 0 {
            return Some(0);
        }
        digits[..keep as usize].to_owned()
    };
    if integer_digits.len() > 20 {
        return None;
    }
    let magnitude: i128 = integer_digits.parse().ok()?;
    i64::try_from(if negative { -magnitude } else { magnitude }).ok()
}

#[cfg(test)]
#[path = "checkpoint_wire_tests.rs"]
mod tests;
