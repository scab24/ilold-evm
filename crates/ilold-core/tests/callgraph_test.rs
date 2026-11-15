use std::path::PathBuf;

use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::callgraph::types::CallKind;
use ilold_core::model::function::Visibility;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_staking_call_graph() {
    let parser = SolcFrontend;
    let mut project = parser.parse(&[fixture_path("staking/src/staking.sol")]).unwrap();
    project.rebuild_index();

    let staking = project.contracts.iter().find(|c| c.name == "Staking").unwrap();
    let cg = build_call_graph(&project, staking);

    println!("Call graph nodes:");
    for idx in cg.node_indices() {
        let node = &cg[idx];
        println!("  {}::{} (external={})", node.contract, node.function, node.is_external);
    }

    println!("\nCall graph edges:");
    for edge_idx in cg.edge_indices() {
        let (src, dst) = cg.edge_endpoints(edge_idx).unwrap();
        let edge = &cg[edge_idx];
        println!("  {}::{} → {}::{} ({:?}, count={})",
            cg[src].contract, cg[src].function,
            cg[dst].contract, cg[dst].function,
            edge.kind, edge.call_count);
    }

    let has_deposit_to_transfer = cg.edge_indices().any(|e| {
        let (src, dst) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "deposit"
            && cg[dst].function == "transferFrom"
            && cg[dst].is_external
    });
    assert!(has_deposit_to_transfer, "deposit should call stakingToken.transferFrom");

    let has_claim_to_transfer = cg.edge_indices().any(|e| {
        let (src, dst) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "claimRewards"
            && cg[dst].function == "transfer"
            && cg[dst].is_external
    });
    assert!(has_claim_to_transfer, "claimRewards should call rewardToken.transfer");

    let has_deposit_to_reward = cg.edge_indices().any(|e| {
        let (src, dst) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "deposit"
            && cg[dst].function == "rewardPerToken"
    });
    assert!(has_deposit_to_reward, "deposit should call rewardPerToken (from updateReward modifier)");

    let set_rate_external = cg.edge_indices().any(|e| {
        let (src, _) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "setRewardRate" && cg[e].kind == CallKind::External
    });
    assert!(!set_rate_external, "setRewardRate should have no external calls");
}

#[test]
fn test_simple_storage_no_calls() {
    let parser = SolcFrontend;
    let mut project = parser.parse(&[fixture_path("simple_storage/src/simple_storage.sol")]).unwrap();
    project.rebuild_index();

    let contract = &project.contracts[0];
    let cg = build_call_graph(&project, contract);

    assert_eq!(cg.edge_count(), 0, "SimpleStorage should have no call edges");
}

#[test]
fn test_solc_cross_contract_resolved() {
    let project = SolcFrontend
        .parse_project(&fixture_path("solc/cross"))
        .expect("parse cross fixture");
    let vault = project.contracts.iter().find(|c| c.name == "Vault").expect("Vault");
    let cg = build_call_graph(&project, vault);

    let resolved_to_ipool = cg
        .node_indices()
        .any(|i| cg[i].contract == "IPool" && cg[i].function == "supply");
    let placeholder = cg.node_indices().any(|i| cg[i].contract == "pool");

    assert!(resolved_to_ipool, "pool.supply() must resolve to IPool::supply");
    assert!(!placeholder, "no 'pool' placeholder node should remain");

    let safe_add = cg
        .node_indices()
        .find(|&i| cg[i].contract == "SafeMath" && cg[i].function == "safeAdd")
        .expect("amount.safeAdd must resolve to SafeMath::safeAdd");
    assert_eq!(
        cg[safe_add].visibility,
        Visibility::Internal,
        "resolved node must carry the real visibility (internal), not the External default"
    );

    let cast_edge = cg.edge_indices().any(|e| {
        let (src, dst) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "depositCast"
            && cg[dst].contract == "IPool"
            && cg[dst].function == "supply"
    });
    assert!(
        cast_edge,
        "IPool(addr).supply() casting must produce a depositCast → IPool::supply edge"
    );

    let struct_as_call = cg
        .node_indices()
        .any(|i| cg[i].function == "DepositInfo");
    assert!(
        !struct_as_call,
        "struct literal DepositInfo({{...}}) must not surface as a function call"
    );

    let nested_safeadd = cg.edge_indices().any(|e| {
        let (src, dst) = cg.edge_endpoints(e).unwrap();
        cg[src].function == "record"
            && cg[dst].contract == "SafeMath"
            && cg[dst].function == "safeAdd"
    });
    assert!(
        nested_safeadd,
        "calls nested inside a struct literal must still be collected"
    );
}
