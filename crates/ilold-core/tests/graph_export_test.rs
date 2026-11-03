//! The graph export through the library: canonical lines and the no-facts envelope. CLI behaviour: crates/ilold-cli/tests/graph_export_test.rs.
use std::path::PathBuf;

use ilold_core::analysis::AnalysisParams;
use ilold_core::export::{export_graph, Build, Status};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(name).canonicalize().unwrap()
}

#[test]
fn every_line_is_canonical_json_with_sorted_keys() {
    let export = export_graph(&fixture("export/kinds"), &AnalysisParams::new(3), &Build::default()).expect("export");
    assert_eq!(export.status, Status::Complete);
    for line in export.jsonl.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(value.to_string(), line, "keys sorted, no whitespace");
    }
}

#[test]
fn broken_sources_export_only_the_envelope() {
    let export = export_graph(&fixture("export/broken_src"), &AnalysisParams::new(3), &Build::default()).expect("export");
    assert_eq!(export.status, Status::NoFacts);
    assert_eq!(export.jsonl.lines().count(), 1);
}
