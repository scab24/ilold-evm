//! The keys a storage access indexes with, read from the raw path. Fixture: tests/fixtures/solc/keys.
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{storage_accesses, AccessKind, KeyKind, KeySite, KeysReason, RecordKeys, StorageAccess};

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/keys");
        SolcFrontend.parse_project(&root).expect("parse keys fixture")
    })
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

fn accesses(func: &str) -> Vec<StorageAccess> {
    let keys = project().lookup_contract("Keys").unwrap();
    let f = keys.functions.iter().find(|f| f.name == func).unwrap();
    let cfg = &analysis().cfgs[&(keys.key().to_string(), f.key.clone())];
    storage_accesses(project(), keys, keys, f, cfg)
}

/// `(text, kind, ref)` of each index level of the function's writes to `var`.
fn written_keys(func: &str, var: &str) -> Vec<Vec<(String, KeyKind, Option<String>)>> {
    accesses(func)
        .into_iter()
        .filter(|a| a.kind == AccessKind::Write && a.path.starts_with(var))
        .map(|a| a.keys.into_iter().map(|k| (k.text, k.kind, k.reference)).collect())
        .collect()
}

fn key(text: &str, kind: KeyKind, reference: Option<&str>) -> (String, KeyKind, Option<String>) {
    (text.to_string(), kind, reference.map(str::to_string))
}

#[test]
fn each_index_level_reports_its_text_and_where_the_value_comes_from() {
    assert_eq!(written_keys("deposit", "balances"), vec![vec![key("msg.sender", KeyKind::MsgSender, None)]]);
    assert_eq!(written_keys("credit", "balances"), vec![vec![key("account", KeyKind::Parameter, Some("Keys::credit"))]]);
    assert_eq!(written_keys("fundTreasury", "balances"), vec![vec![key("treasury", KeyKind::StateVar, Some("Keys::treasury"))]]);
    assert_eq!(
        written_keys("approve", "allowance"),
        vec![vec![key("owner", KeyKind::Parameter, Some("Keys::approve")), key("spender", KeyKind::Parameter, Some("Keys::approve"))]]
    );
    assert_eq!(written_keys("selfCredit", "balances"), vec![vec![key("address(this)", KeyKind::This, None)]]);
    assert_eq!(written_keys("setFirst", "slots"), vec![vec![key("0", KeyKind::Literal, None)]]);
    assert_eq!(written_keys("setNext", "slots"), vec![vec![key("next", KeyKind::Local, None)]]);
}

#[test]
fn a_computed_key_is_an_expression_without_a_reference() {
    assert_eq!(written_keys("store", "slots"), vec![vec![key("slotOf(seed)", KeyKind::Expression, None)]]);
}

#[test]
fn modifier_code_keys_by_the_modifier_parameter() {
    let reads: Vec<_> = accesses("withdraw").into_iter().filter(|a| a.kind == AccessKind::Read && a.modifier.is_some()).collect();
    assert_eq!(reads.len(), 1, "{reads:?}");
    let keys: Vec<_> = reads[0].keys.iter().map(|k| (k.text.as_str(), k.kind, k.reference.as_deref())).collect();
    assert_eq!(keys, [("who", KeyKind::Parameter, Some("Keys::funded"))]);
}

#[test]
fn a_pointer_returned_by_a_callee_keys_by_the_callee_parameter() {
    assert_eq!(written_keys("grow", "positions"), vec![vec![key("id", KeyKind::Parameter, Some("Keys::_position"))]]);
}

#[test]
fn a_text_that_names_two_declarations_is_an_expression() {
    assert_eq!(written_keys("shadowed", "slots"), vec![vec![key("i", KeyKind::Expression, None)]; 2]);
}

#[test]
fn a_variable_without_an_index_has_no_keys() {
    assert_eq!(written_keys("creditLater", "total"), vec![Vec::new()]);
    assert!(written_keys("creditLater", "balances").is_empty(), "the write in _credit is not direct");
}

#[test]
fn reads_in_one_block_with_different_keys_are_several() {
    let reads: Vec<KeySite> = accesses("move")
        .into_iter()
        .filter(|a| a.kind == AccessKind::Read && a.path.starts_with("balances"))
        .map(|a| KeySite { keys: a.keys, span: a.span })
        .collect();
    assert_eq!(reads.len(), 2, "{reads:?}");
    let keys = RecordKeys::direct(reads.clone());
    assert_eq!(keys.keys, None);
    assert_eq!(keys.keys_reason, Some(KeysReason::Several));
    let texts: Vec<&str> = keys.key_sites.iter().map(|s| s.keys[0].text.as_str()).collect();
    assert_eq!(texts, ["from", "to"], "sorted by span, then key text");

    let one = RecordKeys::direct(vec![reads[0].clone(), reads[0].clone()]);
    assert_eq!(one.keys.as_deref(), Some(reads[0].keys.as_slice()), "one distinct key set is the record's");
    assert_eq!(one.key_sites.len(), 1);
}

#[test]
fn transitive_records_name_the_reason_instead_of_keys() {
    let keys = RecordKeys::transitive();
    assert_eq!((keys.keys, keys.keys_reason, keys.key_sites.len()), (None, Some(KeysReason::Transitive), 0));
}
