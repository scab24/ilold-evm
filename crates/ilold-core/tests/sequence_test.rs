use std::path::PathBuf;

use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::builder::build_sequence_tree;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

fn build_staking_path_trees() -> (ilold_core::model::project::Project, Vec<ilold_core::pathtree::types::PathTree>) {
    let parser = SolcFrontend;
    let mut project = parser.parse(&[fixture_path("staking/src/staking.sol")]).unwrap();
    project.rebuild_index();

    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();
    let config = PruningConfig::default();

    let path_trees: Vec<_> = staking
        .functions
        .iter()
        .map(|f| {
            let cfg = CfgBuilder::build(f, staking);
            build_path_tree(&cfg, &staking.name, &f.name, &config)
        })
        .collect();

    (project, path_trees)
}

// REQ-SEQ-4: 8 functions, depth 2 → 72 sequences (8 + 64)
#[test]
fn test_staking_sequence_count_depth_2() {
    let (project, path_trees) = build_staking_path_trees();
    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();

    let st = build_sequence_tree(staking, &path_trees, 2);

    println!("Functions in sequence tree ({}):", st.functions.len());
    for (i, f) in st.functions.iter().enumerate() {
        println!("  [{}] {} (read_only={}, paths={})", i, f.name, f.read_only, f.path_count);
    }

    let sequences = st.page(2, 0, usize::MAX).sequences;
    let depth_1 = sequences.iter().filter(|s| s.depth == 1).count();
    let depth_2 = sequences.iter().filter(|s| s.depth == 2).count();
    println!("\nSequences: {} total (depth 1: {}, depth 2: {})", sequences.len(), depth_1, depth_2);

    assert_eq!(st.functions.len(), 8, "Should have 8 public/external functions (excluding constructor)");
    assert_eq!(depth_1, 8);
    assert_eq!(depth_2, 64);
    assert_eq!(sequences.len(), 72, "Total should be 8 + 64 = 72");
    assert_eq!(st.sequence_count(2), 72);
}

// REQ-SEQ-1: read_only flag correct
#[test]
fn test_read_only_flags() {
    let (project, path_trees) = build_staking_path_trees();
    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();

    let st = build_sequence_tree(staking, &path_trees, 1);

    let read_only: Vec<_> = st.functions.iter().filter(|f| f.read_only).collect();
    let state_changing: Vec<_> = st.functions.iter().filter(|f| !f.read_only).collect();

    println!("Read-only: {:?}", read_only.iter().map(|f| &f.name).collect::<Vec<_>>());
    println!("State-changing: {:?}", state_changing.iter().map(|f| &f.name).collect::<Vec<_>>());

    // rewardPerToken (view) and earned (view) should be read_only
    assert_eq!(read_only.len(), 2, "Should have 2 read-only functions");
    assert_eq!(state_changing.len(), 6, "Should have 6 state-changing functions");
}

// REQ-SEQ-6: constructors excluded
#[test]
fn test_constructor_excluded() {
    let (project, path_trees) = build_staking_path_trees();
    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();

    let st = build_sequence_tree(staking, &path_trees, 1);

    let has_constructor = st.functions.iter().any(|f| f.name.is_empty() || f.name == "constructor");
    assert!(!has_constructor, "Constructor should not appear in sequences");
}

// REQ-SEQ-3: has_state_change correct
#[test]
fn test_has_state_change_flag() {
    let (project, path_trees) = build_staking_path_trees();
    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();

    let st = build_sequence_tree(staking, &path_trees, 2);

    // A sequence of only read-only functions should have has_state_change = false
    let earned_idx = st.functions.iter().position(|f| f.name == "earned").unwrap();
    let reward_idx = st.functions.iter().position(|f| f.name == "rewardPerToken").unwrap();

    let sequences = st.page(2, 0, usize::MAX).sequences;
    let read_only_seq = sequences.iter().find(|s| {
        s.depth == 2 && s.steps == vec![earned_idx, reward_idx]
    });

    if let Some(seq) = read_only_seq {
        assert!(!seq.has_state_change, "earned → rewardPerToken should have no state change");
    }

    // A sequence with deposit should have has_state_change = true
    let deposit_idx = st.functions.iter().position(|f| f.name == "deposit").unwrap();
    let deposit_seq = sequences.iter().find(|s| {
        s.depth == 1 && s.steps == vec![deposit_idx]
    }).unwrap();

    assert!(deposit_seq.has_state_change, "deposit should have state change");
}

#[test]
fn sequences_are_generated_for_the_requested_depth_and_paged() {
    let (project, path_trees) = build_staking_path_trees();
    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();
    let st = build_sequence_tree(staking, &path_trees, 3);
    let n = st.functions.len();

    let shallow = st.page(1, 0, 100);
    assert_eq!((shallow.max_depth, shallow.page.total, shallow.page.truncated), (1, n, false));
    assert_eq!(shallow.sequences.len(), n);
    assert_eq!(st.sequence_count(3), n + n * n + n * n * n);

    let page = st.page(2, n, 3);
    let steps: Vec<Vec<usize>> = page.sequences.iter().map(|s| s.steps.clone()).collect();
    assert_eq!(steps, vec![vec![0, 0], vec![0, 1], vec![0, 2]]);
    assert!(page.sequences.iter().all(|s| s.depth == 2));
    assert_eq!((page.page.offset, page.page.limit, page.page.total, page.page.truncated), (n, 3, n + n * n, true));
    let expected = st.functions[0].path_count as u64 * st.functions[2].path_count as u64;
    assert_eq!(page.sequences[2].path_count, expected);
    assert!(st.page(2, n + n * n, 10).sequences.is_empty());

    let summary = st.page(3, 0, 0);
    assert!(summary.sequences.is_empty() && summary.page.truncated);
    assert_eq!(summary.functions.len(), n);
}

#[test]
fn interfaces_and_functions_without_bodies_have_no_sequences() {
    let (project, path_trees) = build_staking_path_trees();
    let ierc20 = project.contracts.iter().find(|c| c.name == "IERC20").unwrap();
    let st = build_sequence_tree(ierc20, &path_trees, 3);
    assert!(st.functions.is_empty(), "{:?}", st.functions);
    assert_eq!(st.sequence_count(3), 0);
    assert!(st.page(3, 0, 100).sequences.is_empty());
}
