//! State and revert flow across calls and storage pointers: library `storage` parameters
//! bound at the call site, transitive effects with their call chain, behaviors of inherited
//! functions in the derived contract, resolved reads and writes, and modifier inlining.
//! Fixture: tests/fixtures/solc/state_flow.
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::callgraph::types::CallKind;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::{BlockKind, CfgGraph, CfgStatement};
use ilold_core::model::contract::{ContractDef, ContractKind};
use ilold_core::model::function::FunctionDef;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::{PathTree, TerminalKind};
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::analysis::{EffectKind, FunctionBehavior};

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/state_flow");
        SolcFrontend.parse_project(&root).expect("parse state_flow fixture")
    })
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

fn contract(name: &str) -> &'static ContractDef {
    project().contracts.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name}"))
}

fn function(contract_name: &str, key: &str) -> &'static FunctionDef {
    contract(contract_name).functions.iter().find(|f| f.key == key).unwrap_or_else(|| panic!("{contract_name}::{key}"))
}

fn behavior(contract_name: &str, key: &str) -> &'static FunctionBehavior {
    analysis().sequence_analyses[contract_name]
        .functions
        .iter()
        .find(|b| b.name == key)
        .unwrap_or_else(|| panic!("behavior {contract_name}::{key}"))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// (item, via) of the transitive effects of one kind, sorted.
fn traced(b: &FunctionBehavior, kind: EffectKind) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = b
        .transitive_effects
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.item.clone(), e.via.clone()))
        .collect();
    out.sort();
    out
}

fn via(items: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> =
        items.iter().map(|(item, via)| (item.to_string(), strings(via))).collect();
    out.sort();
    out
}

fn cfg(contract_name: &str, key: &str) -> CfgGraph {
    CfgBuilder::build_with_project(function(contract_name, key), contract(contract_name), Some(project()))
}

fn tree(contract_name: &str, key: &str) -> PathTree {
    let c = contract(contract_name);
    build_path_tree(&cfg(contract_name, key), &c.name, key, &project().inherited_state_vars(c), &PruningConfig::default())
}

/// Targets of the assignments and storage writes of `stmts`.
fn written(stmts: &[CfgStatement]) -> Vec<&str> {
    stmts
        .iter()
        .filter_map(|s| match s {
            CfgStatement::Assignment { target, .. } => Some(target.as_str()),
            CfgStatement::StateWrite { variable, .. } => Some(variable.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn external_library_writes_land_on_the_bound_state_variables() {
    let b = behavior("Pool", "supply");
    // passing `_reserves` by reference is not a read of it
    assert!(b.state_reads.is_empty(), "{:?}", b.state_reads);
    assert!(b.state_writes.is_empty(), "{:?}", b.state_writes);
    assert_eq!(b.external_calls, strings(&["SupplyLib.executeSupply"]));

    assert_eq!(b.transitive_state_writes, strings(&["_reserves", "_usersConfig"]));
    assert_eq!(
        b.transitive_state_write_paths,
        strings(&["_reserves[].rate", "_reserves[].updatedAt", "_usersConfig[].data"])
    );
    assert_eq!(b.transitive_state_reads, strings(&["_usersConfig"]));
    assert_eq!(b.transitive_state_read_paths, strings(&["_usersConfig[].data"]));
    assert_eq!(b.transitive_events, strings(&["Supplied"]));
    assert_eq!(
        traced(b, EffectKind::StateWrite),
        via(&[
            ("_reserves[].rate", &["SupplyLib::executeSupply", "ReserveLib::touch"]),
            ("_reserves[].updatedAt", &["SupplyLib::executeSupply", "ReserveLib::touch"]),
            ("_usersConfig[].data", &["SupplyLib::executeSupply"]),
        ])
    );
    assert!(b.can_revert);
    assert!(b.revert_sources.is_empty(), "{:?}", b.revert_sources);
    assert_eq!(traced(b, EffectKind::Revert), via(&[("require(amount != 0)", &["SupplyLib::executeSupply"])]));
    assert!(!b.read_only);
}

#[test]
fn library_behaviors_report_writes_through_their_storage_parameters() {
    let b = behavior("SupplyLib", "executeSupply");
    assert!(b.state_writes.is_empty(), "{:?}", b.state_writes);
    assert_eq!(b.storage_param_writes, strings(&["userConfig.data"]));
    assert_eq!(b.storage_param_reads, strings(&["userConfig.data"]));
    assert_eq!(
        traced(b, EffectKind::StorageParamWrite),
        via(&[
            ("reservesData[].rate", &["ReserveLib::touch"]),
            ("reservesData[].updatedAt", &["ReserveLib::touch"]),
        ])
    );
    assert!(!b.read_only);
    assert!(b.can_revert);
    assert_eq!(b.revert_sources, strings(&["require(amount != 0)"]));
}

#[test]
fn using_for_internal_library_write_is_the_only_effect() {
    let b = behavior("Pool", "touch");
    assert!(b.state_writes.is_empty() && b.external_calls.is_empty());
    assert_eq!(
        traced(b, EffectKind::StateWrite),
        via(&[
            ("_reserves[].rate", &["ReserveLib::touch"]),
            ("_reserves[].updatedAt", &["ReserveLib::touch"]),
        ])
    );
    assert!(!b.read_only, "an internal library write is a state change");
    assert!(!b.can_revert);
}

#[test]
fn nested_library_calls_bind_through_every_level() {
    let b = behavior("Pool", "bumpDirect");
    assert_eq!(
        traced(b, EffectKind::StateWrite),
        via(&[("_reserves[].index", &["ReserveLib::bump", "ReserveLib::_accrue"])])
    );
    assert_eq!(
        traced(b, EffectKind::StateRead),
        via(&[
            ("_reserves[].index", &["ReserveLib::bump", "ReserveLib::_accrue"]),
            ("_reserves[].updatedAt", &["ReserveLib::bump"]),
        ])
    );
    assert!(b.can_revert);
    assert_eq!(traced(b, EffectKind::Revert), via(&[("revert Stale()", &["ReserveLib::bump"])]));
}

#[test]
fn storage_aliases_follow_reassignment_and_returned_pointers() {
    let b = behavior("Pool", "realias");
    assert_eq!(b.state_write_paths, strings(&["_otherReserves[].index", "_reserves[].rate"]));
    assert_eq!(b.state_writes, strings(&["_otherReserves", "_reserves"]));
    assert!(b.state_reads.is_empty(), "{:?}", b.state_reads);

    let b = behavior("Pool", "viaGetter");
    assert_eq!(b.state_write_paths, strings(&["_otherReserves[].index", "_reserves[].updatedAt"]));
}

#[test]
fn reverts_in_modifier_helpers_make_the_function_revertible() {
    let b = behavior("Pool", "setAdmin");
    assert_eq!(b.state_writes, strings(&["admin"]));
    assert!(b.can_revert);
    assert!(b.revert_sources.is_empty());
    assert_eq!(
        traced(b, EffectKind::Revert),
        via(&[("revert NotAdmin()", &["modifier onlyAdmin", "Pool::_onlyAdmin"])])
    );
    assert_eq!(traced(b, EffectKind::StateRead), via(&[("admin", &["modifier onlyAdmin", "Pool::_onlyAdmin"])]));
    // the path tree stays intra-procedural
    assert_eq!(analysis().path_trees[&("Pool".to_string(), "setAdmin".to_string())].stats.revert_paths, 0);
}

#[test]
fn reads_and_writes_follow_declarations_not_names() {
    let b = behavior("Pool", "shadow");
    assert!(b.state_writes.is_empty(), "{:?}", b.state_writes);
    assert!(b.state_reads.is_empty(), "{:?}", b.state_reads);
    assert!(b.read_only);

    let b = behavior("Pool", "capOnly");
    assert_eq!(b.state_reads, strings(&["totalSupplyCap"]));
    assert_eq!(b.state_read_paths, strings(&["totalSupplyCap"]));
    assert!(b.state_writes.is_empty());
    assert!(b.read_only);
}

#[test]
fn called_dependency_libraries_get_cfgs_and_propagate() {
    let cfgs = &analysis().cfgs;
    assert!(cfgs.contains_key(&("ExtCounter".to_string(), "increment".to_string())));
    assert!(cfgs.contains_key(&("ExtCounter".to_string(), "reset".to_string())));
    assert!(cfgs.contains_key(&("ExtCounter".to_string(), "decrement".to_string())), "reached from a modifier override");
    assert!(contract("ExtCounter").is_dependency);

    let hit = behavior("Pool", "hit");
    assert_eq!(traced(hit, EffectKind::StateWrite), via(&[("hits.value", &["ExtCounter::increment"])]));
    assert_eq!(traced(hit, EffectKind::StateRead), via(&[("hits.value", &["ExtCounter::increment"])]));
    assert!(!hit.read_only);

    let clear = behavior("Pool", "clear");
    assert_eq!(traced(clear, EffectKind::StateWrite), via(&[("hits.value", &["ExtCounter::reset"])]));
    assert!(clear.can_revert);
    assert_eq!(traced(clear, EffectKind::Revert), via(&[("revert Empty()", &["ExtCounter::reset"])]));
}

#[test]
fn free_functions_bound_with_using_for_resolve() {
    let free = project()
        .contracts
        .iter()
        .find(|c| c.functions.iter().any(|f| f.name == "addTo"))
        .expect("free function container");
    assert_eq!(free.kind, ContractKind::Library);
    let target = format!("{}::addTo", free.key());

    let cg = build_call_graph(project(), contract("FreeUser"));
    let edges: BTreeSet<(String, String, CallKind)> = cg
        .edge_indices()
        .map(|e| {
            let (s, t) = cg.edge_endpoints(e).unwrap();
            (cg[s].id(), cg[t].id(), cg[e].kind)
        })
        .collect();
    assert_eq!(edges, BTreeSet::from([("FreeUser::add".to_string(), target.clone(), CallKind::InternalLibrary)]));

    let b = behavior("FreeUser", "add");
    assert_eq!(traced(b, EffectKind::StateWrite), vec![("pools[].index".to_string(), vec![target])]);
}

#[test]
fn inherited_functions_dispatch_from_the_derived_contract() {
    let b = behavior("Token", "transfer");
    assert_eq!(b.transitive_external_calls, strings(&["hook.finalize"]));
    assert_eq!(b.transitive_state_writes, strings(&["moved"]));
    assert_eq!(traced(b, EffectKind::ExternalCall), via(&[("hook.finalize", &["Token::_transfer"])]));
    assert_eq!(
        traced(b, EffectKind::StateWrite),
        via(&[("moved", &["Token::_transfer", "TokenBase::_transfer"])])
    );

    let base = behavior("TokenBase", "transfer");
    assert!(base.transitive_external_calls.is_empty(), "{:?}", base.transitive_external_calls);
    assert_eq!(base.transitive_state_writes, strings(&["moved"]));
}

#[test]
fn post_placeholder_code_runs_after_every_return() {
    let g = cfg("Modded", "early");
    let returns: Vec<_> = g.node_indices().filter(|&n| g[n].kind == BlockKind::Return).collect();
    assert_eq!(returns.len(), 1, "one function exit");
    assert!(g[returns[0]].statements.is_empty(), "{:?}", g[returns[0]].statements);
    let exits: Vec<_> = g.node_indices().filter(|&n| written(&g[n].statements).contains(&"exited")).collect();
    assert_eq!(exits.len(), 1, "post-placeholder code is emitted once");

    let t = tree("Modded", "early");
    assert_eq!((t.stats.happy_paths, t.stats.revert_paths), (2, 0));
    for path in &t.paths {
        assert_eq!(path.terminal, TerminalKind::Return);
        assert_eq!(path.annotations.state_write_paths, strings(&["entered", "exited"]));
        let exit_id = g[exits[0]].id;
        assert!(path.nodes.iter().any(|n| n.block_id == exit_id), "path {} skips the post code", path.id);
    }

    // a revert in an outer modifier skips the inner modifier's post code
    let t = tree("Modded", "both");
    assert_eq!((t.stats.happy_paths, t.stats.revert_paths), (1, 1));
    for path in &t.paths {
        let expected: &[&str] = match path.terminal {
            TerminalKind::Revert => &[],
            _ => &["entered", "exited"],
        };
        assert_eq!(path.annotations.state_write_paths, strings(expected));
    }
}

/// `both` with its first modifier renamed to one that does not exist.
fn unresolved_project() -> Project {
    let mut p = project().clone();
    let modded = p.contracts.iter_mut().find(|c| c.name == "Modded").unwrap();
    let both = modded.functions.iter_mut().find(|f| f.key == "both").unwrap();
    assert_eq!(both.modifiers[0].name, "checked");
    both.modifiers[0].name = "missingGuard".into();
    p
}

#[test]
fn unresolved_modifier_keeps_the_others_and_is_marked() {
    let p = unresolved_project();
    let modded = p.contracts.iter().find(|c| c.name == "Modded").unwrap();
    let both = modded.functions.iter().find(|f| f.key == "both").unwrap();
    let g = CfgBuilder::build_with_project(both, modded, Some(&p));
    let stmts: Vec<&CfgStatement> = g.node_weights().flat_map(|b| &b.statements).collect();
    assert!(stmts.iter().any(|s| matches!(s, CfgStatement::UnresolvedModifier { name, .. } if name == "missingGuard")));
    let statements: Vec<CfgStatement> = stmts.iter().map(|s| (*s).clone()).collect();
    let writes: BTreeSet<&str> = written(&statements).into_iter().collect();
    assert_eq!(writes, BTreeSet::from(["entered", "exited"]), "tracked is still inlined");
    assert!(!stmts.iter().any(|s| matches!(s, CfgStatement::RequireCheck { .. })));

    let a = ProjectAnalysis::build(&p, &AnalysisParams::new(1));
    let b = a.sequence_analyses["Modded"].functions.iter().find(|b| b.name == "both").unwrap();
    assert_eq!(b.unresolved_modifiers, strings(&["missingGuard"]));
}

#[test]
fn narratives_carry_the_call_chain_and_revert_sources() {
    use ilold_core::narrative::function::build_function_narrative;

    let key = ("Pool".to_string(), "setAdmin".to_string());
    let behaviors = &analysis().sequence_analyses["Pool"].functions;
    let n = build_function_narrative(
        contract("Pool"),
        function("Pool", "setAdmin"),
        &analysis().path_trees[&key],
        &analysis().cfgs[&key],
        behaviors,
    );
    assert_eq!(n.revert_paths, 0);
    assert!(n.can_revert);
    let sources: Vec<(String, Vec<String>, String)> =
        n.revert_sources.iter().map(|e| (e.item.clone(), e.via.clone(), e.origin_contract.clone())).collect();
    assert_eq!(
        sources,
        [("revert NotAdmin()".to_string(), strings(&["modifier onlyAdmin", "Pool::_onlyAdmin"]), "Pool".to_string())]
    );

    let key = ("Pool".to_string(), "supply".to_string());
    let n = build_function_narrative(
        contract("Pool"),
        function("Pool", "supply"),
        &analysis().path_trees[&key],
        &analysis().cfgs[&key],
        behaviors,
    );
    let rate = n.transitive_state_writes.iter().find(|e| e.item == "_reserves[].rate").expect("rate");
    assert_eq!(rate.via, strings(&["SupplyLib::executeSupply", "ReserveLib::touch"]));
    assert_eq!(rate.origin_contract, "ReserveLib");
}

#[test]
fn storage_aliases_union_their_places_across_branches() {
    let b = behavior("Aliases", "branchAlias");
    assert_eq!(b.state_write_paths, strings(&["_a[].rate", "_b[].rate"]));

    let b = behavior("Aliases", "elseAlias");
    assert_eq!(b.state_write_paths, strings(&["_b[].index", "_c[].index"]));

    let b = behavior("Aliases", "revertingBranch");
    assert_eq!(b.state_write_paths, strings(&["_a[].rate"]));
}

#[test]
fn storage_aliases_reassigned_in_loops_reach_every_place() {
    let b = behavior("Aliases", "loopAlias");
    assert_eq!(
        b.state_write_paths,
        strings(&["_a[].index", "_a[].updatedAt", "_b[].index", "_b[].updatedAt"])
    );
}

#[test]
fn writes_nested_in_arguments_and_indexes_are_recorded() {
    let b = behavior("Aliases", "nested");
    assert_eq!(b.state_write_paths, strings(&["counter", "last", "nonce", "seq", "slots[]"]));
    assert_eq!(b.state_writes, strings(&["counter", "last", "nonce", "seq", "slots"]));
    assert_eq!(traced(b, EffectKind::StateWrite), via(&[]), "slots[] is also written directly");
}

#[test]
fn constants_and_immutables_are_not_storage() {
    let b = behavior("Aliases", "useConsts");
    assert!(b.state_reads.is_empty(), "{:?}", b.state_reads);
    assert!(b.state_read_paths.is_empty(), "{:?}", b.state_read_paths);
    assert!(b.read_only);

    let b = behavior("Aliases", "constructor");
    assert!(b.state_writes.is_empty(), "{:?}", b.state_writes);
    assert!(b.state_write_paths.is_empty(), "{:?}", b.state_write_paths);

    // an immutable used as a call target is not a read either
    let b = behavior("Aliases", "viewExternal");
    assert!(b.state_reads.is_empty(), "{:?}", b.state_reads);
}

#[test]
fn view_and_static_external_calls_keep_a_function_read_only() {
    let b = behavior("Aliases", "viewExternal");
    assert_eq!(b.external_calls, strings(&["ORACLE.price"]));
    assert!(b.read_only);

    let b = behavior("Aliases", "staticRaw");
    assert_eq!(b.external_calls, strings(&["t.staticcall"]));
    assert!(b.read_only);

    let b = behavior("Aliases", "pokeExternal");
    assert!(!b.read_only);
}

#[test]
fn mutual_recursion_gives_every_member_the_full_effects() {
    let ping = behavior("Cycle", "ping");
    let pong = behavior("Cycle", "pong");
    assert_eq!(ping.transitive_state_writes, strings(&["pongs", "side", "tails"]));
    assert_eq!(pong.transitive_state_writes, strings(&["pings", "side", "tails"]));
    assert_eq!(
        traced(pong, EffectKind::StateWrite),
        via(&[
            ("pings", &["Cycle::ping"]),
            ("side", &["Cycle::ping", "Cycle::_pingSide"]),
            ("tails", &["Cycle::_tail"]),
        ])
    );
    let side = traced(ping, EffectKind::StateWrite);
    assert!(side.contains(&("tails".to_string(), strings(&["Cycle::pong", "Cycle::_tail"]))), "{side:?}");
}

#[test]
fn deep_call_chains_are_not_truncated() {
    let b = behavior("Deep", "d0");
    let chain: Vec<String> = (1..=30).map(|i| format!("Deep::_d{i}")).collect();
    assert_eq!(traced(b, EffectKind::StateWrite), vec![("deep".to_string(), chain)]);
}

#[test]
fn inherited_functions_run_the_derived_modifier_override() {
    let derived = behavior("MarkDerived", "act");
    assert_eq!(derived.state_writes, strings(&["derivedMarks"]));
    assert_eq!(traced(derived, EffectKind::StateWrite), via(&[("marks.value", &["modifier mark", "ExtCounter::decrement"])]));
    let base = behavior("MarkBase", "act");
    assert_eq!(base.state_writes, strings(&["baseMarks"]));
}

#[test]
fn delete_and_push_through_storage_pointers_write_the_place() {
    let b = behavior("Aliases", "pointerOps");
    assert_eq!(b.state_write_paths, strings(&["_a[].rate", "list"]));
}

#[test]
fn internal_functions_with_storage_parameters_bind_the_argument() {
    let b = behavior("Aliases", "viaInternalParam");
    assert!(b.state_writes.is_empty(), "{:?}", b.state_writes);
    assert_eq!(traced(b, EffectKind::StateWrite), via(&[("_c[].index", &["Aliases::_bumpRef"])]));
    assert_eq!(behavior("Aliases", "_bumpRef").storage_param_writes, strings(&["r.index"]));
}

#[test]
fn a_modifier_with_two_placeholders_runs_the_body_twice() {
    let g = cfg("Aliases", "twiceRun");
    let writes = g.node_indices().flat_map(|n| written(&g[n].statements)).filter(|w| *w == "twice").count();
    assert_eq!(writes, 2);
    let t = tree("Aliases", "twiceRun");
    assert_eq!((t.stats.happy_paths, t.stats.revert_paths), (1, 0));
    assert_eq!(t.paths[0].annotations.state_writes, strings(&["twice", "twice"]));
}

#[test]
fn self_calls_through_this_are_external_and_not_followed() {
    let b = behavior("Aliases", "selfCall");
    assert_eq!(b.external_calls, strings(&["this.pokeSelf"]));
    assert!(b.transitive_state_writes.is_empty(), "{:?}", b.transitive_state_writes);
    assert!(!b.read_only);
}

#[test]
fn type_expressions_render_in_conditions() {
    let b = behavior("Aliases", "capped");
    assert_eq!(b.revert_sources, strings(&["require(v < type(uint256).max)"]));
    assert_eq!(b.preconditions, strings(&["v < type(uint256).max"]));
}

#[test]
fn pointers_survive_branches_that_all_leave_the_block() {
    for key in ["bothReturn", "bothTry"] {
        assert_eq!(behavior("Aliases", key).state_write_paths, strings(&["_a[].rate"]), "{key}");
    }
}

#[test]
fn unbounded_recursive_effects_are_marked_incomplete() {
    let b = behavior("Tree", "grow");
    assert!(b.effects_incomplete);
    assert!(b.transitive_state_write_paths.contains(&"root.x".to_string()), "{:?}", b.transitive_state_write_paths);
    assert!(!behavior("Cycle", "ping").effects_incomplete);
}
