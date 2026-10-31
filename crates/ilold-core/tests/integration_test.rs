use std::path::PathBuf;

use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::{BlockKind, CfgStatement};
use ilold_core::model::function::{FunctionKind, Visibility, Mutability};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::walker::build_path_tree;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_parse_simple_storage() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();

    // Should have 1 contract
    assert_eq!(project.contracts.len(), 1);
    let contract = &project.contracts[0];
    assert_eq!(contract.name, "SimpleStorage");

    // Should have 2 functions: get and set
    assert_eq!(contract.functions.len(), 2);

    let get_fn = contract.functions.iter().find(|f| f.name == "get").unwrap();
    assert_eq!(get_fn.visibility, Visibility::Public);
    assert_eq!(get_fn.mutability, Mutability::View);
    assert_eq!(get_fn.kind, FunctionKind::Function);
    assert_eq!(get_fn.returns.len(), 1);
    assert_eq!(get_fn.returns[0].type_name, "uint256");

    let set_fn = contract.functions.iter().find(|f| f.name == "set").unwrap();
    assert_eq!(set_fn.visibility, Visibility::Public);
    assert_eq!(set_fn.mutability, Mutability::NonPayable);
    assert_eq!(set_fn.params.len(), 1);
    assert_eq!(set_fn.params[0].name, "newValue");
    assert_eq!(set_fn.params[0].type_name, "uint256");

    // Should have 1 state variable
    assert_eq!(contract.state_vars.len(), 1);
    assert_eq!(contract.state_vars[0].name, "value");

    // Should have 1 event
    assert_eq!(contract.events.len(), 1);
    assert_eq!(contract.events[0].name, "ValueChanged");
}

#[test]
fn test_cfg_simple_get() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();
    let contract = &project.contracts[0];
    let get_fn = contract.functions.iter().find(|f| f.name == "get").unwrap();

    let cfg = CfgBuilder::build(get_fn, contract).unwrap();

    // get() just returns — should have Entry + Return (2 nodes minimum)
    let node_count = cfg.node_count();
    assert!(node_count >= 2, "Expected at least 2 nodes, got {node_count}");

    // Should have exactly one Entry and one Return
    let entries: Vec<_> = cfg.node_weights().filter(|b| b.kind == BlockKind::Entry).collect();
    let returns: Vec<_> = cfg.node_weights().filter(|b| b.kind == BlockKind::Return).collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(returns.len(), 1);
}

#[test]
fn test_cfg_set_with_require() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();
    let contract = &project.contracts[0];
    let set_fn = contract.functions.iter().find(|f| f.name == "set").unwrap();

    let cfg = CfgBuilder::build(set_fn, contract).unwrap();

    // set() has require(newValue > 0) which creates a branch:
    // Entry → [require check] → True path (assignment + emit + return)
    //                          → False path (revert)
    let node_count = cfg.node_count();
    assert!(node_count >= 4, "Expected at least 4 nodes, got {node_count}");

    // Must have at least one Revert node (from require failing)
    let reverts: Vec<_> = cfg.node_weights().filter(|b| b.kind == BlockKind::Revert).collect();
    assert!(!reverts.is_empty(), "Expected at least one Revert node");

    // Must have at least one Return node
    let returns: Vec<_> = cfg.node_weights().filter(|b| b.kind == BlockKind::Return).collect();
    assert!(!returns.is_empty(), "Expected at least one Return node");

    // Must have branch edges (from require)
    let edge_count = cfg.edge_count();
    assert!(edge_count >= 3, "Expected at least 3 edges, got {edge_count}");
}

#[test]
fn test_parse_file_not_found() {
    let parser = SolcFrontend;
    let result = parser.parse(&[PathBuf::from("nonexistent.sol")]);
    assert!(result.is_err());
}

fn state_access(fn_name: &str) -> (Vec<String>, Vec<String>) {
    let project = SolcFrontend.parse(&[fixture_path("solc/statements/src/Stmts.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "Stmts").unwrap();
    let f = contract.functions.iter().find(|f| f.name == fn_name).unwrap();
    let cfg = CfgBuilder::build(f, contract).unwrap();
    let tree = build_path_tree(
        &cfg,
        &contract.name,
        &f.name,
        &contract.state_vars,
        &PruningConfig::default(),
    );
    let reads: Vec<String> = tree.paths.iter().flat_map(|p| p.annotations.state_reads.clone()).collect();
    let writes: Vec<String> = tree.paths.iter().flat_map(|p| p.annotations.state_writes.clone()).collect();
    (reads, writes)
}

#[test]
fn reads_in_branch_condition_and_assert() {
    let (reads, _) = state_access("reads");
    assert!(reads.iter().any(|r| r == "rCond"), "state read in if-condition not detected");
    assert!(reads.iter().any(|r| r == "rAssert"), "state read in assert not detected");
}

#[test]
fn reads_via_compound_assignment_target() {
    let (reads, _) = state_access("compoundRead");
    assert!(reads.iter().any(|r| r == "rCompound"), "state read via compound assignment target not detected");
}

#[test]
fn constructor_is_named() {
    let project = SolcFrontend.parse_project(&fixture_path("solc/cross")).unwrap();
    let vault = project.contracts.iter().find(|c| c.name == "Vault").unwrap();
    let ctor = vault
        .functions
        .iter()
        .find(|f| matches!(f.kind, FunctionKind::Constructor))
        .expect("constructor present");
    assert_eq!(ctor.name, "constructor");
}

#[test]
fn array_push_is_not_an_external_call() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("solc/statements/src/Stmts.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "Stmts").unwrap();
    let push_fn = contract.functions.iter().find(|f| f.name == "pushItem").unwrap();

    let cfg = CfgBuilder::build(push_fn, contract).unwrap();
    let tree = build_path_tree(
        &cfg,
        &contract.name,
        &push_fn.name,
        &contract.state_vars,
        &PruningConfig::default(),
    );

    let has_external = tree.paths.iter().any(|p| !p.annotations.external_calls.is_empty());
    assert!(!has_external, "array push must not be classified as an external call");
}

#[test]
fn push_increment_and_delete_count_as_writes() {
    let (_, writes) = state_access("mutates");
    assert!(writes.iter().any(|w| w.starts_with("items")), "push not detected as write");
    assert!(writes.iter().any(|w| w == "total"), "increment not detected as write");
    assert!(writes.iter().any(|w| w == "delVar"), "delete not detected as write");
}

#[test]
fn array_pop_counts_as_write() {
    let (_, writes) = state_access("popItem");
    assert!(writes.iter().any(|w| w.starts_with("items")), "pop not detected as write");
}

#[test]
fn return_value_reads_are_detected() {
    let (reads, _) = state_access("getTotal");
    assert!(reads.iter().any(|r| r == "total"), "state read in return value not detected");
}

#[test]
fn tuple_return_reads_are_detected() {
    let (reads, _) = state_access("pair");
    assert!(reads.iter().any(|r| r == "total"), "tuple return read (total) not detected");
    assert!(reads.iter().any(|r| r == "rAssert"), "tuple return read (rAssert) not detected");
}

#[test]
fn local_variable_mutations_are_not_state_writes() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("solc/statements/src/Stmts.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "Stmts").unwrap();
    let f = contract.functions.iter().find(|f| f.name == "localOnly").unwrap();

    let cfg = CfgBuilder::build(f, contract).unwrap();
    let local_write = cfg.node_weights().any(|b| {
        b.statements
            .iter()
            .any(|s| matches!(s, CfgStatement::StateWrite { variable, .. } if variable == "tmp"))
    });

    assert!(!local_write, "mutation of a local variable must not emit a state write");
}

#[test]
fn write_after_external_call_is_observed() {
    use ilold_core::narrative::function::build_function_narrative;
    use ilold_core::narrative::types::ObservationKind;

    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("solc/statements/src/Stmts.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "Stmts").unwrap();
    let f = contract.functions.iter().find(|f| f.name == "cei").unwrap();

    let cfg = CfgBuilder::build(f, contract).unwrap();
    let tree = build_path_tree(
        &cfg,
        &contract.name,
        &f.name,
        &contract.state_vars,
        &PruningConfig::default(),
    );

    let narrative = build_function_narrative(contract, f, &tree, &cfg, &[]);
    let observed = narrative
        .observations
        .iter()
        .any(|o| matches!(o.kind, ObservationKind::WriteAfterExternalCall));
    assert!(observed, "write (assignment) after external call not surfaced as an observation");
}

#[test]
fn value_call_options_resolve_the_real_callee() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("solc/statements/src/Stmts.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "Stmts").unwrap();
    let f = contract.functions.iter().find(|f| f.name == "valueCall").unwrap();

    let cfg = CfgBuilder::build(f, contract).unwrap();
    let tree = build_path_tree(
        &cfg,
        &contract.name,
        &f.name,
        &contract.state_vars,
        &PruningConfig::default(),
    );

    let detects_call = tree.paths.iter().any(|p| p.annotations.external_calls.iter().any(|c| c.function == "call"));
    let leaks_placeholder = tree.paths.iter().any(|p| {
        p.annotations.internal_calls.iter().any(|c| c.contains("/*"))
            || p.annotations.external_calls.iter().any(|c| c.function.contains("/*"))
    });

    assert!(detects_call, "value-call `to.call{{value:}}` not detected as an external call");
    assert!(!leaks_placeholder, "FunctionCallOptions placeholder leaked into calls");
}

#[test]
fn is_type_cast_distinguishes_casts_from_calls() {
    use ilold_core::util::is_type_cast;

    // Elementary type casts (bare or with args)
    for c in ["uint256", "uint256(x)", "int", "address", "address(0)", "bytes32", "bool", "string"] {
        assert!(is_type_cast(c), "{c} should be a type cast");
    }
    // User-defined casts in Type(expr) form
    for c in ["IERC20(addr)", "MyContract(addr)"] {
        assert!(is_type_cast(c), "{c} should be a type cast");
    }
    // Real calls the old starts_with heuristic wrongly dropped
    for f in ["internalTransfer", "intMax", "uintToString", "addressOf", "stringify", "IMint", "IExecute", "foo(bar)"] {
        assert!(!is_type_cast(f), "{f} is a real call, not a type cast");
    }
}

#[test]
fn method_named_push_is_an_external_call_not_a_write() {
    let project = SolcFrontend.parse_project(&fixture_path("solc/cross")).unwrap();
    let vault = project.contracts.iter().find(|c| c.name == "Vault").unwrap();
    let f = vault.functions.iter().find(|f| f.name == "pushVia").unwrap();

    let cfg = CfgBuilder::build(f, vault).unwrap();
    let tree = build_path_tree(&cfg, &vault.name, &f.name, &vault.state_vars, &PruningConfig::default());

    let calls_push = tree.paths.iter().any(|p| p.annotations.external_calls.iter().any(|c| c.function == "push"));
    let writes_pool = tree.paths.iter().any(|p| p.annotations.state_writes.iter().any(|w| w == "pool"));

    assert!(calls_push, "pool.push() (resolved contract method) must be an external call");
    assert!(!writes_pool, "pool.push() must not be recorded as a write to `pool`");
}

#[test]
fn storage_pointer_alias_writes_attributed_to_state_var() {
    let (_, w_direct) = state_access("aliasWrite");
    assert!(w_direct.iter().any(|w| w.starts_with("info")), "write through storage alias not attributed to `info`");

    let (_, w_map) = state_access("aliasMappingWrite");
    assert!(w_map.iter().any(|w| w.starts_with("infos")), "write through mapping storage alias not attributed to `infos`");

    let (_, w_copy) = state_access("memoryCopyNotWrite");
    assert!(!w_copy.iter().any(|w| w.starts_with("info")), "memory copy must NOT count as a state write");
}

#[test]
fn reads_in_call_arguments_are_detected() {
    let (reads, _) = state_access("readViaCallArg");
    assert!(reads.iter().any(|r| r == "total"), "state var passed as a call argument not detected as a read");
}

#[test]
fn reads_in_emit_arguments_are_detected() {
    let (reads, _) = state_access("emitRead");
    assert!(reads.iter().any(|r| r == "total"), "state var in an emit argument not detected as a read");
}

#[test]
fn increment_on_assignment_rhs_is_a_write() {
    let (_, writes) = state_access("rhsIncrement");
    assert!(writes.iter().any(|w| w == "total"), "`x = total++` must count as a write to total");
}
