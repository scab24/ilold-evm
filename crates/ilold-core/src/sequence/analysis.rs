use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::contract::ContractDef;
use crate::model::project::Project;
use crate::pathtree::types::{InternalCallTarget, PathTree, TerminalKind};
use crate::util::is_type_cast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionBehavior {
    pub name: String,
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
        });
    }

    behaviors.sort_by(|a, b| a.name.cmp(&b.name));

    let mut transitions = Vec::new();

    for a in &behaviors {
        for b in &behaviors {
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

            if !shared.is_empty() || !conditions_affected.is_empty() || !a.external_calls.is_empty() || !b.external_calls.is_empty() {
                transitions.push(TransitionInfo {
                    from: a.name.clone(),
                    to: b.name.clone(),
                    shared_state: shared,
                    conditions_affected,
                    has_external_in_from: !a.external_calls.is_empty(),
                    has_external_in_to: !b.external_calls.is_empty(),
                });
            }
        }
    }

    SequenceAnalysis {
        functions: behaviors,
        transitions,
    }
}


pub fn analyze_project(
    project: &Project,
    analyses: &mut HashMap<String, SequenceAnalysis>,
) {
    let all_funcs: Vec<(String, String)> = analyses
        .iter()
        .flat_map(|(c, sa)| sa.functions.iter().map(move |b| (c.clone(), b.name.clone())))
        .collect();

    let snapshot: HashMap<String, SequenceAnalysis> = analyses.clone();

    for (contract_name, func_name) in all_funcs {
        let root_contract = match project.contract_index.get(&contract_name).map(|&i| &project.contracts[i]) {
            Some(c) => c,
            None => continue,
        };

        let root_behavior = match snapshot
            .get(&contract_name)
            .and_then(|sa| sa.functions.iter().find(|b| b.name == func_name))
        {
            Some(b) => b,
            None => continue,
        };

        let effects = collect_transitive_for_function(
            root_behavior,
            root_contract,
            project,
            &snapshot,
        );

        if let Some(sa) = analyses.get_mut(&contract_name) {
            if let Some(b) = sa.functions.iter_mut().find(|b| b.name == func_name) {
                b.transitive_state_writes = effects.state_writes;
                b.transitive_state_reads = effects.state_reads;
                b.transitive_state_write_paths = effects.state_write_paths;
                b.transitive_state_read_paths = effects.state_read_paths;
                b.transitive_external_calls = effects.external_calls;
                b.transitive_events = effects.events;
                b.transitive_internal_calls = effects.internal_calls;
            }
        }
    }
}

struct TransitiveSets {
    state_writes: Vec<String>,
    state_reads: Vec<String>,
    state_write_paths: Vec<String>,
    state_read_paths: Vec<String>,
    external_calls: Vec<String>,
    events: Vec<String>,
    internal_calls: Vec<String>,
}

fn collect_transitive_for_function(
    root: &FunctionBehavior,
    root_contract: &ContractDef,
    project: &Project,
    all: &HashMap<String, SequenceAnalysis>,
) -> TransitiveSets {
    let mut writes: HashSet<String> = HashSet::new();
    let mut reads: HashSet<String> = HashSet::new();
    let mut write_paths: HashSet<String> = HashSet::new();
    let mut read_paths: HashSet<String> = HashSet::new();
    let mut external: HashSet<String> = HashSet::new();
    let mut events: HashSet<String> = HashSet::new();
    let mut internal: HashSet<String> = HashSet::new();

    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut stack: Vec<(String, InternalCallTarget)> = call_targets(root)
        .into_iter()
        .map(|t| (root_contract.key().to_string(), t))
        .collect();

    while let Some((owner_name, target)) = stack.pop() {
        let callee_name = target.call.clone();
        let owner = match project.contract_index.get(&owner_name) {
            Some(&i) => &project.contracts[i],
            None => continue,
        };

        let (callee_behavior, owning) =
            match resolve_call_target(&target, owner, root_contract, project, all) {
                Some(x) => x,
                None => continue,
            };

        let key = (owning.clone(), callee_behavior.name.clone());
        if !visited.insert(key) {
            continue;
        }

        for w in &callee_behavior.state_writes {
            if !root.state_writes.contains(w) {
                writes.insert(w.clone());
            }
        }
        for r in &callee_behavior.state_reads {
            if !root.state_reads.contains(r) {
                reads.insert(r.clone());
            }
        }
        for p in &callee_behavior.state_write_paths {
            if !root.state_write_paths.contains(p) {
                write_paths.insert(p.clone());
            }
        }
        for p in &callee_behavior.state_read_paths {
            if !root.state_read_paths.contains(p) {
                read_paths.insert(p.clone());
            }
        }
        for c in &callee_behavior.external_calls {
            if !root.external_calls.contains(c) {
                external.insert(c.clone());
            }
        }
        for e in &callee_behavior.events {
            if !root.events.contains(e) {
                events.insert(e.clone());
            }
        }
        internal.insert(callee_name.clone());

        for next in call_targets(callee_behavior) {
            stack.push((owning.clone(), next));
        }
    }

    fn to_sorted(s: HashSet<String>) -> Vec<String> {
        let mut v: Vec<String> = s.into_iter().collect();
        v.sort();
        v
    }

    TransitiveSets {
        state_writes: to_sorted(writes),
        state_reads: to_sorted(reads),
        state_write_paths: to_sorted(write_paths),
        state_read_paths: to_sorted(read_paths),
        external_calls: to_sorted(external),
        events: to_sorted(events),
        internal_calls: to_sorted(internal),
    }
}

// Behaviors from older snapshots or hand-built models only carry the call names.
fn call_targets(behavior: &FunctionBehavior) -> Vec<InternalCallTarget> {
    if !behavior.internal_call_targets.is_empty() {
        return behavior.internal_call_targets.clone();
    }
    behavior
        .internal_calls
        .iter()
        .filter(|c| !is_type_cast(c))
        .map(|call| InternalCallTarget { call: call.clone(), declaration: None })
        .collect()
}

/// Behavior of the implementation an internal call reaches when `root` is the most derived
/// contract: `super.f` continues after `owner` in root's C3 order, an unqualified call to a
/// virtual function reaches the visible override, anything else is the declaration solc
/// resolved. Without a declaration, falls back to a lookup by name.
fn resolve_call_target<'a>(
    target: &InternalCallTarget,
    owner: &'a ContractDef,
    root: &'a ContractDef,
    project: &'a Project,
    all: &'a HashMap<String, SequenceAnalysis>,
) -> Option<(&'a FunctionBehavior, String)> {
    let behavior = |contract: &str, key: &str| {
        all.get(contract)?
            .functions
            .iter()
            .find(|b| b.name == key)
            .map(|b| (b, contract.to_string()))
    };
    let linearization = project.linearization(root);
    let in_root = linearization.iter().any(|c| c.key() == owner.key());
    let is_super = target.call.starts_with("super.");

    let Some((contract, key)) = &target.declaration else {
        if is_super {
            let name = &target.call["super.".len()..];
            let order = if in_root { linearization } else { project.linearization(owner) };
            let start = order.iter().position(|c| c.key() == owner.key())? + 1;
            return order[start..].iter().find_map(|c| behavior(c.key(), name));
        }
        return resolve_internal_callee(&target.call, owner, project, all);
    };
    let declared = project
        .contract_index
        .get(contract)
        .map(|&i| &project.contracts[i])
        .and_then(|c| c.functions.iter().find(|f| &f.key == key).map(|f| (c, f)));
    let Some((declared_in, def)) = declared else {
        return behavior(contract, key);
    };
    let signature = def.match_signature();

    let implementation = if is_super {
        let order = if in_root { linearization } else { project.linearization(owner) };
        project.implementation(&order, signature, Some(owner.key()))
    } else if !target.call.contains('.')
        && def.is_virtual
        && linearization.iter().any(|c| c.key() == declared_in.key())
    {
        project.implementation(&linearization, signature, None)
    } else {
        None
    };
    match implementation {
        Some((c, f)) => behavior(c.key(), &f.key),
        None => behavior(contract, key),
    }
}

pub fn resolve_internal_callee<'a>(
    callee: &str,
    starting_contract: &'a ContractDef,
    project: &'a Project,
    all: &'a HashMap<String, SequenceAnalysis>,
) -> Option<(&'a FunctionBehavior, String)> {
    // `super.f` continues after the caller; `Library.f` and `Base.f` name their contract
    if let Some((qualifier, name)) = callee.split_once('.') {
        let find = |contract: &str| {
            all.get(contract)?.functions.iter().find(|b| b.name == name).map(|b| (b, contract.to_string()))
        };
        if qualifier != "super" {
            return find(qualifier);
        }
        return project
            .linearization(starting_contract)
            .iter()
            .skip(1)
            .find_map(|c| find(c.key()));
    }

    // the first contract of the linearization that defines it
    project.linearization(starting_contract).into_iter().find_map(|c| {
        let sa = all.get(c.key())?;
        sa.functions.iter().find(|b| b.name == callee).map(|b| (b, c.key().to_string()))
    })
}
