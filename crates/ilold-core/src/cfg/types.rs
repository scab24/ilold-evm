use petgraph::stable_graph::StableDiGraph;
use serde::{Deserialize, Serialize};

use crate::model::common::SourceSpan;
use crate::model::decl_id::DeclId;
use crate::model::expression::{AssignOperator, CallKind};

pub type CfgGraph = StableDiGraph<BasicBlock, BranchEdge>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: usize,
    pub kind: BlockKind,
    pub statements: Vec<CfgStatement>,
    pub span: Option<SourceSpan>,
    #[serde(default)]
    pub return_value: Option<String>,
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

fn external_kind() -> CallKind { CallKind::External }
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
        // contract named by the callee's static type (`contract IPool` -> IPool)
        #[serde(default)]
        contract: Option<String>,
        // call options as (name, value), e.g. ("value", "msg.value") or ("salt", "s")
        #[serde(default)]
        options: Vec<(String, String)>,
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
        // `super`, the library or the base contract named before the function
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        resolved: Option<DeclId>,
        // the declaration solc resolved, as (contract, function key)
        #[serde(default)]
        declaration: Option<(String, String)>,
    },
    EmitEvent {
        event: String,
        span: Option<SourceSpan>,
        #[serde(default)]
        from_modifier: Option<String>,
        #[serde(default)]
        arguments: String,
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
}
