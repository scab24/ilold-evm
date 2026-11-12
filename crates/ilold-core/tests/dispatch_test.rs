//! External calls resolved to the in-scope contracts that may answer them. Fixture: tests/fixtures/solc/dispatch.
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::expression::CallKind;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{AssignmentKind, Basis, Dispatch, Tag, UnresolvedReason};

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/dispatch");
        SolcFrontend.parse_project(&root).expect("parse dispatch fixture")
    })
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

fn dispatch(kind: CallKind, target: Option<&str>) -> Dispatch {
    let router = project().lookup_contract("Router").unwrap();
    analysis().relations.dispatch(project(), router, kind, target).into_owned()
}

fn candidates(d: &Dispatch) -> Vec<(&str, &str, Basis, Tag)> {
    d.candidates.iter().map(|c| (c.function.as_str(), c.contract.as_str(), c.basis, c.tag)).collect()
}

#[test]
fn an_interface_with_one_implementation_resolves_to_it() {
    let d = dispatch(CallKind::External, Some("IVault::deposit"));
    assert_eq!(candidates(&d), vec![("Vault::deposit", "Vault", Basis::InheritsInterface, Tag::Inferred)]);
    assert_eq!(d.unresolved_reason, None);
}

#[test]
fn an_interface_with_two_implementations_is_ambiguous_with_both() {
    let d = dispatch(CallKind::External, Some("IHook::onCall"));
    assert_eq!(
        candidates(&d),
        vec![
            ("HookA::onCall", "HookA", Basis::InheritsInterface, Tag::Ambiguous),
            ("HookB::onCall", "HookB", Basis::InheritsInterface, Tag::Ambiguous),
        ]
    );
}

#[test]
fn a_contract_exposing_the_selector_without_inheriting_matches_by_selector() {
    let d = dispatch(CallKind::External, Some("IRegistry::lookup"));
    assert_eq!(candidates(&d), vec![("Registry::lookup", "Registry", Basis::SelectorMatch, Tag::Inferred)]);
}

#[test]
fn calls_without_candidates_say_why() {
    let oracle = project().lookup_contract("IOracle").unwrap();
    assert!(oracle.is_dependency, "the fixture keeps IOracle outside the sources");
    for (target, reason) in [
        (Some("IOracle::price"), UnresolvedReason::OutOfScopeInterface),
        (Some("IOrphan::ping"), UnresolvedReason::NoImplementation),
        (None, UnresolvedReason::DynamicAddress),
    ] {
        let d = dispatch(CallKind::External, target);
        assert!(d.candidates.is_empty(), "{target:?}: {d:?}");
        assert_eq!(d.unresolved_reason, Some(reason), "{target:?}");
    }
}

#[test]
fn a_call_to_this_runs_the_calling_contract() {
    let d = dispatch(CallKind::ExternalSelf, Some("Router::selfValue"));
    assert_eq!(candidates(&d), vec![("Router::selfValue", "Router", Basis::SelfCall, Tag::Inferred)]);
}

#[test]
fn every_call_in_the_fixture_was_resolved_during_the_analysis() {
    let relations = &analysis().relations;
    let targets: Vec<(Option<&str>, Option<&str>)> =
        relations.dispatches.iter().map(|d| (d.context.as_deref(), d.target.as_deref())).collect();
    for expected in [
        (None, None),
        (None, Some("IHook::onCall")),
        (None, Some("IOracle::price")),
        (None, Some("IOrphan::ping")),
        (None, Some("IRegistry::lookup")),
        (None, Some("IVault::deposit")),
        (Some("Router"), Some("Router::selfValue")),
    ] {
        assert!(targets.contains(&expected), "{expected:?} in {targets:?}");
    }
    assert!(relations.dispatches.iter().all(|d| d.candidates.is_empty() == d.unresolved_reason.is_some()), "{relations:?}");
}

#[test]
fn assignment_sites_cover_constructor_setter_initializer_and_declaration() {
    let sites = |var: &str| -> Vec<(Option<String>, AssignmentKind, String)> {
        analysis().relations.assignments(var).iter().map(|a| (a.function.clone(), a.kind, a.expr_text.clone())).collect()
    };
    let at = |f: &str| Some(format!("Router::{f}"));
    assert_eq!(
        sites("Router::hook"),
        vec![(at("constructor"), AssignmentKind::ConstructorParam, "h".into()), (at("setHook"), AssignmentKind::Setter, "h".into())]
    );
    assert_eq!(sites("Router::vault"), vec![(at("constructor"), AssignmentKind::ConstructorParam, "v".into())]);
    assert_eq!(sites("Router::orphan")[0].1, AssignmentKind::Literal);
    assert_eq!(sites("Router::orphan")[1], (at("initialize"), AssignmentKind::Initializer, "o".into()));
    let registry = sites("Router::registry");
    assert_eq!((registry.len(), registry[0].0.clone(), registry[0].1), (1, None, AssignmentKind::Literal));
}

#[test]
fn links_aggregate_the_candidates_per_contract_pair() {
    let links: Vec<(&str, &str, Basis, Tag, Vec<&str>, usize)> = analysis()
        .relations
        .outgoing("Router")
        .map(|l| {
            let vars = l.via_state_vars.iter().map(String::as_str).collect();
            (l.from_contract.as_str(), l.to_contract.as_str(), l.basis, l.tag, vars, l.calls.len())
        })
        .collect();
    assert_eq!(
        links,
        vec![
            ("Router", "HookA", Basis::InheritsInterface, Tag::Ambiguous, vec!["hook"], 1),
            ("Router", "HookB", Basis::InheritsInterface, Tag::Ambiguous, vec!["hook"], 1),
            ("Router", "Registry", Basis::SelectorMatch, Tag::Inferred, vec!["registry"], 1),
            ("Router", "Router", Basis::SelfCall, Tag::Inferred, vec![], 1),
            ("Router", "Vault", Basis::InheritsInterface, Tag::Inferred, vec!["vault"], 1),
        ]
    );
    assert_eq!(analysis().relations.incoming("Vault").count(), 1);
}

#[test]
fn relations_survive_the_analysis_cache_format() {
    let relations = &analysis().relations;
    let json = serde_json::to_string(relations).unwrap();
    assert_eq!(&serde_json::from_str::<ilold_core::relations::Relations>(&json).unwrap(), relations);
}
