use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::function::FunctionDef;
use crate::pathtree::types::{InternalCallTarget, PathTree, TerminalKind};

use super::effects::{CfgSummary, Effects};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionBehavior {
    pub name: String,
    /// Declaring contract; function keys are unique only within it.
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
    /// Internal callees with the declaration solc resolved; used for transitive effects.
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

    /// Explicit revert points of the function itself (`require(c)`, `revert E()`).
    #[serde(default)]
    pub revert_sources: Vec<String>,
    /// Writes through the function's own `storage` parameters, as normalised paths rooted at
    /// the parameter name (`userConfig.data`, `reservesData[].rate`).
    #[serde(default)]
    pub storage_param_writes: Vec<String>,
    #[serde(default)]
    pub storage_param_reads: Vec<String>,
    /// Effects reached through callees, each with the call chain that reaches it.
    #[serde(default)]
    pub transitive_effects: Vec<EffectTrace>,
    /// Modifiers named by the function that resolve to no definition; not inlined.
    #[serde(default)]
    pub unresolved_modifiers: Vec<String>,
    /// The effects of a recursion whose written places keep growing were cut.
    #[serde(default)]
    pub effects_incomplete: bool,
    #[serde(default)]
    pub guards: Vec<crate::classify::guards::GuardFact>,
}

/// What a transitive effect is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    StateWrite,
    StateRead,
    /// A write through a `storage` parameter of the analysed (library) function.
    StorageParamWrite,
    StorageParamRead,
    ExternalCall,
    Event,
    Revert,
}

/// One effect reached through calls. `via` lists the callees from the analysed function to
/// the one that has the effect (`Contract::key`, or `modifier m` for a modifier body).
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

/// Behaviors of the functions of `contract_name` from their own paths, sorted by name.
pub fn direct_behaviors(
    path_trees: &HashMap<(String, String), PathTree>,
    contract_name: &str,
) -> Vec<FunctionBehavior> {
    let mut behaviors: Vec<FunctionBehavior> = Vec::new();

    for ((c, f), pt) in path_trees {
        if c != contract_name || f.is_empty() { continue; }

        let mut preconditions = HashSet::new();
        let mut state_writes = HashSet::new();
        let mut state_reads = HashSet::new();
        let mut state_write_paths = HashSet::new();
        let mut state_read_paths = HashSet::new();
        let mut external_calls = HashSet::new();
        let mut internal_calls = HashSet::new();
        let mut internal_call_targets = HashSet::new();
        let mut events = HashSet::new();
        let mut has_return = false;
        let mut has_revert = false;

        for path in &pt.paths {
            for check in &path.annotations.require_checks {
                preconditions.insert(check.clone());
            }
            for write in &path.annotations.state_writes {
                let base = write
                    .split(|c| c == '[' || c == '.')
                    .next()
                    .unwrap_or(write)
                    .to_string();
                state_writes.insert(base);
            }
            for write in &path.annotations.state_write_paths {
                state_write_paths.insert(write.clone());
            }
            for read in &path.annotations.state_reads {
                let base = read
                    .split(|c| c == '[' || c == '.')
                    .next()
                    .unwrap_or(read)
                    .to_string();
                state_reads.insert(base);
            }
            for read in &path.annotations.state_read_paths {
                state_read_paths.insert(read.clone());
            }
            for call in &path.annotations.external_calls {
                external_calls.insert(format!("{}.{}", call.target, call.function));
            }
            for ic in &path.annotations.internal_calls {
                internal_calls.insert(ic.clone());
            }
            for target in &path.annotations.internal_call_targets {
                internal_call_targets.insert(target.clone());
            }
            for event in &path.annotations.events_emitted {
                events.insert(event.clone());
            }
            match path.terminal {
                TerminalKind::Return => has_return = true,
                TerminalKind::Revert => has_revert = true,
                _ => {}
            }
        }

        let read_only = state_writes.is_empty() && external_calls.is_empty();

        let mut state_writes: Vec<String> = state_writes.into_iter().collect();
        state_writes.sort();
        let mut state_reads: Vec<String> = state_reads.into_iter().collect();
        state_reads.sort();
        let mut state_write_paths: Vec<String> = state_write_paths.into_iter().collect();
        state_write_paths.sort();
        let mut state_read_paths: Vec<String> = state_read_paths.into_iter().collect();
        state_read_paths.sort();
        let mut external_calls: Vec<String> = external_calls.into_iter().collect();
        external_calls.sort();
        let mut internal_calls: Vec<String> = internal_calls.into_iter().collect();
        internal_calls.sort();
        let mut internal_call_targets: Vec<InternalCallTarget> = internal_call_targets.into_iter().collect();
        internal_call_targets.sort();
        let mut preconditions: Vec<String> = preconditions.into_iter().collect();
        preconditions.sort();
        let mut events: Vec<String> = events.into_iter().collect();
        events.sort();

        behaviors.push(FunctionBehavior {
            name: f.clone(),
            owner: c.clone(),
            preconditions,
            state_writes,
            state_reads,
            state_write_paths,
            state_read_paths,
            external_calls,
            internal_calls,
            internal_call_targets,
            events,
            can_revert: has_revert,
            always_reverts: !has_return && has_revert,
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
            guards: Vec::new(),
        });
    }

    behaviors.sort_by(|a, b| a.name.cmp(&b.name));
    behaviors
}

/// Ordered pairs of functions where the first one changes something the second one uses.
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
    let set: std::collections::BTreeSet<String> = items.into_iter().collect();
    set.into_iter().collect()
}

/// Completes a behavior with its own CFG summary and the effects reached through calls.
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
    b.effects_incomplete = effects.incomplete;
    b.can_revert = b.can_revert || !effects.reverts.is_empty();
    // solc already rejects state changes in view and pure functions
    b.read_only = func.mutability.is_view_or_pure()
        || (effects.state_writes.is_empty()
            && effects.param_writes.is_empty()
            && effects.events.is_empty()
            && effects.state_changing_calls.is_empty());
}
