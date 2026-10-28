//! WorkspaceHandle JSON data, separate from operational provisioning evidence.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{CommitSha, WorkspaceDateTimeV1, WorkspaceHandle};

/// Validated wire data. Decoding never observes Git or grants worktree authority.
/// Timestamps follow Workspace V1's lexical grammar, including year 0000 and
/// second 60; they do not establish leap-second occurrence or instant validity.
/// SHAs require exactly 40 ASCII lowercase hex bytes, even where a Python
/// schema validator's `$` anchor accepts a final LF. No input is normalized.
///
/// There is deliberately no conversion to an operational handle:
/// ```compile_fail
/// use receipts_workspace_execution::{WorkspaceHandle, WorkspaceHandleWire};
/// fn import(data: WorkspaceHandleWire) -> WorkspaceHandle { data.into() }
/// ```
/// Decoded records cannot authorize teardown:
/// ```compile_fail
/// use receipts_workspace_execution::{WorkspaceHandleWire, WorkspaceTeardownRequest};
/// fn remove(data: WorkspaceHandleWire, request: WorkspaceTeardownRequest) {
///     request.teardown(&data).unwrap();
/// }
/// ```
/// Operational handles cannot be deserialized directly:
/// ```compile_fail
/// use receipts_workspace_execution::WorkspaceHandle;
/// let _: WorkspaceHandle = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct WorkspaceHandleWire(Fields);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    workspace_id: String,
    branch: String,
    worktree_path: String,
    base_sha: String,
    state: String,
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
    head_sha: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    isolation: Option<WorkspaceHandleWireIsolation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    remote_publish_policy: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    created_at: Option<String>,
}

/// Optional isolation metadata is data, never a containment claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct WorkspaceHandleWireIsolation(IsolationFields);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IsolationFields {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    kind: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    sandbox_mode: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    network_access: Option<Option<bool>>,
}

// Missing uses the field default; present null is accepted only if T permits it.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

// Serde struct derives also admit sequences; JSON Schema requires objects.
fn object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(std::marker::PhantomData))
}

impl<'de> Deserialize<'de> for WorkspaceHandleWireIsolation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: IsolationFields = object(deserializer)?;
        if fields
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "WORKSPACE_ISOLATION")
        {
            return Err(serde::de::Error::custom("invalid isolation.kind"));
        }
        Ok(Self(fields))
    }
}

impl WorkspaceHandleWireIsolation {
    pub fn kind(&self) -> Option<&str> {
        self.0.kind.as_deref()
    }
    /// None = absent, Some(None) = explicit null, Some(Some(value)) = value.
    pub fn sandbox_mode(&self) -> Option<Option<&str>> {
        self.0.sandbox_mode.as_ref().map(|value| value.as_deref())
    }
    /// None = absent, Some(None) = explicit null, Some(Some(value)) = value.
    pub fn network_access(&self) -> Option<Option<bool>> {
        self.0.network_access
    }
}

impl<'de> Deserialize<'de> for WorkspaceHandleWire {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields: Fields = object(deserializer)?;
        fields.validate().map_err(serde::de::Error::custom)?;
        Ok(Self(fields))
    }
}

impl Fields {
    fn validate(&self) -> Result<(), &'static str> {
        if !(1..=200).contains(&self.workspace_id.chars().count()) {
            return Err("workspace_id must contain 1–200 Unicode scalars");
        }
        if self
            .task_id
            .as_ref()
            .is_some_and(|id| !(1..=200).contains(&id.chars().count()))
        {
            return Err("task_id must contain 1–200 Unicode scalars");
        }
        if self.branch.is_empty() {
            return Err("branch must be nonempty");
        }
        if self.worktree_path.is_empty() {
            return Err("worktree_path must be nonempty");
        }
        CommitSha::parse(&self.base_sha).map_err(|_| "invalid base_sha")?;
        if let Some(sha) = &self.head_sha {
            CommitSha::parse(sha).map_err(|_| "invalid head_sha")?;
        }
        if !matches!(
            self.state.as_str(),
            "PROVISIONED" | "ACTIVE" | "CHECKPOINTED" | "ORPHANED" | "RECOVERED" | "TORN_DOWN"
        ) {
            return Err("invalid state");
        }
        if let Some(policy) = &self.remote_publish_policy
            && !matches!(
                policy.as_str(),
                "LOCAL_ONLY" | "PUSH_ON_ACCEPT" | "PUSH_ALWAYS"
            )
        {
            return Err("invalid remote_publish_policy");
        }
        if let Some(time) = &self.created_at {
            WorkspaceDateTimeV1::try_new(time.clone()).map_err(|_| "invalid created_at")?;
        }
        Ok(())
    }
}

impl WorkspaceHandleWire {
    /// Decode strict JSON; unknown, duplicate, mistyped and invalid fields refuse.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
    /// Encode the validated record, preserving field values and presence.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
    pub fn workspace_id(&self) -> &str {
        &self.0.workspace_id
    }
    pub fn task_id(&self) -> Option<&str> {
        self.0.task_id.as_deref()
    }
    pub fn branch(&self) -> &str {
        &self.0.branch
    }
    pub fn worktree_path(&self) -> &str {
        &self.0.worktree_path
    }
    pub fn base_sha(&self) -> &str {
        &self.0.base_sha
    }
    pub fn head_sha(&self) -> Option<&str> {
        self.0.head_sha.as_deref()
    }
    pub fn state(&self) -> &str {
        &self.0.state
    }
    pub fn isolation(&self) -> Option<&WorkspaceHandleWireIsolation> {
        self.0.isolation.as_ref()
    }
    pub fn remote_publish_policy(&self) -> Option<&str> {
        self.0.remote_publish_policy.as_deref()
    }
    pub fn created_at(&self) -> Option<&str> {
        self.0.created_at.as_deref()
    }
}

/// A live handle cannot always be represented by the schema/JSON domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHandleExportError {
    IdentifierOutOfRange {
        field: &'static str,
        characters: usize,
    },
    NonUtf8WorktreePath,
}

impl std::fmt::Display for WorkspaceHandleExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentifierOutOfRange { field, characters } => write!(
                f,
                "{field} has {characters} Unicode scalars; wire requires 1–200"
            ),
            Self::NonUtf8WorktreePath => {
                f.write_str("worktree path is not UTF-8; lossless JSON export refused")
            }
        }
    }
}
impl std::error::Error for WorkspaceHandleExportError {}

impl TryFrom<&WorkspaceHandle> for WorkspaceHandleWire {
    type Error = WorkspaceHandleExportError;

    /// Export immutable observed evidence. No Git operation or lifecycle change.
    /// IDs outside 1–200 Unicode scalars and non-UTF-8 paths refuse explicitly.
    fn try_from(handle: &WorkspaceHandle) -> Result<Self, Self::Error> {
        for (field, value) in [
            ("workspace_id", Some(handle.workspace_id())),
            ("task_id", handle.task_id()),
        ] {
            if let Some(value) = value {
                let characters = value.chars().count();
                if !(1..=200).contains(&characters) {
                    return Err(WorkspaceHandleExportError::IdentifierOutOfRange {
                        field,
                        characters,
                    });
                }
            }
        }
        let path = handle
            .worktree_path()
            .to_str()
            .ok_or(WorkspaceHandleExportError::NonUtf8WorktreePath)?;
        Ok(Self(Fields {
            workspace_id: handle.workspace_id().into(),
            task_id: handle.task_id().map(str::to_owned),
            branch: handle.branch().into(),
            worktree_path: path.into(),
            base_sha: handle.base_sha().as_str().into(),
            head_sha: handle.head_sha().map(|sha| sha.as_str().into()),
            state: handle.state().as_str().into(),
            isolation: Some(WorkspaceHandleWireIsolation(IsolationFields {
                kind: Some(handle.isolation().as_str().into()),
                sandbox_mode: None,
                network_access: None,
            })),
            remote_publish_policy: handle
                .remote_publish_policy()
                .map(|policy| policy.as_str().into()),
            created_at: handle.created_at().map(|time| time.as_str().into()),
        }))
    }
}
