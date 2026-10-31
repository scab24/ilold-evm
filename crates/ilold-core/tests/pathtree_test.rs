use std::path::PathBuf;

use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::TerminalKind;
use ilold_core::pathtree::walker::build_path_tree;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

// REQ-PT-5: set() must produce exactly 2 paths (1 happy + 1 revert)
#[test]
fn test_set_produces_2_paths() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();
    let contract = &project.contracts[0];
    let set_fn = contract.functions.iter().find(|f| f.name == "set").unwrap();
    let cfg = CfgBuilder::build(set_fn, contract);

    let pt = build_path_tree(&cfg, &contract.name, "set", &contract.state_vars, &PruningConfig::default());

    println!("set() paths:");
    for p in &pt.paths {
        let nodes: Vec<String> = p.nodes.iter().map(|n| format!("{:?}", n.block_kind)).collect();
        println!("  [{}] {:?} → {}", p.id, p.terminal, nodes.join(" → "));
    }

    assert_eq!(pt.stats.total_paths, 2, "set() should have exactly 2 paths");
    assert_eq!(pt.stats.happy_paths, 1);
    assert_eq!(pt.stats.revert_paths, 1);
}

// REQ-PT-6: transferFrom() must produce exactly 5 paths (1 happy + 4 revert)
#[test]
fn test_transfer_from_produces_5_paths() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("erc20/src/erc20.sol")]).unwrap();
    let contract = &project.contracts[0];
    let func = contract.functions.iter().find(|f| f.name == "transferFrom").unwrap();
    let cfg = CfgBuilder::build(func, contract);

    let pt = build_path_tree(&cfg, &contract.name, "transferFrom", &contract.state_vars, &PruningConfig::default());

    println!("transferFrom() paths:");
    for p in &pt.paths {
        let nodes: Vec<String> = p.nodes.iter().map(|n| format!("{:?}", n.block_kind)).collect();
        println!("  [{}] {:?} → {}", p.id, p.terminal, nodes.join(" → "));
    }

    assert_eq!(pt.stats.total_paths, 5, "transferFrom() should have exactly 5 paths");
    assert_eq!(pt.stats.happy_paths, 1);
    assert_eq!(pt.stats.revert_paths, 4);
}

// REQ-PT-4: paths should have annotations (external calls, state writes, events)
#[test]
fn test_path_annotations() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();
    let contract = &project.contracts[0];
    let set_fn = contract.functions.iter().find(|f| f.name == "set").unwrap();
    let cfg = CfgBuilder::build(set_fn, contract);

    let pt = build_path_tree(&cfg, &contract.name, "set", &contract.state_vars, &PruningConfig::default());

    // The happy path should have: state write (value) + event (ValueChanged)
    let happy = pt.paths.iter().find(|p| p.terminal == TerminalKind::Return).unwrap();
    println!("Happy path annotations: {:?}", happy.annotations);

    assert!(!happy.annotations.state_writes.is_empty(), "Happy path should write state");
    assert!(!happy.annotations.events_emitted.is_empty(), "Happy path should emit event");
    assert!(!happy.annotations.require_checks.is_empty(), "Happy path should have require check");

    // The revert path should have: require check but no state write
    let revert = pt.paths.iter().find(|p| p.terminal == TerminalKind::Revert).unwrap();
    assert!(revert.annotations.state_writes.is_empty(), "Revert path should NOT write state");
}

// REQ-PT-8: loops should be unrolled max 3 times
#[test]
fn test_loop_unrolling() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("uniswap_v2_pair/src/uniswap_v2_pair.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "UniswapV2Pair").unwrap();
    let sqrt_fn = contract.functions.iter().find(|f| f.name == "_sqrt").unwrap();
    let cfg = CfgBuilder::build(sqrt_fn, contract);

    let config = PruningConfig { max_loop_unroll: 3, ..PruningConfig::default() };
    let pt = build_path_tree(&cfg, &contract.name, "_sqrt", &contract.state_vars, &config);

    println!("_sqrt() paths: {} total, {} pruned", pt.stats.total_paths, pt.stats.paths_pruned);
    for p in &pt.paths {
        println!("  [{}] {:?} depth={}", p.id, p.terminal, p.depth);
    }

    // Should have some LoopCutoff paths (loop was stopped)
    let loop_cutoffs = pt.paths.iter().filter(|p| p.terminal == TerminalKind::LoopCutoff).count();
    println!("Loop cutoff paths: {loop_cutoffs}");

    // _sqrt has a while loop — with max_unroll=3, we should get cutoffs
    assert!(loop_cutoffs > 0, "_sqrt should have loop cutoff paths");
}

// Interface functions should produce 0 paths
#[test]
fn test_interface_function_no_paths() {
    let parser = SolcFrontend;
    let project = parser.parse(&[fixture_path("staking/src/staking.sol")]).unwrap();
    let ierc20 = project.contracts.iter().find(|c| c.name == "IERC20").unwrap();
    let transfer = ierc20.functions.iter().find(|f| f.name == "transfer").unwrap();
    let cfg = CfgBuilder::build(transfer, ierc20);

    let pt = build_path_tree(&cfg, "IERC20", "transfer", &ierc20.state_vars, &PruningConfig::default());

    assert_eq!(pt.stats.total_paths, 0, "Interface function should have 0 paths");
}
