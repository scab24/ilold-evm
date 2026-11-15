use std::path::PathBuf;

use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::slicing::{
    build_slice_result, SliceDirection, SliceEntry, SliceResult, StatementOrigin,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join("tests/fixtures")
        .join(name)
}

fn slice(
    contract: &str,
    function: &str,
    variable: &str,
    direction: SliceDirection,
) -> SliceResult {
    let parser = SolcFrontend;
    let mut project = parser.parse(&[fixture_path("staking/src/staking.sol")]).unwrap();
    project.rebuild_index();

    let c = project
        .contracts
        .iter()
        .find(|c| c.name == contract)
        .unwrap_or_else(|| panic!("contract {contract} not found"));
    let f = c
        .functions
        .iter()
        .find(|f| f.name == function)
        .unwrap_or_else(|| panic!("function {function} not found"));

    build_slice_result(&project, c, f, variable, direction)
}

fn texts(entries: &[SliceEntry]) -> Vec<String> {
    entries.iter().map(|e| e.text.clone()).collect()
}

fn modifier_origin(entries: &[SliceEntry], substr: &str) -> Option<String> {
    entries.iter().find(|e| e.text.contains(substr)).and_then(|e| match &e.origin {
        StatementOrigin::Modifier(name) => Some(name.clone()),
        StatementOrigin::FunctionBody => None,
    })
}

#[test]
fn backward_slice_picks_up_transitive_assignment() {
    let res = slice("Staking", "deposit", "totalStaked", SliceDirection::Backward);
    let lines = texts(&res.backward);
    assert!(
        lines.iter().any(|l| l.contains("totalStaked")),
        "expected a totalStaked assignment, got {lines:?}"
    );
    assert!(res.forward.is_empty());
}

#[test]
fn forward_slice_propagates_taint_across_assignments() {
    let res = slice("Staking", "withdraw", "amount", SliceDirection::Forward);
    let lines = texts(&res.forward);
    assert!(
        lines.iter().any(|l| l.contains("balances")),
        "expected balances update in forward slice, got {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("totalStaked")),
        "expected totalStaked update in forward slice, got {lines:?}"
    );
    assert!(res.backward.is_empty());
}

#[test]
fn backward_slice_pulls_in_control_dependency() {
    let res = slice("Staking", "claimRewards", "rewards", SliceDirection::Backward);
    let lines = texts(&res.backward);
    assert!(
        lines.iter().any(|l| l.starts_with("if (")),
        "expected enclosing if(...) in backward slice, got {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("uint256 reward")),
        "expected `uint256 reward = rewards[..]` pulled in via control dep, got {lines:?}"
    );
}

#[test]
fn slice_on_unknown_variable_is_empty() {
    let res = slice("Staking", "deposit", "nonexistent_var", SliceDirection::Both);
    assert!(res.backward.is_empty(), "backward should be empty");
    assert!(res.forward.is_empty(), "forward should be empty");
}

#[test]
fn both_direction_populates_both_sides() {
    let res = slice("Staking", "withdraw", "amount", SliceDirection::Both);
    assert!(!res.forward.is_empty(), "forward should have entries");
    assert!(res.backward.is_empty(), "backward on parameter should be empty");
}

#[test]
fn backward_slice_pulls_writes_from_modifier() {
    let res = slice("Staking", "deposit", "lastUpdateTime", SliceDirection::Backward);
    let lines = texts(&res.backward);
    assert!(
        lines.iter().any(|l| l.contains("lastUpdateTime") && l.contains("block.timestamp")),
        "expected `lastUpdateTime = block.timestamp` from updateReward, got {lines:?}"
    );
    assert_eq!(
        modifier_origin(&res.backward, "lastUpdateTime"),
        Some("updateReward".to_string()),
        "the lastUpdateTime entry should be tagged as coming from updateReward"
    );
}

#[test]
fn modifier_writes_appear_before_function_body_in_program_order() {
    let res = slice("Staking", "deposit", "rewardPerTokenStored", SliceDirection::Forward);
    let lines = texts(&res.forward);
    if let Some(idx) = lines.iter().position(|l| l.contains("rewardPerTokenStored")) {
        let modifier_entry = &res.forward[idx];
        assert!(
            matches!(modifier_entry.origin, StatementOrigin::Modifier(ref n) if n == "updateReward"),
            "rewardPerTokenStored entry should be tagged as updateReward, got {:?}",
            modifier_entry.origin
        );
    } else {
        panic!("expected rewardPerTokenStored in forward slice, got {lines:?}");
    }
}

#[test]
fn slice_respects_program_order() {
    let res = slice("Staking", "deposit", "amount", SliceDirection::Forward);
    let lines = texts(&res.forward);
    let find = |needle: &str| lines.iter().position(|l| l.contains(needle));
    if let (Some(i_balances), Some(i_total), Some(i_emit)) = (
        find("balances"),
        find("totalStaked"),
        find("emit"),
    ) {
        assert!(
            i_balances < i_total && i_total < i_emit,
            "program order broken: balances@{i_balances}, total@{i_total}, emit@{i_emit} — {lines:?}"
        );
    } else {
        panic!("expected balances / totalStaked / emit in forward slice, got {lines:?}");
    }
}
