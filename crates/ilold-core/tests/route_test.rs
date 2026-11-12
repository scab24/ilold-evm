//! How one contract reaches another: shortest chains of calls, function to function.
use std::fs;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{routes, Routes};

const SOURCES: &[(&str, &str)] = &[
    ("foundry.toml", "[profile.default]\nsrc = 'src'\nlibs = []\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n"),
    (
        "src/Route.sol",
        "pragma solidity 0.8.28;
interface IMid { function f() external; function g() external; function solo() external; function r() external view returns (uint256); }
interface IEnd { function h() external; function v() external view returns (uint256); }
contract End is IEnd {
    function h() external {}
    function v() external pure returns (uint256) { return 1; }
}
contract Mid is IMid {
    IEnd public end;
    function f() external { _pass(); }
    function _pass() internal { end.h(); }
    function g() external { end.h(); }
    function solo() external { end.h(); }
    function r() external view returns (uint256) { return end.v(); }
}
contract Side {
    IEnd public end;
    function f() external { end.h(); }
}
contract Start {
    IMid public mid;
    Side public side;
    function run() external { mid.f(); }
    function lone() external { mid.g(); }
    function peek() external view returns (uint256) { return mid.r(); }
    function other() external { side.f(); }
}
",
    ),
];

fn build(name: &str, sources: &[(&str, &str)]) -> ProjectAnalysis {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-route-{name}-{}", std::process::id()));
    for (path, text) in sources {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let project = SolcFrontend.parse_project(&root).expect("parse route project");
    fs::remove_dir_all(&root).unwrap();
    ProjectAnalysis::build(&project, &AnalysisParams::new(1))
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| build("chain", SOURCES))
}

fn route(from: &str, to: &str, include_reads: bool) -> Routes {
    let analysis = analysis();
    routes(&analysis.relations, &analysis.call_graphs, from, to, include_reads)
}

/// Per route, per step: `caller -> callee`, and whether the previous step's code reaches the caller inside the contract.
fn hops(found: &Routes) -> Vec<Vec<Vec<(&str, &str, bool)>>> {
    found
        .routes
        .iter()
        .map(|r| r.steps.iter().map(|s| s.calls.iter().map(|c| (c.call.function.as_str(), c.callee.as_str(), c.recorded)).collect()).collect())
        .collect()
}

#[test]
fn every_shortest_route_lists_its_calls_and_marks_a_join_no_code_records() {
    let found = route("Start", "End", false);
    assert!(!found.truncated);
    let contracts: Vec<Vec<(&str, &str)>> =
        found.routes.iter().map(|r| r.steps.iter().map(|s| (s.from.as_str(), s.to.as_str())).collect()).collect();
    assert_eq!(contracts, vec![vec![("Start", "Mid"), ("Mid", "End")], vec![("Start", "Side"), ("Side", "End")]]);
    assert_eq!(
        hops(&found),
        vec![
            vec![
                vec![("Start::lone", "Mid::g", true), ("Start::run", "Mid::f", true)],
                vec![("Mid::_pass", "End::h", true), ("Mid::g", "End::h", true), ("Mid::solo", "End::h", false)],
            ],
            vec![vec![("Start::other", "Side::f", true)], vec![("Side::f", "End::h", true)]],
        ]
    );
}

#[test]
fn reads_join_the_route_only_when_asked() {
    let with_reads = route("Start", "End", true);
    let first = &hops(&with_reads)[0];
    assert!(first[0].contains(&("Start::peek", "Mid::r", true)), "{first:?}");
    assert!(first[1].contains(&("Mid::r", "End::v", true)), "{first:?}");
    assert!(!hops(&route("Start", "End", false))[0][1].iter().any(|c| c.0 == "Mid::r"));
}

#[test]
fn many_equally_short_routes_are_cut_and_say_so() {
    let mids: String = (0..33).map(|i| format!("contract Mid{i:02} {{ End public end; function f() external {{ end.h(); }} }}\n")).collect();
    let calls: String = (0..33).map(|i| format!("Mid{i:02}(address({})).f(); ", i + 1)).collect();
    let source = format!("pragma solidity 0.8.28;\ncontract End {{ function h() external {{}} }}\n{mids}contract Start {{ function run() external {{ {calls}}} }}\n");
    let analysis = build("wide", &[SOURCES[0], ("src/Wide.sol", source.as_str())]);
    let found = routes(&analysis.relations, &analysis.call_graphs, "Start", "End", false);
    assert!(found.truncated);
    assert_eq!(found.routes.len(), 32);
    assert_eq!(found.routes[31].steps[0].to, "Mid31", "the first routes in key order are kept");
}

#[test]
fn no_route_against_the_calls_or_to_an_unknown_contract() {
    assert!(route("End", "Start", true).routes.is_empty());
    assert!(route("Start", "Nowhere", true).routes.is_empty());
    assert!(route("Start", "Start", true).routes.is_empty());
}
