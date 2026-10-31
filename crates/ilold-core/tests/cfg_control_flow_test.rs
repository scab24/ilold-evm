use std::path::PathBuf;
use std::sync::OnceLock;

use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::{BlockKind, BranchEdge, CfgGraph, CfgStatement};
use ilold_core::model::expression::AssignOperator;
use ilold_core::model::function::FunctionDef;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::{PathTree, TerminalKind};
use ilold_core::pathtree::walker::build_path_tree;

use BlockKind::{Entry, LoopCondition as Cond, Normal, Return, Revert};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/solc/control_flow")
}

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        SolcFrontend
            .parse_project(&fixture_root())
            .expect("parse control_flow fixture")
    })
}

fn function(contract: &str, name: &str) -> &'static FunctionDef {
    let contract = project().contracts.iter().find(|c| c.name == contract).unwrap();
    contract.functions.iter().find(|f| f.name == name).unwrap()
}

fn analyze(project: &Project, contract: &str, name: &str) -> (CfgGraph, PathTree) {
    let contract = project.contracts.iter().find(|c| c.name == contract).unwrap();
    let f = contract.functions.iter().find(|f| f.name == name).unwrap();
    let cfg = CfgBuilder::build_with_project(f, contract, Some(project));
    let state_vars = project.inherited_state_vars(contract);
    let tree = build_path_tree(&cfg, &contract.name, name, &state_vars, &PruningConfig::default());
    (cfg, tree)
}

fn build(name: &str) -> (CfgGraph, PathTree) {
    analyze(project(), "ControlFlow", name)
}

fn kinds(cfg: &CfgGraph) -> Vec<BlockKind> {
    let mut blocks: Vec<_> = cfg.node_weights().map(|b| (b.id, b.kind)).collect();
    blocks.sort_by_key(|(id, _)| *id);
    blocks.into_iter().map(|(_, kind)| kind).collect()
}

fn edge_label(edge: &BranchEdge) -> &'static str {
    match edge {
        BranchEdge::Unconditional => "U",
        BranchEdge::ConditionalTrue { .. } => "T",
        BranchEdge::ConditionalFalse { .. } => "F",
        BranchEdge::ExternalCallSuccess => "Ok",
        BranchEdge::ExternalCallFailure => "Fail",
        BranchEdge::LoopBack => "Back",
        BranchEdge::LoopExit => "Exit",
        BranchEdge::CatchClause { .. } => "Catch",
    }
}

fn edges(cfg: &CfgGraph) -> Vec<(usize, usize, &'static str)> {
    let mut out: Vec<_> = cfg
        .edge_references()
        .map(|e| (cfg[e.source()].id, cfg[e.target()].id, edge_label(e.weight())))
        .collect();
    out.sort();
    out
}

fn block(cfg: &CfgGraph, id: usize) -> &ilold_core::cfg::types::BasicBlock {
    cfg.node_weights().find(|b| b.id == id).unwrap()
}

fn call_names(stmts: &[CfgStatement]) -> Vec<String> {
    stmts
        .iter()
        .filter_map(|s| match s {
            CfgStatement::ExternalCall { target, function, .. } => Some(format!("ext:{target}.{function}")),
            CfgStatement::InternalCall { function, .. } => Some(format!("int:{function}")),
            _ => None,
        })
        .collect()
}

fn calls_in_order(cfg: &CfgGraph) -> Vec<String> {
    let mut blocks: Vec<_> = cfg.node_weights().collect();
    blocks.sort_by_key(|b| b.id);
    blocks.into_iter().flat_map(|b| call_names(&b.statements)).collect()
}

fn counts(tree: &PathTree) -> (usize, usize, usize) {
    let cutoffs = tree.paths.iter().filter(|p| p.terminal == TerminalKind::LoopCutoff).count();
    (tree.stats.happy_paths, tree.stats.revert_paths, cutoffs)
}

#[test]
fn custom_error_revert_in_if_ends_the_branch() {
    let (cfg, tree) = build("ifRevert");
    assert_eq!(kinds(&cfg), vec![Entry, Normal, Revert, Normal, Return]);
    assert_eq!(edges(&cfg), vec![(0, 1, "T"), (0, 3, "F"), (1, 2, "U"), (3, 4, "U")]);
    assert_eq!(counts(&tree), (1, 1, 0));
}

#[test]
fn revert_call_form_ends_the_branch() {
    let (cfg, tree) = build("revertCall");
    assert_eq!(kinds(&cfg), vec![Entry, Normal, Revert, Normal, Return]);
    assert_eq!(edges(&cfg), vec![(0, 1, "T"), (0, 3, "F"), (1, 2, "U"), (3, 4, "U")]);
    assert!(
        call_names(&block(&cfg, 1).statements).is_empty(),
        "revert(...) must not be recorded as an internal call"
    );
    assert_eq!(counts(&tree), (1, 1, 0));
}

#[test]
fn revert_as_last_statement_has_no_happy_path() {
    let (cfg, tree) = build("revertLast");
    assert_eq!(kinds(&cfg), vec![Entry, Revert]);
    assert_eq!(edges(&cfg), vec![(0, 1, "U")]);
    assert_eq!(counts(&tree), (0, 1, 0));
}

#[test]
fn for_break_and_continue_use_the_increment_and_exit() {
    let (cfg, tree) = build("forBreakContinue");
    assert_eq!(
        kinds(&cfg),
        vec![Entry, Cond, Normal, Normal, Normal, Normal, Normal, Normal, Normal, Return]
    );
    assert_eq!(
        edges(&cfg),
        vec![
            (0, 1, "U"),
            (1, 2, "F"),
            (1, 3, "T"),
            (2, 9, "U"),
            (3, 4, "T"),
            (3, 6, "F"),
            (4, 5, "U"),
            (5, 1, "Back"),
            (6, 7, "T"),
            (6, 8, "F"),
            (7, 2, "Exit"),
            (8, 5, "U"),
        ]
    );
    // Per iteration: exit (happy), break (happy), continue or fall through (2 back edges).
    assert_eq!(counts(&tree), (14, 0, 8));
}

#[test]
fn for_increment_is_emitted_on_the_continue_block() {
    let (cfg, _) = build("forBreakContinue");
    let incr = block(&cfg, 5);
    assert!(
        incr.statements.iter().any(|s| matches!(s,
            CfgStatement::Assignment { target, operator: AssignOperator::AddAssign, value, .. }
                if target == "i" && value == "1")),
        "for increment i++ missing from the continue block: {:?}",
        incr.statements
    );
}

#[test]
fn while_continue_jumps_to_the_condition() {
    let (cfg, tree) = build("whileContinue");
    assert_eq!(kinds(&cfg), vec![Entry, Cond, Normal, Normal, Normal, Normal, Return]);
    assert_eq!(
        edges(&cfg),
        vec![
            (0, 1, "U"),
            (1, 2, "T"),
            (1, 3, "F"),
            (2, 4, "T"),
            (2, 5, "F"),
            (3, 6, "U"),
            (4, 1, "Back"),
            (5, 1, "Back"),
        ]
    );
    assert_eq!(counts(&tree), (7, 0, 8));
}

#[test]
fn do_while_break_and_continue() {
    let (cfg, tree) = build("doWhileBreakContinue");
    assert_eq!(
        kinds(&cfg),
        vec![Entry, Normal, Normal, Cond, Normal, Normal, Normal, Normal, Return]
    );
    assert_eq!(
        edges(&cfg),
        vec![
            (0, 1, "U"),
            (1, 2, "T"),
            (1, 4, "F"),
            (2, 3, "U"),
            (3, 1, "T"),
            (3, 6, "F"),
            (4, 5, "T"),
            (4, 7, "F"),
            (5, 6, "Exit"),
            (6, 8, "U"),
            (7, 3, "U"),
        ]
    );
    assert_eq!(counts(&tree), (29, 0, 16));
}

#[test]
fn nested_loops_target_the_innermost_loop() {
    let (cfg, tree) = build("nested");
    assert_eq!(
        kinds(&cfg),
        vec![
            Entry, Cond, Normal, Normal, Cond, Normal, Normal, Normal, Normal, Normal, Normal,
            Normal, Normal, Normal, Normal, Return,
        ]
    );
    assert_eq!(
        edges(&cfg),
        vec![
            (0, 1, "U"),
            (1, 2, "F"),
            (1, 3, "T"),
            (2, 15, "U"),
            (3, 4, "U"),
            (4, 5, "F"),
            (4, 6, "T"),
            (5, 12, "T"),
            (5, 13, "F"),
            (6, 7, "T"),
            (6, 9, "F"),
            (7, 8, "U"),
            (8, 4, "Back"),
            (9, 10, "T"),
            (9, 11, "F"),
            (10, 5, "Exit"),
            (11, 8, "U"),
            (12, 2, "Exit"),
            (13, 14, "U"),
            (14, 1, "Back"),
        ]
    );
    assert_eq!(tree.stats.revert_paths, 0);
    assert!(tree.stats.happy_paths > 0);
}

#[test]
fn local_mutations_are_kept_as_assignments() {
    let (cfg, _) = build("localInc");
    let entry = block(&cfg, 0);
    let x_assignments: Vec<(AssignOperator, &str)> = entry
        .statements
        .iter()
        .filter_map(|s| match s {
            CfgStatement::Assignment { target, operator, value, .. } if target == "x" => {
                Some((*operator, value.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        x_assignments,
        vec![
            (AssignOperator::Assign, "a"),
            (AssignOperator::AddAssign, "1"),
            (AssignOperator::AddAssign, "2"),
        ]
    );
    assert!(entry.statements.iter().any(|s| matches!(s,
        CfgStatement::StateWrite { variable, .. } if variable == "total")));
    assert!(!entry.statements.iter().any(|s| matches!(s,
        CfgStatement::StateWrite { variable, .. } if variable == "x")));
}

#[test]
fn calls_in_loop_and_branch_conditions_are_collected() {
    let (cfg, _) = build("condCalls");
    assert_eq!(call_names(&block(&cfg, 0).statements), vec!["ext:provider.check"]);
    let cond_calls: Vec<Vec<String>> = cfg
        .node_weights()
        .filter(|b| b.kind == Cond)
        .map(|b| call_names(&b.statements))
        .collect();
    assert_eq!(cond_calls, vec![vec!["int:_ok".to_string()]; 3]);
}

#[test]
fn calls_in_require_arguments_and_callee_objects_are_collected() {
    let (cfg, tree) = build("requireCalls");
    assert_eq!(
        calls_in_order(&cfg),
        vec![
            "ext:provider.acl",
            "ext:IAcl(provider.acl()).isAdmin",
            "int:_ok",
            "int:_val",
        ]
    );
    assert_eq!(counts(&tree), (1, 2, 0));
}

#[test]
fn calls_in_emit_revert_and_ternary_are_collected() {
    let (cfg, tree) = build("emitRevertCalls");
    assert_eq!(kinds(&cfg), vec![Entry, Normal, Revert, Normal, Return]);
    assert_eq!(call_names(&block(&cfg, 0).statements), vec!["int:_val"]);
    assert_eq!(call_names(&block(&cfg, 1).statements), vec!["int:_val2"]);
    assert_eq!(call_names(&block(&cfg, 3).statements), vec!["int:_val", "int:_val2"]);
    assert_eq!(counts(&tree), (1, 1, 0));
}

#[test]
fn constructor_base_call_is_not_a_modifier() {
    let ctor = function("ControlFlow", "constructor");
    let modifiers: Vec<&str> = ctor.modifiers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(modifiers, vec!["guarded"]);
    let bases: Vec<&str> = ctor.base_constructor_calls.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(bases, vec!["Base"]);

    let (cfg, tree) = build("constructor");
    assert_eq!(kinds(&cfg), vec![Entry, Normal, Revert, Return]);
    assert!(block(&cfg, 0).statements.iter().any(|s| matches!(s,
        CfgStatement::RequireCheck { from_modifier: Some(m), .. } if m == "guarded")));
    assert_eq!(counts(&tree), (1, 1, 0));
}

fn describe(project: &Project, contract: &str, name: &str) {
    let (cfg, tree) = analyze(project, contract, name);
    println!("== {contract}.{name}");
    let mut blocks: Vec<_> = cfg.node_weights().collect();
    blocks.sort_by_key(|b| b.id);
    for b in blocks {
        println!("  block {} {:?} calls={:?}", b.id, b.kind, call_names(&b.statements));
    }
    println!("  edges {:?}", edges(&cfg));
    let (happy, revert, cutoff) = counts(&tree);
    println!("  paths total={} happy={happy} revert={revert} loop_cutoff={cutoff}", tree.stats.total_paths);
}

/// Spot check on a copy of Aave v3 origin: ILOLD_AAVE_ROOT=<copy> cargo test ... -- --ignored --nocapture
#[test]
#[ignore]
fn aave_spot_check() {
    let Some(root) = std::env::var_os("ILOLD_AAVE_ROOT") else {
        println!("ILOLD_AAVE_ROOT not set; skipping");
        return;
    };
    let project = SolcFrontend.parse_project(&PathBuf::from(root)).expect("parse aave");
    describe(&project, "DebtTokenBase", "_decreaseBorrowAllowance");
    describe(&project, "PoolLogic", "executeMintToTreasury");
    describe(&project, "Pool", "getReservesList");
    describe(&project, "Pool", "initReserve");
    for name in ["initReserve", "_onlyPoolAdmin"] {
        let (cfg, _) = analyze(&project, "Pool", name);
        println!("== Pool.{name} calls {:?}", calls_in_order(&cfg));
    }
}
