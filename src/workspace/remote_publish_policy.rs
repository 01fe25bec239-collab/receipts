/// Closed remote-publication policy vocabulary for workspaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRemotePublishPolicy {
    LocalOnly,
    PushOnAccept,
    PushAlways,
}

impl WorkspaceRemotePublishPolicy {
    pub const ALL: [Self; 3] = [Self::LocalOnly, Self::PushOnAccept, Self::PushAlways];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "LOCAL_ONLY",
            Self::PushOnAccept => "PUSH_ON_ACCEPT",
            Self::PushAlways => "PUSH_ALWAYS",
        }
    }
}
