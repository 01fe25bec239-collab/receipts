//! Closed operation vocabulary embedded in the frozen `GraphMutation` contract.

/// The exact operation kinds a graph mutation may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphMutationOperationKind {
    AddNode,
    AddEdge,
    SetNodeState,
    AttachResult,
    CancelNode,
    ExpandRepair,
}

impl GraphMutationOperationKind {
    /// Every operation kind in frozen schema order.
    pub const ALL: [Self; 6] = [
        Self::AddNode,
        Self::AddEdge,
        Self::SetNodeState,
        Self::AttachResult,
        Self::CancelNode,
        Self::ExpandRepair,
    ];

    /// The exact canonical schema string for this operation kind.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AddNode => "ADD_NODE",
            Self::AddEdge => "ADD_EDGE",
            Self::SetNodeState => "SET_NODE_STATE",
            Self::AttachResult => "ATTACH_RESULT",
            Self::CancelNode => "CANCEL_NODE",
            Self::ExpandRepair => "EXPAND_REPAIR",
        }
    }
}
