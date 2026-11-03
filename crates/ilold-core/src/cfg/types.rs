use petgraph::stable_graph::StableDiGraph;
use serde::{Deserialize, Serialize};

use crate::model::common::SourceSpan;
use crate::model::decl_id::DeclId;
use crate::model::expression::{AssignOperator, CallKind};
use crate::model::source::ValueSource;

pub type CfgGraph = StableDiGraph<BasicBlock, BranchEdge>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: usize,
    pub kind: BlockKind,
    pub statements: Vec<CfgStatement>,
    pub span: Option<SourceSpan>,
    #[serde(default)]
    pub return_value: Option<String>,
    /// Conditions it branches on included, resolved through declaration ids and storage pointers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<BlockRead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert_reason: Option<String>,
    /// The modifier whose code reverts here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert_modifier: Option<String>,
}

/// A storage read and the modifier whose code makes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockRead {
    pub place: StorageRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_modifier: Option<String>,
}

/// With `param`, the path is rooted at the `storage` parameter with that index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<usize>,
}

impl StorageRef {
    pub fn state(path: impl Into<String>) -> Self {
        Self { path: path.into(), param: None }
    }

    pub(crate) fn extend(&self, suffix: &str) -> Self {
        Self { path: format!("{}{suffix}", self.path), param: self.param }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageArg {
    /// The receiver of a `using for` call is parameter 0.
    pub param: usize,
    pub place: StorageRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Entry,
    Normal,
    LoopCondition,
    Return,
    Revert,
    Assembly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BranchEdge {
    Unconditional,
    ConditionalTrue { condition: String },
    ConditionalFalse { condition: String },
    ExternalCallSuccess,
    ExternalCallFailure,
    LoopBack,
    LoopExit,
    CatchClause { kind: String },
}

impl BranchEdge {
    // traversal order: lower first, then target index
    pub fn variant_order(&self) -> u8 {
        match self {
            BranchEdge::Unconditional => 0,
            BranchEdge::ConditionalTrue { .. } => 1,
            BranchEdge::ConditionalFalse { .. } => 2,
            BranchEdge::ExternalCallSuccess => 3,
            BranchEdge::ExternalCallFailure => 4,
            BranchEdge::LoopBack => 5,
            BranchEdge::LoopExit => 6,
            BranchEdge::CatchClause { .. } => 7,
        }
    }
}

pub(crate) fn external_kind() -> CallKind { CallKind::External }
fn internal_kind() -> CallKind { CallKind::Internal }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CfgStatement {
    Assignment {
        target: String,
        value: String,
        operator: AssignOperator,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        /// `None` for locals and memory.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage: Option<StorageRef>,
    },
    ExternalCall {
        target: String,
        function: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        #[serde(default)]
        resolved: Option<DeclId>,
        #[serde(default)]
        arguments: String,
        #[serde(default = "external_kind")]
        kind: CallKind,
        #[serde(default)]
        contract: Option<String>,
        #[serde(default)]
        options: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        storage_args: Vec<StorageArg>,
        /// The receiver of a call to an address; none for library and creation calls.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_source: Option<ValueSource>,
        /// Low-level calls and `send` only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_used: Option<bool>,
    },
    InternalCall {
        function: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        #[serde(default)]
        arguments: String,
        #[serde(default = "internal_kind")]
        kind: CallKind,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        resolved: Option<DeclId>,
        #[serde(default)]
        declaration: Option<(String, String)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        storage_args: Vec<StorageArg>,
    },
    EmitEvent {
        event: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        #[serde(default)]
        arguments: String,
        /// The event declaration solc resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_id: Option<DeclId>,
    },
    StateRead {
        variable: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
    StateWrite {
        variable: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        /// `None` for state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        param: Option<usize>,
    },
    EthTransfer {
        to: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
    RequireCheck {
        condition: String,
        message: Option<String>,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
    AssertCheck {
        condition: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
    AssemblyBlock {
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
    /// Its code is unknown.
    UnresolvedModifier {
        name: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
    },
}

impl CfgStatement {
    pub fn origin(&self) -> (Option<SourceSpan>, Option<&str>) {
        match self {
            Self::Assignment { span, from_modifier, .. }
            | Self::ExternalCall { span, from_modifier, .. }
            | Self::InternalCall { span, from_modifier, .. }
            | Self::EmitEvent { span, from_modifier, .. }
            | Self::StateRead { span, from_modifier, .. }
            | Self::StateWrite { span, from_modifier, .. }
            | Self::EthTransfer { span, from_modifier, .. }
            | Self::RequireCheck { span, from_modifier, .. }
            | Self::AssertCheck { span, from_modifier, .. }
            | Self::AssemblyBlock { span, from_modifier }
            | Self::UnresolvedModifier { span, from_modifier, .. } => (*span, from_modifier.as_deref()),
        }
    }

    pub(crate) fn span_mut(&mut self) -> &mut Option<SourceSpan> {
        match self {
            Self::Assignment { span, .. }
            | Self::ExternalCall { span, .. }
            | Self::InternalCall { span, .. }
            | Self::EmitEvent { span, .. }
            | Self::StateRead { span, .. }
            | Self::StateWrite { span, .. }
            | Self::EthTransfer { span, .. }
            | Self::RequireCheck { span, .. }
            | Self::AssertCheck { span, .. }
            | Self::AssemblyBlock { span, .. }
            | Self::UnresolvedModifier { span, .. } => span,
        }
    }
}
