//! Facts a code map exports: never dropped by path caps or depth caps. Fixture: tests/fixtures/solc/facts.
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::cfg::types::CfgStatement;
use ilold_core::classify::code_facts::Builtin;
use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::classify::guards::{GuardFact, GuardKind, GuardScope};
use ilold_core::model::common::SourceSpan;
use ilold_core::model::expression::CallKind;
use ilold_core::model::project::Project;
use ilold_core::model::source::ValueSource;
use ilold_core::narrative::function::build_function_narrative;
use ilold_core::narrative::types::ObservationKind;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::sequence::analysis::FunctionBehavior;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/facts")
}

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| SolcFrontend.parse_project(&root()).expect("parse facts fixture"))
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

fn behavior_in<'a>(analysis: &'a ProjectAnalysis, contract: &str, key: &str) -> &'a FunctionBehavior {
    analysis.sequence_analyses[contract]
        .functions
        .iter()
        .find(|b| b.name == key)
        .unwrap_or_else(|| panic!("behavior {contract}::{key}"))
}

fn behavior(contract: &str, key: &str) -> &'static FunctionBehavior {
    behavior_in(analysis(), contract, key)
}

fn access(contract: &str, func: &str) -> AccessLevel {
    analysis().classifications[contract].iter().find(|(n, _)| n == func).map(|(_, a)| a.clone()).unwrap()
}

/// The 1-based line of the first fixture line containing `needle`.
fn line_of(needle: &str) -> u32 {
    let source = std::fs::read_to_string(root().join("src/Facts.sol")).unwrap();
    source.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle}")) as u32 + 1
}

fn lines(spans: &[SourceSpan]) -> Vec<u32> {
    spans.iter().map(|s| s.start_line).collect()
}

/// (function, kind, target source, result used) of every external call in reachable code.
fn external_calls(contract: &str, key: &str) -> Vec<(String, CallKind, Option<ValueSource>, Option<bool>)> {
    let cfg = &analysis().cfgs[&(contract.to_string(), key.to_string())];
    cfg.node_weights()
        .flat_map(|b| &b.statements)
        .filter_map(|s| match s {
            CfgStatement::ExternalCall { function, kind, target_source, result_used, .. } => {
                Some((function.clone(), *kind, target_source.clone(), *result_used))
            }
            _ => None,
        })
        .collect()
}

fn access_guard(contract: &str, key: &str) -> GuardFact {
    let guards = &behavior(contract, key).guards;
    let found: Vec<&GuardFact> = guards.iter().filter(|g| g.kind.is_access()).collect();
    assert_eq!(found.len(), 1, "{contract}::{key}: {guards:?}");
    found[0].clone()
}

fn state(name: &str, contract: &str) -> ValueSource {
    ValueSource::StateVar { name: name.into(), key: Some(format!("src/Facts.sol:{contract}::{name}")) }
}

#[test]
fn path_cap_never_drops_direct_effects() {
    let mut params = AnalysisParams::new(1);
    params.pruning = PruningConfig { max_paths: 3, ..PruningConfig::default() };
    let capped = ProjectAnalysis::build(project(), &params);
    let tree = &capped.path_trees[&("PathCap".to_string(), "branches".to_string())];
    assert!(tree.stats.cap_reached, "{:?}", tree.stats);

    let b = behavior_in(&capped, "PathCap", "branches");
    assert_eq!(b.state_writes, ["a", "b", "c", "rare"], "{b:?}");
    assert!(b.state_write_paths.contains(&"rare".to_string()), "{:?}", b.state_write_paths);
    assert!(!b.read_only && !b.always_reverts);
}

/// (effects incomplete, reads incomplete)
fn opacity(key: &str) -> (bool, bool) {
    let b = behavior("Asm", key);
    (b.effects_incomplete, b.reads_incomplete)
}

#[test]
fn any_assembly_may_revert_unseen() {
    for key in ["pureAsm", "callsPure", "raw", "callsRaw", "writesAsm", "callsWrites", "guarded"] {
        assert!(behavior("Asm", key).reverts_incomplete, "{key}");
    }
    assert!(!behavior("Asm", "plain").reverts_incomplete);
    assert!(!behavior("Guards", "writeThenCheck").reverts_incomplete);
}

#[test]
fn assembly_is_an_explicit_fact_whose_opacity_follows_the_mutability() {
    assert_eq!(lines(&behavior("Asm", "pureAsm").assembly_blocks), [line_of("r := add(a, 1)")]);
    assert_eq!(opacity("pureAsm"), (false, false), "pure Yul neither reads nor writes state");
    assert_eq!(opacity("callsPure"), (false, false));

    assert_eq!(lines(&behavior("Asm", "raw").assembly_blocks), [line_of("r := sload(0)")]);
    assert_eq!(opacity("raw"), (false, true), "view Yul may read state");
    assert_eq!(opacity("callsRaw"), (false, true), "propagated from raw");

    assert_eq!(lines(&behavior("Asm", "writesAsm").assembly_blocks), [line_of("sstore(0, 1)")]);
    assert_eq!(opacity("writesAsm"), (true, true));
    let caller = behavior("Asm", "callsWrites");
    assert!(caller.assembly_blocks.is_empty());
    assert_eq!(opacity("callsWrites"), (true, true), "propagated from writesAsm");

    assert_eq!(lines(&behavior("Asm", "guarded").assembly_blocks), [line_of("let x := 1")], "the modifier's block");
    assert_eq!(opacity("guarded"), (true, true), "a modifier runs at the function's mutability");

    assert!(behavior("Asm", "plain").assembly_blocks.is_empty());
    assert_eq!(opacity("plain"), (false, false));
}

#[test]
fn a_check_after_a_write_still_restricts() {
    let g = access_guard("Guards", "writeThenCheck");
    assert_eq!((g.kind, g.scope), (GuardKind::CallerEquals, GuardScope::Unconditional));
    assert_eq!(access("Guards", "writeThenCheck"), AccessLevel::Restricted { role: "require(msg.sender == owner)".into() });
}

#[test]
fn a_check_on_one_branch_is_conditional_and_does_not_restrict() {
    let g = access_guard("Guards", "oneBranch");
    assert_eq!((g.kind, g.scope), (GuardKind::CallerEquals, GuardScope::Conditional));
    assert_eq!(g.span.map(|s| s.start_line), Some(line_of("if (v > 10)") + 1));
    assert_eq!(access("Guards", "oneBranch"), AccessLevel::Public);
}

#[test]
fn a_function_guarded_only_conditionally_is_reported_without_access_control() {
    let contract = project().contracts.iter().find(|c| c.name == "Guards").unwrap();
    let func = contract.functions.iter().find(|f| f.key == "oneBranch").unwrap();
    let key = ("Guards".to_string(), "oneBranch".to_string());
    let a = analysis();
    let n = build_function_narrative(contract, func, &a.path_trees[&key], &a.cfgs[&key], &a.sequence_analyses["Guards"].functions);
    assert_eq!(n.access, AccessLevel::Public);
    assert!(n.observations.iter().any(|o| o.kind == ObservationKind::NoAccessControl), "{:?}", n.observations);
}

#[test]
fn guards_record_what_the_caller_is_compared_against() {
    assert_eq!(access_guard("Guards", "writeThenCheck").compared_against, Some(state("owner", "Guards")));
    assert_eq!(access_guard("Guards", "toCheck").compared_against, Some(ValueSource::Param { index: 0, name: "to".into() }));
    assert_eq!(access_guard("Guards", "viaGetter").compared_against, Some(state("owner", "Guards")), "through the getter");

    let role = access_guard("Guards", "roleCheck");
    assert_eq!(role.kind, GuardKind::RoleCheck);
    assert_eq!(
        role.compared_against,
        Some(ValueSource::Constant { name: "ADMIN".into(), key: Some("src/Facts.sol:Guards::ADMIN".into()) })
    );
    assert_eq!(role.checked_by.as_deref(), Some("hasRole"));
}

#[test]
fn checks_in_loops_and_try_clauses_are_conditional() {
    for key in ["inLoop", "inTry"] {
        let g = access_guard("Guards", key);
        assert_eq!((g.kind, g.scope), (GuardKind::CallerEquals, GuardScope::Conditional), "{key}");
        assert_eq!(access("Guards", key), AccessLevel::Public, "{key}");
    }
}

#[test]
fn a_role_passed_to_a_modifier_is_resolved_to_its_argument() {
    let g = access_guard("Guards", "roleViaModifier");
    assert_eq!((g.kind, g.scope), (GuardKind::RoleCheck, GuardScope::Unconditional));
    assert_eq!(g.via, ["modifier onlyRole"]);
    assert_eq!(
        g.compared_against,
        Some(ValueSource::Constant { name: "ADMIN".into(), key: Some("src/Facts.sol:Guards::ADMIN".into()) })
    );
    assert_eq!(g.checked_by.as_deref(), Some("hasRole"));
}

#[test]
fn a_modifier_checking_on_one_branch_never_falls_back_to_its_name() {
    let g = access_guard("Guards", "asmModifierBranch");
    assert_eq!((g.kind, g.scope, g.heuristic), (GuardKind::CallerEquals, GuardScope::Conditional, false));
    assert_eq!(access("Guards", "asmModifierBranch"), AccessLevel::Public);
}

#[test]
fn guard_depth_cap_marks_guards_incomplete() {
    assert!(behavior("Guards", "deep").guards_incomplete);
    assert!(!behavior("Guards", "writeThenCheck").guards_incomplete);
    assert!(!behavior("Guards", "oneBranch").guards_incomplete);
}

#[test]
fn external_calls_record_the_receiver_source() {
    let source = |key: &str| {
        let calls = external_calls("Targets", key);
        assert_eq!(calls.len(), 1, "{key}: {calls:?}");
        assert_eq!(calls[0].0, "supply");
        assert_eq!(calls[0].3, None, "not a low-level call");
        calls[0].2.clone()
    };
    assert_eq!(source("fromState"), Some(state("pool", "Targets")));
    assert_eq!(source("fromCastState"), Some(state("poolAddress", "Targets")));
    assert_eq!(
        source("fromImmutable"),
        Some(ValueSource::Immutable { name: "fixedPool".into(), key: Some("src/Facts.sol:Targets::fixedPool".into()) })
    );
    assert_eq!(source("fromParam"), Some(ValueSource::Param { index: 1, name: "p".into() }));
    assert_eq!(source("fromSender"), Some(ValueSource::MsgSender));
    assert_eq!(source("fromExpression"), Some(ValueSource::Expression { text: "pools[i]".into() }));
    assert_eq!(
        source("fromConstant"),
        Some(ValueSource::Constant { name: "KNOWN".into(), key: Some("src/Facts.sol:Targets::KNOWN".into()) })
    );
    assert_eq!(source("fromLiteral"), Some(ValueSource::Literal { text: "0x1234".into() }));
}

#[test]
fn low_level_calls_record_whether_the_result_is_used() {
    let used = |key: &str| {
        let calls = external_calls("LowLevel", key);
        assert_eq!(calls.len(), 1, "{key}: {calls:?}");
        assert_eq!(calls[0].2, Some(ValueSource::Param { index: 0, name: "to".into() }), "{key}");
        calls[0].3
    };
    for key in ["ignored", "unusedOk", "sendIgnored", "staticIgnored"] {
        assert_eq!(used(key), Some(false), "{key}");
    }
    for key in ["checked", "sendUsed", "sendStored", "delegated"] {
        assert_eq!(used(key), Some(true), "{key}");
    }
}

#[test]
fn builtin_uses_and_unchecked_blocks_are_recorded_with_spans() {
    let all = behavior("Builtins", "all");
    let names: Vec<&str> = all.builtin_uses.iter().map(|u| u.builtin.name()).collect();
    assert_eq!(
        names,
        [
            "ecrecover", "tx.origin", "block.timestamp", "block.number", "blockhash", "block.number",
            "code.length", "gasleft", "msg.value", "balance", "balance", "block.chainid", "abi.encodePacked",
        ],
        "{:?}",
        all.builtin_uses
    );
    let line = |builtin: Builtin| all.builtin_uses.iter().find(|u| u.builtin == builtin).unwrap().span.start_line;
    assert_eq!(line(Builtin::Ecrecover), line_of("ecrecover(h, v, r, s)"));
    assert_eq!(line(Builtin::Balance), line_of("a.balance"));
    assert_eq!(lines(&all.unchecked_blocks), [line_of("out += 1;") - 1]);

    let stamped = behavior("Builtins", "stamped");
    let found: Vec<(Builtin, u32)> = stamped.builtin_uses.iter().map(|u| (u.builtin, u.span.start_line)).collect();
    let (argument, body) = (line_of("stamp(block.timestamp)"), line_of("stampCount += block.number"));
    assert_eq!(found, [(Builtin::BlockTimestamp, argument), (Builtin::BlockNumber, body)], "modifier argument, then its body");
    assert_eq!(lines(&stamped.unchecked_blocks), [body]);

    let kill = behavior("Builtins", "kill");
    assert_eq!(kill.builtin_uses.iter().map(|u| u.builtin).collect::<Vec<_>>(), [Builtin::Selfdestruct]);
    let none = behavior("Builtins", "none");
    assert!(none.builtin_uses.is_empty() && none.unchecked_blocks.is_empty());

    let contract = project().contracts.iter().find(|c| c.name == "Builtins").unwrap();
    let graph = build_call_graph(project(), contract);
    let ids: Vec<String> = graph.node_weights().map(|n| n.id()).collect();
    for name in ["ecrecover", "selfdestruct", "gasleft", "blockhash", "encodePacked"] {
        assert!(!ids.iter().any(|id| id.contains(name)), "{name} is not a call: {ids:?}");
    }
}

