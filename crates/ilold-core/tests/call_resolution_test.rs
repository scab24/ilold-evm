//! Call resolution from compiler facts: declaration ids, C3 linearization, solc function
//! types and call options. Fixture: tests/fixtures/solc/call_resolution.
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::callgraph::types::CallKind;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::CfgStatement;
use ilold_core::model::contract::ContractDef;
use ilold_core::model::function::FunctionDef;
use ilold_core::model::project::{FunctionLookupError, Project};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::{InternalCallTarget, PathTree};
use ilold_core::sequence::analysis::FunctionBehavior;
use ilold_core::pathtree::walker::build_path_tree;

use CallKind::*;

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/solc/call_resolution");
        SolcFrontend.parse_project(&root).expect("parse call_resolution fixture")
    })
}

fn contract(name: &str) -> &'static ContractDef {
    project().contracts.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name}"))
}

fn function(contract_name: &str, key: &str) -> &'static FunctionDef {
    contract(contract_name)
        .functions
        .iter()
        .find(|f| f.key == key)
        .unwrap_or_else(|| panic!("{contract_name}::{key}"))
}

type Edge = (String, String, CallKind);

fn edges(contract_name: &str) -> BTreeSet<Edge> {
    let cg = build_call_graph(project(), contract(contract_name));
    cg.edge_indices()
        .map(|e| {
            let (s, t) = cg.edge_endpoints(e).unwrap();
            (cg[s].id(), cg[t].id(), cg[e].kind)
        })
        .collect()
}

fn expected(items: &[(&str, &str, CallKind)]) -> BTreeSet<Edge> {
    items.iter().map(|(a, b, k)| (a.to_string(), b.to_string(), *k)).collect()
}

fn tree(contract_name: &str, key: &str) -> PathTree {
    let c = contract(contract_name);
    let f = function(contract_name, key);
    let cfg = CfgBuilder::build_with_project(f, c, Some(project())).unwrap();
    let vars = project().inherited_state_vars(c);
    build_path_tree(&cfg, &c.name, &f.key, &vars, &PruningConfig::default())
}

fn internal_calls(t: &PathTree) -> BTreeSet<String> {
    t.paths.iter().flat_map(|p| p.annotations.internal_calls.iter().cloned()).collect()
}

fn external_calls(t: &PathTree) -> BTreeSet<(String, String, CallKind)> {
    t.paths
        .iter()
        .flat_map(|p| p.annotations.external_calls.iter())
        .map(|c| (c.target.clone(), c.function.clone(), c.kind))
        .collect()
}

#[test]
fn linearization_comes_from_solc() {
    let names = |c: &str| -> Vec<String> {
        project().linearization(contract(c)).iter().map(|c| c.name.clone()).collect()
    };
    assert_eq!(names("Leaf"), ["Leaf", "Middle", "Root"]);
    assert_eq!(names("D"), ["D", "C", "B", "A"]);
}

#[test]
fn is_override_reads_solc_overrides() {
    assert!(!function("Root", "_move").is_override);
    assert!(function("Middle", "_move").is_override);
    assert!(function("Leaf", "hook").is_override);
    assert!(!function("Leaf", "move").is_override);
    assert!(!function("A", "who").is_override);
    assert!(function("D", "who").is_override);
    assert!(!function("Over", "put(uint256)").is_override);
}

#[test]
fn overloads_get_signature_keys_and_selectors() {
    let over = contract("Over");
    let keys: Vec<&str> = over.functions.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["constructor", "put(uint256)", "put(address)", "receive", "fallback"]);
    assert_eq!(function("OverBase", "put").signature, "put(uint256,bool)");
    assert_eq!(function("Over", "put(uint256)").selector.as_deref(), Some("3f81a2c0"));
    assert!(function("Over", "put(address)").id.is_some());
}

#[test]
fn three_level_super_chain_hook_dispatch_and_inherited_modifier() {
    assert_eq!(
        edges("Leaf"),
        expected(&[
            ("Leaf::move", "Root::_track", Internal),
            ("Leaf::move", "Leaf::_move", Internal),
            ("Leaf::_move", "Middle::_move", Internal),
            ("Middle::_move", "Root::_move", Internal),
            ("Root::run", "Leaf::hook", Internal),
            ("Root::run", "Leaf::_move", Internal),
            ("Leaf::value", "Root::value", Internal),
            ("Leaf::value", "Leaf::extra", ExternalSelf),
        ])
    );
    assert_eq!(
        edges("Root"),
        expected(&[("Root::run", "Root::hook", Internal), ("Root::run", "Root::_move", Internal)])
    );
}

#[test]
fn dispatched_edges_record_the_static_target() {
    let cg = build_call_graph(project(), contract("Leaf"));
    let statics: BTreeSet<(String, String, Option<String>)> = cg
        .edge_indices()
        .map(|e| {
            let (s, t) = cg.edge_endpoints(e).unwrap();
            (cg[s].id(), cg[t].id(), cg[e].static_target.clone())
        })
        .filter(|(from, _, _)| from == "Root::run")
        .collect();
    assert_eq!(
        statics,
        BTreeSet::from([
            ("Root::run".into(), "Leaf::hook".into(), Some("Root::hook".into())),
            ("Root::run".into(), "Leaf::_move".into(), Some("Root::_move".into())),
        ])
    );
}

#[test]
fn diamond_super_follows_c3_of_the_most_derived_contract() {
    assert_eq!(
        edges("D"),
        expected(&[
            ("D::who", "C::who", Internal),
            ("C::who", "B::who", Internal),
            ("B::who", "A::who", Internal),
        ])
    );
    assert_eq!(edges("C"), expected(&[("C::who", "A::who", Internal)]));
}

#[test]
fn overloads_are_distinct_nodes() {
    assert_eq!(
        edges("Over"),
        expected(&[
            ("Over::put(address)", "Over::put(uint256)", Internal),
            ("Over::put(uint256)", "OverBase::put", Internal),
        ])
    );
    let cg = build_call_graph(project(), contract("Over"));
    let ids: BTreeSet<String> = cg.node_indices().map(|n| cg[n].id()).collect();
    for id in ["Over::constructor", "Over::receive", "Over::fallback", "Over::put(uint256)",
        "Over::put(address)", "OverBase::put"] {
        assert!(ids.contains(id), "missing node {id} in {ids:?}");
    }
}

#[test]
fn library_calls_split_internal_and_delegatecall() {
    assert_eq!(
        edges("UsesLib"),
        expected(&[
            ("UsesLib::viaUsing", "MathLib::double", InternalLibrary),
            ("UsesLib::direct", "MathLib::double", InternalLibrary),
            ("UsesLib::delegated", "MathLib::triple", LibraryDelegatecall),
        ])
    );
}

#[test]
fn external_low_level_value_and_creation_calls() {
    assert_eq!(
        edges("Caller"),
        expected(&[
            ("Caller::viaCast", "IToken::transfer", External),
            ("Caller::viaNestedCast", "IToken::balanceOf", External),
            ("Caller::viaState", "IToken::balanceOf", External),
            ("Caller::lowLevel", "address::call", LowLevelCall),
            ("Caller::lowLevel", "address::delegatecall", LowLevelDelegatecall),
            ("Caller::lowLevel", "address::staticcall", LowLevelStaticcall),
            ("Caller::pay", "address::transfer", Transfer),
            ("Caller::pay", "address::send", Send),
            ("Caller::create", "Child::constructor", Creation),
        ])
    );
    let cg = build_call_graph(project(), contract("Caller"));
    let create = cg
        .edge_indices()
        .find(|&e| cg[cg.edge_endpoints(e).unwrap().0].function == "create")
        .unwrap();
    assert_eq!(cg[create].call_count, 2);
}

#[test]
fn path_annotations_split_internal_and_external_calls() {
    let using = tree("UsesLib", "viaUsing");
    assert_eq!(internal_calls(&using), BTreeSet::from(["MathLib.double".to_string()]));
    assert!(external_calls(&using).is_empty());

    let delegated = tree("UsesLib", "delegated");
    assert!(internal_calls(&delegated).is_empty());
    assert_eq!(
        external_calls(&delegated),
        BTreeSet::from([("MathLib".into(), "triple".into(), LibraryDelegatecall)])
    );

    let value = tree("Leaf", "value");
    assert_eq!(internal_calls(&value), BTreeSet::from(["super.value".to_string()]));
    assert_eq!(
        external_calls(&value),
        BTreeSet::from([("this".into(), "extra".into(), ExternalSelf)])
    );

    let builtins = tree("Caller", "builtins");
    assert!(!builtins.paths.is_empty());
    assert!(internal_calls(&builtins).is_empty(), "{:?}", internal_calls(&builtins));
    assert!(external_calls(&builtins).is_empty(), "{:?}", external_calls(&builtins));

    let create = tree("Caller", "create");
    assert_eq!(
        external_calls(&create),
        BTreeSet::from([("Child".into(), "constructor".into(), Creation)])
    );
}

#[test]
fn call_options_are_kept() {
    let c = contract("Caller");
    let cfg = CfgBuilder::build_with_project(function("Caller", "lowLevel"), c, Some(project())).unwrap();
    let options: Vec<Vec<(String, String)>> = cfg
        .node_weights()
        .flat_map(|b| b.statements.iter())
        .filter_map(|s| match s {
            CfgStatement::ExternalCall { function, options, .. } if function == "call" => Some(options.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(options, [vec![("value".to_string(), "msg.value".to_string()), ("gas".into(), "5000".into())]]);

    let cfg = CfgBuilder::build_with_project(function("Caller", "create"), c, Some(project())).unwrap();
    let salted = cfg.node_weights().flat_map(|b| b.statements.iter()).any(|s| matches!(s,
        CfgStatement::ExternalCall { kind: Creation, options, .. }
            if options == &[("salt".to_string(), "salt".to_string())]));
    assert!(salted, "new Child{{salt: salt}}(2) must keep its salt option");
}

#[test]
fn analysis_maps_never_collide_on_overloads() {
    let analysis = ProjectAnalysis::build(project(), &AnalysisParams::new(2));
    for key in ["put(uint256)", "put(address)", "constructor", "receive", "fallback"] {
        let k = ("Over".to_string(), key.to_string());
        assert!(analysis.cfgs.contains_key(&k), "cfg {key}");
        assert!(analysis.path_trees.contains_key(&k), "paths {key}");
    }
    assert!(analysis.cfgs.contains_key(&("OverBase".to_string(), "put".to_string())));
    let pt = &analysis.path_trees[&("Over".to_string(), "put(address)".to_string())];
    assert_eq!(pt.function, "put(address)");
}

#[test]
fn function_lookup_accepts_unique_names_and_signatures() {
    let p = project();
    let over = contract("Over");
    match p.lookup_function(over, "put") {
        Err(FunctionLookupError::Ambiguous(c)) => {
            assert_eq!(c, ["put(uint256)", "put(address)", "put(uint256,bool)"])
        }
        other => panic!("expected ambiguity, got {:?}", other.map(|(c, f)| (&c.name, &f.key))),
    }
    let (owner, f) = p.lookup_function(over, "put(uint256,bool)").unwrap();
    assert_eq!((owner.name.as_str(), f.key.as_str()), ("OverBase", "put"));
    let (owner, f) = p.lookup_function(over, "put(address)").unwrap();
    assert_eq!((owner.name.as_str(), f.key.as_str()), ("Over", "put(address)"));
    let (owner, f) = p.lookup_function(contract("OverBase"), "put").unwrap();
    assert_eq!((owner.name.as_str(), f.key.as_str()), ("OverBase", "put"));
    assert!(matches!(p.lookup_function(over, "nope"), Err(FunctionLookupError::NotFound)));
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
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

#[test]
fn overloaded_internal_callees_keep_their_own_effects() {
    let t = tree("Ledger", "moveTwo");
    let targets: BTreeSet<InternalCallTarget> =
        t.paths.iter().flat_map(|p| p.annotations.internal_call_targets.iter().cloned()).collect();
    assert_eq!(
        targets,
        BTreeSet::from([InternalCallTarget {
            call: "_move".into(),
            declaration: Some(("Ledger".into(), "_move(uint256,bool)".into())),
        }])
    );

    let one = behavior("Ledger", "moveOne");
    assert_eq!(one.transitive_state_writes, strings(&["plain"]));
    assert!(one.transitive_external_calls.is_empty(), "{:?}", one.transitive_external_calls);

    let two = behavior("Ledger", "moveTwo");
    assert_eq!(two.transitive_state_writes, strings(&["settled"]));
    assert_eq!(two.transitive_external_calls, strings(&["sink.settle"]));
}

#[test]
fn transitive_super_follows_the_most_derived_linearization() {
    assert_eq!(behavior("SD", "f").transitive_state_writes, strings(&["sa", "sb", "sc"]));
    assert_eq!(behavior("SB", "f").transitive_state_writes, strings(&["sa"]));
    assert_eq!(behavior("SC", "f").transitive_state_writes, strings(&["sa"]));
}

#[test]
fn internal_overloads_by_data_location_have_distinct_keys() {
    let keys: Vec<&str> = contract("Ledger")
        .functions
        .iter()
        .filter(|f| f.name == "_sum")
        .map(|f| f.key.as_str())
        .collect();
    assert_eq!(keys, ["_sum(uint256[] calldata)", "_sum(uint256[] storage)"]);
    // public and external functions keep the ABI form
    assert_eq!(function("Ledger", "sums").signature, "sums(uint256[])");
    let sums: BTreeSet<Edge> =
        edges("Ledger").into_iter().filter(|(from, _, _)| from == "Ledger::sums").collect();
    assert_eq!(
        sums,
        expected(&[
            ("Ledger::sums", "Ledger::_sum(uint256[] calldata)", Internal),
            ("Ledger::sums", "Ledger::_sum(uint256[] storage)", Internal),
        ])
    );
    let (_, f) = project().lookup_function(contract("Ledger"), "_sum(uint256[]storage)").unwrap();
    assert_eq!(f.key, "_sum(uint256[] storage)");
}

#[test]
fn special_function_names_never_collide() {
    let regular = function("Pointers", "fallback()");
    assert_eq!(regular.signature, "fallback()");
    let special = function("Over", "fallback");
    assert_eq!((special.signature.as_str(), special.key.as_str()), ("fallback", "fallback"));
    let (_, found) = project().lookup_function(contract("Pointers"), "fallback").unwrap();
    assert_eq!(found.key, "fallback()");
    let (_, found) = project().lookup_function(contract("Over"), "receive").unwrap();
    assert_eq!(found.key, "receive");
}

#[test]
fn function_type_parameters_canonicalise_readably() {
    assert_eq!(
        function("Pointers", "withCallback").signature,
        "withCallback(function(uint256) external returns (bool))"
    );
}

#[test]
fn base_constructors_and_function_pointer_fields() {
    let child = edges("CtorChild");
    assert!(child.contains(&("CtorBase::constructor".into(), "CtorBase::_seed".into(), Internal)), "{child:?}");
    assert_eq!(
        edges("Pointers"),
        expected(&[("Pointers::applyOp", "?::op", Internal)])
    );
}
