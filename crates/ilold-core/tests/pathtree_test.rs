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
    for p in pt.paths() {
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
    for p in pt.paths() {
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
    let happy = pt.paths().find(|p| p.terminal == TerminalKind::Return).unwrap();
    println!("Happy path annotations: {:?}", happy.annotations);

    assert!(!happy.annotations.state_writes.is_empty(), "Happy path should write state");
    assert!(!happy.annotations.events_emitted.is_empty(), "Happy path should emit event");
    assert!(!happy.annotations.require_checks.is_empty(), "Happy path should have require check");

    // The revert path should have: require check but no state write
    let revert = pt.paths().find(|p| p.terminal == TerminalKind::Revert).unwrap();
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

    println!("_sqrt() paths: {} total, {:?}", pt.stats.total_paths, pt.stats);
    for p in pt.paths() {
        println!("  [{}] {:?} depth={}", p.id, p.terminal, p.depth);
    }

    // Should have some LoopCutoff paths (loop was stopped)
    let loop_cutoffs = pt.paths().filter(|p| p.terminal == TerminalKind::LoopCutoff).count();
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

fn erc20_transfer_from(config: &PruningConfig) -> ilold_core::pathtree::types::PathTree {
    let project = SolcFrontend.parse(&[fixture_path("erc20/src/erc20.sol")]).unwrap();
    let contract = &project.contracts[0];
    let func = contract.functions.iter().find(|f| f.name == "transferFrom").unwrap();
    let cfg = CfgBuilder::build(func, contract);
    build_path_tree(&cfg, &contract.name, "transferFrom", &contract.state_vars, config)
}

fn assert_totals(stats: &ilold_core::pathtree::types::PathTreeStats) {
    assert_eq!(
        stats.happy_paths + stats.revert_paths + stats.loop_cutoff + stats.depth_cutoff,
        stats.total_paths,
        "{stats:?}"
    );
    assert_eq!(stats.truncated, stats.cap_reached || stats.loop_cutoff + stats.depth_cutoff > 0, "{stats:?}");
}

#[test]
fn complete_enumeration_is_not_truncated() {
    let pt = erc20_transfer_from(&PruningConfig::default());
    assert_totals(&pt.stats);
    assert!(!pt.stats.truncated && !pt.stats.cap_reached, "{:?}", pt.stats);
    assert_eq!((pt.stats.loop_cutoff, pt.stats.depth_cutoff, pt.stats.paths_dropped), (0, 0, 0));
}

#[test]
fn path_cap_is_flagged_with_the_dropped_branches() {
    let pt = erc20_transfer_from(&PruningConfig { max_paths: 2, ..PruningConfig::default() });
    assert_totals(&pt.stats);
    assert_eq!(pt.stats.total_paths, 2);
    assert!(pt.stats.cap_reached && pt.stats.truncated, "{:?}", pt.stats);
    assert!(pt.stats.paths_dropped >= 1, "{:?}", pt.stats);
    assert_eq!(pt.paths().count(), 2);
}

#[test]
fn depth_and_loop_cutoffs_are_counted_apart() {
    let shallow = erc20_transfer_from(&PruningConfig { max_depth: 1, ..PruningConfig::default() });
    assert_totals(&shallow.stats);
    assert!(shallow.stats.depth_cutoff > 0 && shallow.stats.truncated, "{:?}", shallow.stats);
    assert_eq!(shallow.stats.loop_cutoff, 0);
    assert!(!shallow.stats.cap_reached);

    let project = SolcFrontend.parse(&[fixture_path("uniswap_v2_pair/src/uniswap_v2_pair.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "UniswapV2Pair").unwrap();
    let sqrt_fn = contract.functions.iter().find(|f| f.name == "_sqrt").unwrap();
    let cfg = CfgBuilder::build(sqrt_fn, contract);
    let looped = build_path_tree(&cfg, &contract.name, "_sqrt", &contract.state_vars, &PruningConfig::default());
    assert_totals(&looped.stats);
    let cutoffs = looped.paths().filter(|p| p.terminal == TerminalKind::LoopCutoff).count();
    assert_eq!(looped.stats.loop_cutoff, cutoffs);
    assert!(cutoffs > 0 && looped.stats.truncated && looped.stats.depth_cutoff == 0, "{:?}", looped.stats);
}

#[test]
fn paths_page_carries_ids_annotations_and_a_truncation_marker() {
    let pt = erc20_transfer_from(&PruningConfig::default());
    let all: Vec<_> = pt.paths().collect();
    assert_eq!(all.len(), 5);
    assert!(all.iter().enumerate().all(|(i, p)| p.id == i && p.depth + 1 == p.nodes.len()));

    let first = pt.page(0, 2);
    assert_eq!(first.paths.len(), 2);
    assert_eq!((first.page.offset, first.page.limit, first.page.total, first.page.truncated), (0, 2, 5, true));
    let last = pt.page(4, 10);
    assert_eq!(last.paths.len(), 1);
    assert!(!last.page.truncated);
    assert_eq!(last.paths[0].id, 4);
    assert_eq!(
        serde_json::to_value(&last.paths[0]).unwrap(),
        serde_json::to_value(&all[4]).unwrap()
    );
    assert!(pt.page(9, 10).paths.is_empty());
}

#[test]
fn stored_tree_round_trips_with_identical_paths() {
    let pt = erc20_transfer_from(&PruningConfig::default());
    let back: ilold_core::pathtree::types::PathTree =
        serde_json::from_slice(&serde_json::to_vec(&pt).unwrap()).unwrap();
    let json = |t: &ilold_core::pathtree::types::PathTree| serde_json::to_value(t.paths().collect::<Vec<_>>()).unwrap();
    assert_eq!(json(&back), json(&pt));
}

#[test]
fn narrative_carries_path_stats_and_counts_the_paths_it_omits() {
    use ilold_core::narrative::function::build_function_narrative;

    let project = SolcFrontend.parse(&[fixture_path("uniswap_v2_pair/src/uniswap_v2_pair.sol")]).unwrap();
    let contract = project.contracts.iter().find(|c| c.name == "UniswapV2Pair").unwrap();
    let sqrt_fn = contract.functions.iter().find(|f| f.name == "_sqrt").unwrap();
    let cfg = CfgBuilder::build(sqrt_fn, contract);
    let config = PruningConfig { max_loop_unroll: 400, max_depth: 6400, ..PruningConfig::default() };
    let pt = build_path_tree(&cfg, &contract.name, "_sqrt", &contract.state_vars, &config);
    let total = pt.stats.total_paths;
    assert!(total > 100, "{:?}", pt.stats);

    let mut n = build_function_narrative(contract, sqrt_fn, &pt, &cfg, &[]);
    assert_eq!(n.path_stats, pt.stats);
    assert_eq!((n.paths.len(), n.paths_omitted), (total, 0));

    n.truncate_paths(100);
    assert_eq!((n.paths.len(), n.paths_omitted), (100, total - 100));
    assert!(n.paths.iter().enumerate().all(|(i, p)| p.id == i));
    n.truncate_paths(100);
    assert_eq!(n.paths_omitted, total - 100);
}
