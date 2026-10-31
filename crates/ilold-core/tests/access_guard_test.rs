//! Guards read from code: revert checks on the entry path whose condition depends on the
//! caller (access) or on pause, reentrancy and initialization flags (state guards), found in
//! the body, in inlined modifiers and in the helpers and libraries they call.
//! Fixture: tests/fixtures/solc/access.
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::classify::entry_points::{classify_in, AccessLevel};
use ilold_core::classify::guards::{GuardFact, GuardKind};
use ilold_core::model::contract::ContractDef;
use ilold_core::model::project::Project;
use ilold_core::narrative::function::build_function_narrative;
use ilold_core::narrative::types::ObservationKind;
use ilold_core::parse::solc_frontend::SolcFrontend;

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/access");
        SolcFrontend.parse_project(&root).expect("parse access fixture")
    })
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

fn contract(name: &str) -> &'static ContractDef {
    project().contracts.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name}"))
}

/// (kind, condition, via) of the guards of `func` as `contract` runs it.
fn facts(contract_name: &str, func: &str) -> Vec<(GuardKind, String, Vec<String>)> {
    guards(contract_name, func).into_iter().map(|g| (g.kind, g.condition, g.via)).collect()
}

fn guards(contract_name: &str, func: &str) -> Vec<GuardFact> {
    analysis().sequence_analyses[contract_name]
        .functions
        .iter()
        .find(|b| b.name == func)
        .unwrap_or_else(|| panic!("behavior {contract_name}::{func}"))
        .guards
        .clone()
}

fn access(contract_name: &str, func: &str) -> AccessLevel {
    analysis().classifications[contract_name]
        .iter()
        .find(|(name, _)| name == func)
        .unwrap_or_else(|| panic!("classification {contract_name}::{func}"))
        .1
        .clone()
}

fn fact(kind: GuardKind, condition: &str, via: &[&str]) -> (GuardKind, String, Vec<String>) {
    (kind, condition.to_string(), via.iter().map(|s| s.to_string()).collect())
}

fn restricted(role: &str) -> AccessLevel {
    AccessLevel::Restricted { role: role.to_string() }
}

#[test]
fn require_on_msg_sender_in_a_modifier() {
    assert_eq!(
        facts("Guards", "setByRequire"),
        [fact(GuardKind::CallerEquals, "require(msg.sender == admin)", &["modifier onlyAdminRequire"])]
    );
    assert_eq!(access("Guards", "setByRequire"), restricted("onlyAdminRequire"));
}

#[test]
fn if_revert_on_msg_sender() {
    assert_eq!(
        facts("Guards", "setByIf"),
        [fact(GuardKind::CallerEquals, "if (msg.sender != admin) revert Unauthorized()", &["modifier onlyAdminIf"])]
    );
    assert_eq!(
        facts("Guards", "setInBody"),
        [fact(GuardKind::CallerEquals, "if (msg.sender != admin) revert Unauthorized()", &[])]
    );
    assert_eq!(access("Guards", "setInBody"), restricted("if (msg.sender != admin) revert Unauthorized()"));
}

#[test]
fn ownable_check_through_an_internal_helper() {
    assert_eq!(
        facts("Guards", "setByOwner"),
        [fact(
            GuardKind::CallerEquals,
            "if (owner() != _msgSender()) revert OwnableUnauthorizedAccount(_msgSender())",
            &["modifier onlyOwner", "Ownable::_checkOwner"],
        )]
    );
    assert_eq!(access("Guards", "setByOwner"), restricted("onlyOwner"));
}

#[test]
fn has_role_reached_with_the_caller_bound_to_a_parameter() {
    let expected = fact(
        GuardKind::RoleCheck,
        "if (!hasRole(role, account)) revert AccessControlUnauthorizedAccount(account, role)",
        &["modifier onlyRole", "AccessControl::_checkRole(bytes32)", "AccessControl::_checkRole(bytes32,address)"],
    );
    assert_eq!(facts("Guards", "setByRole"), [expected.clone()]);
    assert_eq!(access("Guards", "setByRole"), restricted("onlyRole"));
    // inherited public function guarded in the base, classified in the derived contract
    assert_eq!(facts("Guards", "grantRole"), [expected]);
    assert_eq!(access("Guards", "grantRole"), restricted("onlyRole"));
}

#[test]
fn role_guard_of_a_called_public_function() {
    assert_eq!(
        facts("Guards", "addMinter"),
        [fact(
            GuardKind::RoleCheck,
            "if (!hasRole(role, account)) revert AccessControlUnauthorizedAccount(account, role)",
            &[
                "AccessControl::grantRole",
                "modifier onlyRole",
                "AccessControl::_checkRole(bytes32)",
                "AccessControl::_checkRole(bytes32,address)",
            ],
        )]
    );
    assert_eq!(access("Guards", "addMinter"), restricted("onlyRole"));
}

#[test]
fn conditions_read_like_the_source() {
    assert_eq!(
        facts("Guards", "setByProvider"),
        [fact(GuardKind::CallerEquals, "require(_msgSender() == address(provider))", &[])]
    );
    // a long revert message is not part of the check
    assert_eq!(
        facts("Guards", "setWithMessage"),
        [fact(GuardKind::CallerEquals, "if (msg.sender != admin) revert(...)", &[])]
    );
}

#[test]
fn acl_manager_external_view_check() {
    assert_eq!(
        facts("Guards", "setByAcl"),
        [fact(
            GuardKind::RoleCheck,
            "require(IACL(provider.getACL()).isPoolAdmin(msg.sender))",
            &["modifier onlyPoolAdmin", "Guards::_onlyPoolAdmin"],
        )]
    );
    assert_eq!(access("Guards", "setByAcl"), restricted("onlyPoolAdmin"));
}

#[test]
fn allow_list_indexed_by_a_local_holding_the_caller() {
    assert_eq!(
        facts("Guards", "setByAllowList"),
        [fact(GuardKind::RoleCheck, "require(authorized[sender])", &[])]
    );
}

#[test]
fn tx_origin_check() {
    assert_eq!(
        facts("Guards", "setByOrigin"),
        [fact(GuardKind::TxOrigin, "require(tx.origin == admin)", &[])]
    );
    assert!(matches!(access("Guards", "setByOrigin"), AccessLevel::Restricted { .. }));
}

#[test]
fn guard_only_in_a_library_helper() {
    assert_eq!(
        facts("Guards", "setByKeeper"),
        [fact(GuardKind::CallerEquals, "if (msg.sender != keeper) revert NotKeeper()", &["AuthLib::requireKeeper"])]
    );
    assert_eq!(access("Guards", "setByKeeper"), restricted("if (msg.sender != keeper) revert NotKeeper()"));
}

#[test]
fn a_flag_check_named_like_access_is_not_access() {
    assert_eq!(facts("Guards", "setWhenReady"), []);
    assert_eq!(access("Guards", "setWhenReady"), AccessLevel::Public);
}

#[test]
fn user_entry_points_that_use_the_caller_are_not_guarded() {
    for func in ["deposit", "withdraw"] {
        assert_eq!(facts("Guards", func), [], "{func}");
        assert_eq!(access("Guards", func), AccessLevel::Public, "{func}");
    }
}

#[test]
fn pause_reentrancy_and_initializer_are_state_guards() {
    assert_eq!(
        facts("Guards", "pausedSet"),
        [fact(GuardKind::Pause, "if (paused()) revert EnforcedPause()", &["modifier whenNotPaused", "Pausable::_requireNotPaused"])]
    );
    assert_eq!(
        facts("Guards", "guardedSet"),
        [fact(
            GuardKind::Reentrancy,
            "if (_status == ENTERED) revert ReentrancyGuardReentrantCall()",
            &["modifier nonReentrant", "ReentrancyGuard::_nonReentrantBefore"],
        )]
    );
    assert_eq!(
        facts("Upgradeable", "initialize"),
        [fact(GuardKind::Initializer, "require(initializing || revision > lastInitializedRevision)", &["modifier initializer"])]
    );
    for (c, f) in [("Guards", "pausedSet"), ("Guards", "guardedSet"), ("Upgradeable", "initialize")] {
        assert_eq!(access(c, f), AccessLevel::Public, "{c}::{f}");
        assert!(guards(c, f).iter().all(|g| !g.kind.is_access() && !g.heuristic), "{c}::{f}");
    }
}

#[test]
fn modifier_override_resolved_from_the_derived_contract() {
    assert_eq!(facts("KeeperBase", "baseSet"), []);
    assert_eq!(access("KeeperBase", "baseSet"), AccessLevel::Public);
    assert_eq!(
        facts("KeeperDerived", "baseSet"),
        [fact(GuardKind::CallerEquals, "require(msg.sender == keeper)", &["modifier onlyKeeperish"])]
    );
    assert_eq!(access("KeeperDerived", "baseSet"), restricted("onlyKeeperish"));
    // inherited and guarded in its own body
    assert_eq!(
        facts("KeeperDerived", "ownerOnly"),
        [fact(GuardKind::CallerEquals, "require(msg.sender == keeper)", &[])]
    );
}

#[test]
fn a_modifier_without_code_falls_back_to_its_name() {
    let found = guards("Governed", "govSet");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, GuardKind::AccessByName);
    assert!(found[0].heuristic);
    assert_eq!(found[0].via, ["modifier onlyGovernance"]);
    assert_eq!(access("Governed", "govSet"), restricted("onlyGovernance"));
}

#[test]
fn guard_spans_point_at_the_check() {
    let source = |g: &GuardFact| {
        let span = g.span.as_ref().expect("span");
        let file = &project().source_files[span.file_index];
        (file.path.rsplit('/').next().unwrap().to_string(), file.content.lines().nth(span.start_line as usize - 1).unwrap().trim().to_string())
    };
    let owner = &guards("Guards", "setByOwner")[0];
    assert_eq!(source(owner), ("Ownable.sol".to_string(), "if (owner() != _msgSender()) {".to_string()));
    let body = &guards("Guards", "setByOrigin")[0];
    assert_eq!(source(body), ("Guards.sol".to_string(), "require(tx.origin == admin);".to_string()));
}

#[test]
fn classify_in_matches_the_analysis() {
    let guards_contract = contract("Guards");
    let (owner, func) = project().lookup_function(guards_contract, "transferOwnership").unwrap();
    let (level, found) = classify_in(project(), guards_contract, owner, func);
    assert_eq!(level, restricted("onlyOwner"));
    assert_eq!(found, guards("Guards", "transferOwnership"));
}

#[test]
fn no_access_control_observation_follows_guard_facts() {
    let narrative = |c: &str, f: &str| {
        let key = (c.to_string(), f.to_string());
        let contract = contract(c);
        let func = contract.functions.iter().find(|x| x.key == f).unwrap();
        build_function_narrative(
            contract,
            func,
            &analysis().path_trees[&key],
            &analysis().cfgs[&key],
            &analysis().sequence_analyses[c].functions,
        )
    };
    let flagged = |c: &str, f: &str| {
        narrative(c, f).observations.iter().any(|o| o.kind == ObservationKind::NoAccessControl)
    };
    for f in ["setByRequire", "setByOwner", "setByRole", "setByAcl", "setByKeeper", "setInBody"] {
        assert!(!flagged("Guards", f), "{f}");
    }
    for f in ["setWhenReady", "pausedSet", "guardedSet"] {
        assert!(flagged("Guards", f), "{f}");
    }
    let n = narrative("Guards", "setByAcl");
    assert_eq!(n.access, restricted("onlyPoolAdmin"));
    assert_eq!(n.guards, guards("Guards", "setByAcl"));
}

// Regressions: checks that must not be reported as access guards.

#[test]
fn checks_after_an_early_return_are_not_guards() {
    assert_eq!(facts("Lookalikes", "setAfterEarlyReturn"), []);
    assert_eq!(access("Lookalikes", "setAfterEarlyReturn"), AccessLevel::Public);
}

#[test]
fn only_view_calls_taking_the_caller_are_role_checks() {
    assert_eq!(facts("Lookalikes", "payIn"), []);
    assert_eq!(access("Lookalikes", "payIn"), AccessLevel::Public);
    assert_eq!(
        facts("Lookalikes", "membersOnly"),
        [fact(GuardKind::RoleCheck, "require(token.isMember(msg.sender))", &[])]
    );
}

#[test]
fn negations_and_disjunctions_follow_what_must_hold() {
    for f in ["everyoneButAdmin", "adminReverts", "blockList", "adminOrOpen", "notAdminAndClosed"] {
        assert_eq!(facts("Lookalikes", f), [], "{f}");
        assert_eq!(access("Lookalikes", f), AccessLevel::Public, "{f}");
    }
    assert_eq!(
        facts("Lookalikes", "openAndAdmin"),
        [fact(GuardKind::CallerEquals, "require(open && msg.sender == admin)", &[])]
    );
    // every disjunct is about the caller
    assert_eq!(
        facts("Lookalikes", "adminOrOwner"),
        [fact(GuardKind::CallerEquals, "if (!(msg.sender == admin || tx.origin == admin)) revert Closed()", &[])]
    );
}

#[test]
fn session_lists_do_not_trust_behaviors_without_guards() {
    use ilold_core::exploration::commands::{execute_command, AnalysisData, CommandResult, SessionCommand};
    use ilold_core::exploration::session::ExplorationSession;

    let a = analysis();
    let c = contract("Guards");
    // a behavior whose guards were never computed (no summary)
    let mut behaviors = a.sequence_analyses["Guards"].functions.clone();
    for b in &mut behaviors {
        b.guards.clear();
    }
    let data = AnalysisData {
        project: project(),
        contract: c,
        cfgs: &a.cfgs,
        path_trees: &a.path_trees,
        behaviors: &behaviors,
        transitions: &a.sequence_analyses["Guards"].transitions,
        classifications: &a.classifications["Guards"],
        all_sequence_analyses: &a.sequence_analyses,
        all_classifications: &a.classifications,
    };
    let mut session = ExplorationSession::new("Guards", "access");
    let CommandResult::FunctionList { functions } = execute_command(SessionCommand::Functions, &mut session, &data, "0") else {
        panic!("FunctionList")
    };
    let entry = functions.iter().find(|f| f.name == "setByOwner").unwrap();
    assert_eq!(entry.access, restricted("onlyOwner"));
    assert_eq!(entry.guards, guards("Guards", "setByOwner"));
    let CommandResult::FunctionListAll { functions } = execute_command(SessionCommand::FunctionsAll, &mut session, &data, "0") else {
        panic!("FunctionListAll")
    };
    let entry = functions.iter().find(|f| f.name == "transferOwnership").unwrap();
    assert_eq!(entry.access, restricted("onlyOwner"));

    // and the analysis computes guards for every behavior it lists
    for (name, sa) in &a.sequence_analyses {
        let Ok(c) = project().lookup_contract(name) else { continue };
        for b in &sa.functions {
            let Some(af) = project().accessible_functions(c).into_iter().find(|af| af.function.key == b.name) else { continue };
            assert_eq!(b.guards, classify_in(project(), c, af.owner, af.function).1, "{name}::{}", b.name);
        }
    }
}

#[test]
fn narratives_never_borrow_guards_from_another_overload() {
    let a = analysis();
    let over = contract("Overloads");
    let func = over.functions.iter().find(|f| f.key == "put(uint256)").unwrap();
    let key = (over.key().to_string(), func.key.clone());
    // behaviors without `put(uint256)` but with a guarded function keyed `put`
    let others = &a.sequence_analyses["SinglePut"].functions;
    assert!(others.iter().any(|b| b.name == "put" && !b.guards.is_empty()));
    let n = build_function_narrative(over, func, &a.path_trees[&key], &a.cfgs[&key], others);
    assert_eq!(n.access, AccessLevel::Public);
    assert_eq!(n.guards, []);
    // the guarded overload keeps its own guard
    let guarded = over.functions.iter().find(|f| f.key == "put(address)").unwrap();
    let key = (over.key().to_string(), guarded.key.clone());
    let n = build_function_narrative(over, guarded, &a.path_trees[&key], &a.cfgs[&key], &a.sequence_analyses["Overloads"].functions);
    assert_eq!(n.guards, guards("Overloads", "put(address)"));
    assert!(matches!(n.access, AccessLevel::Restricted { .. }));
}
