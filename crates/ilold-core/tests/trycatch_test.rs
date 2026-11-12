//! Try statements read from the CFG, with the kind of each clause from the model. Fixture: tests/fixtures/solc/trycatch.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::expression::CallKind;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{try_catches, CatchKind, Tag, TryCatchFact};

type Analysed = (Project, ProjectAnalysis);

fn analysed(fixture: &str) -> &'static Analysed {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, &'static Analysed>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache.entry(fixture.to_string()).or_insert_with(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(fixture);
        let project = SolcFrontend.parse_project(&root).expect("parse fixture");
        let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
        Box::leak(Box::new((project, analysis)))
    })
}

fn facts_in(fixture: &str, contract: &str, func: &str) -> Vec<TryCatchFact> {
    let (project, analysis) = analysed(fixture);
    let c = project.lookup_contract(contract).unwrap();
    let f = c.functions.iter().find(|f| f.name == func).unwrap();
    try_catches(project, c, c, f, &analysis.cfgs[&(c.key().to_string(), f.key.clone())])
}

fn facts(func: &str) -> Vec<TryCatchFact> {
    facts_in("trycatch", "TryCatch", func)
}

fn only(func: &str) -> TryCatchFact {
    let mut facts = facts(func);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    facts.remove(0)
}

fn kinds(fact: &TryCatchFact) -> Vec<Option<CatchKind>> {
    fact.catch_clauses.iter().map(|c| c.kind).collect()
}

fn texts(sites: &[ilold_core::relations::EffectSite]) -> Vec<&str> {
    sites.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn an_empty_catch_swallows_the_failure_of_the_guarded_call() {
    let fact = only("swallow");
    let call = fact.guarded_call.as_ref().unwrap();
    assert_eq!((call.text.as_str(), call.kind, call.span.start_line), ("this.step", CallKind::ExternalSelf, 46));
    assert_eq!((fact.id.as_str(), fact.function.as_str(), fact.in_context.as_str()), ("TryCatch::swallow#0", "TryCatch::swallow", "TryCatch"));
    assert_eq!((fact.span.start_line, fact.tag, fact.reason), (46, Tag::Extracted, None));
    assert_eq!(fact.success_span.map(|s| s.start_line), Some(46));
    assert_eq!(kinds(&fact), [Some(CatchKind::All)]);
    let clause = &fact.catch_clauses[0];
    assert!(clause.empty);
    assert_eq!(clause.effects, Default::default());
}

#[test]
fn a_catch_that_reverts_passes_the_failure_on() {
    let fact = only("rethrow");
    let clause = &fact.catch_clauses[0];
    assert!(!clause.empty);
    assert!(clause.effects.reverts);
    assert!(clause.effects.writes.is_empty(), "the success branch writes count, not the catch");
    assert_eq!((clause.span.start_line, clause.span.end_line), (53, 55));
}

#[test]
fn a_catch_lists_the_writes_of_its_own_code() {
    let fact = only("record");
    assert_eq!(kinds(&fact), [Some(CatchKind::Error)]);
    let effects = &fact.catch_clauses[0].effects;
    assert_eq!(texts(&effects.writes), ["failures", "lastError", "misses[msg.sender]"]);
    assert!(!effects.reverts);
    assert!(effects.calls.is_empty() && effects.emits.is_empty());
}

#[test]
fn bytes_and_bare_catches_are_told_apart_by_the_model() {
    let fact = only("clauses");
    assert_eq!(kinds(&fact), [Some(CatchKind::Error), Some(CatchKind::Panic), Some(CatchKind::Bytes)]);
    let [error, panic, bytes] = &fact.catch_clauses[..] else { unreachable!() };
    assert_eq!(texts(&error.effects.emits), ["Rejected"]);
    assert_eq!(texts(&panic.effects.writes), ["panics[code]"]);
    assert_eq!(texts(&bytes.effects.writes), ["lastData"]);
    assert_eq!(texts(&bytes.effects.calls), ["pinger.ping"]);
    assert!(fact.catch_clauses.iter().all(|c| !c.effects.reverts && !c.empty));
    assert_eq!(fact.success_span.map(|s| (s.start_line, s.end_line)), Some((69, 72)));

    assert_eq!(kinds(&only("bare")), [Some(CatchKind::Error), Some(CatchKind::All)]);
    let bare = &only("bare").catch_clauses[1];
    assert_eq!(texts(&bare.effects.calls), ["_note"]);
    assert!(bare.effects.writes.is_empty(), "the callee's write is the callee's");
}

#[test]
fn a_try_on_a_contract_creation_guards_the_constructor() {
    let fact = only("create");
    let call = fact.guarded_call.as_ref().unwrap();
    assert_eq!((call.text.as_str(), call.kind), ("Child.constructor", CallKind::Creation));
    assert_eq!(texts(&fact.catch_clauses[0].effects.writes), ["failures"]);
}

#[test]
fn code_after_the_try_is_not_the_catch() {
    let fact = only("loop");
    let clause = &fact.catch_clauses[0];
    assert!(!clause.empty, "continue is a statement");
    assert_eq!(clause.effects, Default::default());
}

#[test]
fn a_try_in_a_modifier_names_it() {
    let fact = only("guarded");
    assert_eq!(fact.modifier.as_deref(), Some("pinged"));
    assert_eq!(fact.guarded_call.as_ref().map(|c| c.text.as_str()), Some("pinger.ping"));
    assert_eq!(texts(&fact.catch_clauses[0].effects.writes), ["failures"]);
}

#[test]
fn functions_without_try_have_none() {
    assert!(facts("step").is_empty());
}

#[test]
fn existing_fixtures_report_their_try_statements() {
    let stmts = facts_in("statements", "Stmts", "run");
    assert_eq!(stmts.len(), 1, "{stmts:#?}");
    assert_eq!(kinds(&stmts[0]), [Some(CatchKind::Error), Some(CatchKind::All)]);
    assert_eq!(stmts[0].guarded_call.as_ref().map(|c| c.text.as_str()), Some("this.ext"));
    assert!(stmts[0].catch_clauses.iter().all(|c| texts(&c.effects.writes).is_empty()), "acc is a local");

    let aliases = facts_in("state_flow", "Aliases", "bothTry");
    assert_eq!(aliases.len(), 1, "{aliases:#?}");
    assert_eq!(kinds(&aliases[0]), [Some(CatchKind::All)]);
    let clause = &aliases[0].catch_clauses[0];
    assert!(!clause.empty, "return 0 is a statement");
    assert!(clause.effects.writes.is_empty(), "the modifier writes after the return, outside the catch");

    let facts = facts_in("facts", "Guards", "inTry");
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert!(facts[0].catch_clauses[0].empty);
}
