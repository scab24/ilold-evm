use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::classify::code_facts::BuiltinUse;
use crate::model::common::SourceSpan;
use crate::model::function::FunctionDef;
use crate::pathtree::types::{InternalCallTarget, PathAnnotations, PathTree};

use super::effects::{CfgSummary, Effects};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionBehavior {
    pub name: String,
    /// Function keys are unique only within the declaring contract.
    #[serde(default)]
    pub owner: String,
    pub preconditions: Vec<String>,
    pub state_writes: Vec<String>,
    pub state_reads: Vec<String>,
    #[serde(default)]
    pub state_write_paths: Vec<String>,
    #[serde(default)]
    pub state_read_paths: Vec<String>,
    pub external_calls: Vec<String>,
    #[serde(default)]
    pub internal_calls: Vec<String>,
    #[serde(default)]
    pub internal_call_targets: Vec<InternalCallTarget>,
    pub events: Vec<String>,
    pub can_revert: bool,
    pub always_reverts: bool,
    pub read_only: bool,

    #[serde(default)]
    pub transitive_state_writes: Vec<String>,
    #[serde(default)]
    pub transitive_state_reads: Vec<String>,
    #[serde(default)]
    pub transitive_state_write_paths: Vec<String>,
    #[serde(default)]
    pub transitive_state_read_paths: Vec<String>,
    #[serde(default)]
    pub transitive_external_calls: Vec<String>,
    #[serde(default)]
    pub transitive_events: Vec<String>,
    #[serde(default)]
    pub transitive_internal_calls: Vec<String>,

    #[serde(default)]
    pub revert_sources: Vec<String>,
    /// Normalised paths rooted at the parameter name.
    #[serde(default)]
    pub storage_param_writes: Vec<String>,
    #[serde(default)]
    pub storage_param_reads: Vec<String>,
    #[serde(default)]
    pub transitive_effects: Vec<EffectTrace>,
    #[serde(default)]
    pub unresolved_modifiers: Vec<String>,
    /// Assembly that may change state runs on some path, or the effects of a recursion whose written places keep growing were cut.
    #[serde(default)]
    pub effects_incomplete: bool,
    /// Reads may be missing: assembly in a `view` function runs, or the effects are incomplete altogether.
    #[serde(default)]
    pub reads_incomplete: bool,
    /// Assembly runs on some path: Yul can revert whatever the mutability, so the revert sources may miss some.
    #[serde(default)]
    pub reverts_incomplete: bool,
    #[serde(default)]
    pub guards: Vec<crate::classify::guards::GuardFact>,
    /// A helper chain deeper than the guard walk follows was cut.
    #[serde(default)]
    pub guards_incomplete: bool,
    /// Inline assembly in the function or its modifiers; its mutability tells what it may hide.
    #[serde(default)]
    pub assembly_blocks: Vec<SourceSpan>,
    #[serde(default)]
    pub builtin_uses: Vec<BuiltinUse>,
    #[serde(default)]
    pub unchecked_blocks: Vec<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    StateWrite,
    StateRead,
    StorageParamWrite,
    StorageParamRead,
    ExternalCall,
    Event,
    Revert,
}

/// `via` lists the callees from the analysed function to the one with the effect.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EffectTrace {
    pub kind: EffectKind,
    pub item: String,
    pub via: Vec<String>,
}

impl FunctionBehavior {
    pub fn effective_state_writes(&self) -> Vec<String> {
        let mut out: Vec<String> = self.state_writes.clone();
        for w in &self.transitive_state_writes {
            if !out.contains(w) { out.push(w.clone()); }
        }
        out.sort();
        out
    }

    pub fn effective_state_reads(&self) -> Vec<String> {
        let mut out: Vec<String> = self.state_reads.clone();
        for r in &self.transitive_state_reads {
            if !out.contains(r) { out.push(r.clone()); }
        }
        out.sort();
        out
    }

    pub fn effective_state_write_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self.state_write_paths.clone();
        for p in &self.transitive_state_write_paths {
            if !out.contains(p) { out.push(p.clone()); }
        }
        out.sort();
        out
    }

    pub fn effective_state_read_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self.state_read_paths.clone();
        for p in &self.transitive_state_read_paths {
            if !out.contains(p) { out.push(p.clone()); }
        }
        out.sort();
        out
    }

    pub fn effective_external_calls(&self) -> Vec<String> {
        let mut out: Vec<String> = self.external_calls.clone();
        for c in &self.transitive_external_calls {
            if !out.contains(c) { out.push(c.clone()); }
        }
        out.sort();
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionInfo {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub from_owner: String,
    #[serde(default)]
    pub to_owner: String,
    pub shared_state: Vec<String>,
    pub conditions_affected: Vec<String>,
    pub has_external_in_from: bool,
    pub has_external_in_to: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceAnalysis {
    pub functions: Vec<FunctionBehavior>,
    pub transitions: Vec<TransitionInfo>,
}

pub fn analyze_sequences(
    path_trees: &HashMap<(String, String), PathTree>,
    contract_name: &str,
) -> SequenceAnalysis {
    let functions = direct_behaviors(path_trees, contract_name);
    let transitions = transitions(&functions);
    SequenceAnalysis { functions, transitions }
}

/// From path trees, for callers without CFG summaries; see [`direct_behavior`].
pub fn direct_behaviors(
    path_trees: &HashMap<(String, String), PathTree>,
    contract_name: &str,
) -> Vec<FunctionBehavior> {
    let mut behaviors: Vec<FunctionBehavior> = path_trees
        .iter()
        .filter(|((c, f), _)| c == contract_name && !f.is_empty())
        .map(|((c, f), pt)| {
            let mut annotations = PathAnnotations::default();
            for block in pt.block_annotations() {
                annotations.extend(block);
            }
            direct_behavior(c, f, &annotations, pt.stats.happy_paths > 0, pt.stats.revert_paths > 0)
        })
        .collect();
    behaviors.sort_by(|a, b| a.name.cmp(&b.name));
    behaviors
}

/// `annotations` covers every block the function may run.
pub fn direct_behavior(owner: &str, key: &str, annotations: &PathAnnotations, returns: bool, reverts: bool) -> FunctionBehavior {
    let bases = |paths: &[String]| sorted(paths.iter().map(|p| base_name(p)));
    let state_writes = bases(&annotations.state_writes);
    let external_calls = sorted(annotations.external_calls.iter().map(|c| format!("{}.{}", c.target, c.function)));
    let read_only = state_writes.is_empty() && external_calls.is_empty();
    let internal_call_targets: BTreeSet<InternalCallTarget> = annotations.internal_call_targets.iter().cloned().collect();
    FunctionBehavior {
        name: key.to_string(),
        owner: owner.to_string(),
        preconditions: sorted(annotations.require_checks.iter().cloned()),
        state_writes,
        state_reads: bases(&annotations.state_reads),
        state_write_paths: sorted(annotations.state_write_paths.iter().cloned()),
        state_read_paths: sorted(annotations.state_read_paths.iter().cloned()),
        external_calls,
        internal_calls: sorted(annotations.internal_calls.iter().cloned()),
        internal_call_targets: internal_call_targets.into_iter().collect(),
        events: sorted(annotations.events_emitted.iter().cloned()),
        can_revert: reverts,
        always_reverts: !returns && reverts,
        read_only,
        transitive_state_writes: Vec::new(),
        transitive_state_reads: Vec::new(),
        transitive_state_write_paths: Vec::new(),
        transitive_state_read_paths: Vec::new(),
        transitive_external_calls: Vec::new(),
        transitive_events: Vec::new(),
        transitive_internal_calls: Vec::new(),
        revert_sources: Vec::new(),
        storage_param_writes: Vec::new(),
        storage_param_reads: Vec::new(),
        transitive_effects: Vec::new(),
        unresolved_modifiers: Vec::new(),
        effects_incomplete: false,
        reads_incomplete: false,
        reverts_incomplete: false,
        guards: Vec::new(),
        guards_incomplete: false,
        assembly_blocks: Vec::new(),
        builtin_uses: Vec::new(),
        unchecked_blocks: Vec::new(),
    }
}

/// Pairs where the first function changes something the second one uses.
pub fn transitions(behaviors: &[FunctionBehavior]) -> Vec<TransitionInfo> {
    behaviors.iter().flat_map(|a| behaviors.iter().filter_map(move |b| transition(a, b))).collect()
}

pub fn transition(a: &FunctionBehavior, b: &FunctionBehavior) -> Option<TransitionInfo> {
    let shared: Vec<String> = a.state_writes.iter()
        .filter(|w| b.state_reads.contains(*w) || b.state_writes.contains(*w))
        .cloned()
        .collect();

    let mut conditions_affected: Vec<String> = Vec::new();
    for w in &a.state_writes {
        for p in &b.preconditions {
            if p.contains(w.as_str()) {
                conditions_affected.push(format!("{} writes '{}' → {} needs require({})", a.name, w, b.name, p));
            }
        }
    }

    if shared.is_empty() && conditions_affected.is_empty() && a.external_calls.is_empty() && b.external_calls.is_empty() {
        return None;
    }
    Some(TransitionInfo {
        from: a.name.clone(),
        to: b.name.clone(),
        from_owner: a.owner.clone(),
        to_owner: b.owner.clone(),
        shared_state: shared,
        conditions_affected,
        has_external_in_from: !a.external_calls.is_empty(),
        has_external_in_to: !b.external_calls.is_empty(),
    })
}

pub fn find_behavior<'a>(behaviors: &'a [FunctionBehavior], owner: &str, key: &str) -> Option<&'a FunctionBehavior> {
    behaviors.iter().find(|b| b.is(owner, key))
}

impl FunctionBehavior {
    // behaviors without an owner come from older snapshots
    pub fn is(&self, owner: &str, key: &str) -> bool {
        self.name == key && (self.owner == owner || self.owner.is_empty())
    }
}

fn base_name(path: &str) -> String {
    crate::util::target_base_name(path).to_string()
}

fn sorted(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = items.into_iter().collect();
    set.into_iter().collect()
}

/// `effects` must be computed in the contract the behavior is listed under.
pub fn apply_effects(behavior: &mut FunctionBehavior, summary: &CfgSummary, effects: &Effects, func: &FunctionDef) {
    let b = behavior;
    let reached = |map: &std::collections::BTreeMap<String, Vec<String>>| -> Vec<String> {
        map.iter().filter(|(_, via)| !via.is_empty()).map(|(item, _)| item.clone()).collect()
    };
    let write_paths = reached(&effects.state_writes);
    let read_paths = reached(&effects.state_reads);
    b.transitive_state_writes =
        sorted(write_paths.iter().map(|p| base_name(p)).filter(|w| !b.state_writes.contains(w)));
    b.transitive_state_reads =
        sorted(read_paths.iter().map(|p| base_name(p)).filter(|r| !b.state_reads.contains(r)));
    b.transitive_state_write_paths =
        sorted(write_paths.into_iter().filter(|p| !b.state_write_paths.contains(p)));
    b.transitive_state_read_paths = sorted(read_paths.into_iter().filter(|p| !b.state_read_paths.contains(p)));
    b.transitive_external_calls =
        sorted(reached(&effects.external_calls).into_iter().filter(|c| !b.external_calls.contains(c)));
    b.transitive_events = sorted(reached(&effects.events).into_iter().filter(|e| !b.events.contains(e)));
    b.transitive_internal_calls = effects.internal_calls.iter().cloned().collect();
    b.transitive_effects = effects.traces();

    b.revert_sources = sorted(summary.reverts.iter().cloned());
    b.storage_param_writes = sorted(summary.param_writes.iter().map(|(_, p)| p.clone()));
    b.storage_param_reads = sorted(summary.param_reads.iter().map(|(_, p)| p.clone()));
    b.unresolved_modifiers = summary.unresolved_modifiers.clone();
    b.assembly_blocks = summary.assembly.clone();
    b.effects_incomplete = effects.incomplete;
    b.reads_incomplete = effects.reads_incomplete || effects.incomplete;
    b.reverts_incomplete = effects.reverts_incomplete;
    b.can_revert = b.can_revert || !effects.reverts.is_empty();
    // solc already rejects state changes in view and pure functions
    b.read_only = func.mutability.is_view_or_pure()
        || (effects.state_writes.is_empty()
            && effects.param_writes.is_empty()
            && effects.events.is_empty()
            && effects.state_changing_calls.is_empty());
}
