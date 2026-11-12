//! Phase candidates: state variables tested in the conditions of several entry points and written by one. Fixture: tests/fixtures/solc/phases.
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{phase_candidates, PhaseCandidate, Tag};

fn analysed() -> &'static (Project, ProjectAnalysis) {
    static CACHE: OnceLock<(Project, ProjectAnalysis)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/phases");
        let project = SolcFrontend.parse_project(&root).expect("parse fixture");
        let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
        (project, analysis)
    })
}

fn candidates(contract: &str) -> Vec<PhaseCandidate> {
    let (project, analysis) = analysed();
    let c = project.lookup_contract(contract).unwrap();
    phase_candidates(project, c, &analysis.sequence_analyses[c.key()].functions)
}

fn candidate(contract: &str, state_var: &str) -> PhaseCandidate {
    let found = candidates(contract);
    found.iter().find(|c| c.state_var == state_var).cloned().unwrap_or_else(|| panic!("no {state_var} in {found:#?}"))
}

#[test]
fn a_pause_flag_tested_by_a_modifier_and_set_by_a_setter_is_a_phase() {
    let paused = candidate("Sale", "Pausable::paused");
    assert_eq!((paused.in_context.as_str(), paused.type_name.as_str(), paused.tag), ("Sale", "bool", Tag::Inferred));
    assert_eq!(paused.guarded_functions, ["Sale::buy", "Sale::refund"]);
    assert_eq!(paused.writers, ["Pausable::setPaused"]);
    assert_eq!(paused.condition_texts, ["!paused"]);
    assert_eq!(paused.span.start_line, 6, "the declaration");
}

#[test]
fn an_enum_state_machine_lists_the_functions_it_gates_and_the_transitions() {
    let state = candidate("Sale", "Sale::state");
    assert_eq!(state.type_name, "enum Sale.State");
    assert_eq!(state.guarded_functions, ["Sale::buy", "Sale::close", "Sale::open", "Sale::refund"]);
    assert_eq!(state.writers, ["Sale::close", "Sale::open"]);
    assert_eq!(state.condition_texts, ["state != State.Closed", "state == State.Open", "state == State.Setup"]);
}

#[test]
fn a_counter_compared_with_a_constant_in_a_helper_counts_for_the_caller() {
    let round = candidate("Sale", "Sale::round");
    assert_eq!(round.guarded_functions, ["Sale::buy", "Sale::nextRound"]);
    assert_eq!(round.writers, ["Sale::nextRound"]);
    assert_eq!(round.condition_texts, ["round != FINAL_ROUND", "round == FINAL_ROUND"]);
}

#[test]
fn one_test_no_writer_or_no_constant_is_not_a_phase() {
    let found: Vec<String> = candidates("Sale").into_iter().map(|c| c.state_var).collect();
    assert_eq!(found, ["Pausable::paused", "Sale::round", "Sale::state"], "audited is tested once, frozen never written, raised compared with cap");
}

#[test]
fn inherited_functions_count_in_each_deployable_contract() {
    let paused = candidate("Vault", "Pausable::paused");
    assert_eq!(paused.in_context, "Vault");
    assert_eq!(paused.guarded_functions, ["Vault::deposit", "Vault::withdraw"]);
    assert_eq!(paused.writers, ["Pausable::setPaused"]);
    assert!(candidates("Pausable").is_empty(), "an abstract contract runs nothing on its own");
}
