//! Who pays whom: value moves grouped per running contract, with a contract on each side only where the code resolves one.
use std::fs;
use std::path::PathBuf;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{AssetKind, Party, ValuePair};

const SOURCES: &[(&str, &str)] = &[
    ("foundry.toml", "[profile.default]\nsrc = 'src'\nlibs = []\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n"),
    (
        "src/Shop.sol",
        "pragma solidity 0.8.28;
interface ISink { function fund() external payable; }
interface IToken {
    function transfer(address to, uint256 a) external returns (bool);
    function allowance(address o, address s) external view returns (uint256);
    function mint(address to, uint256 a) external;
    function burn(uint256 a) external;
}
contract Sink is ISink { function fund() external payable {} }
contract Token is IToken {
    function transfer(address, uint256) external pure returns (bool) { return true; }
    function allowance(address, address) external pure returns (uint256) { return 0; }
    function mint(address, uint256) external {}
    function burn(uint256) external {}
}
abstract contract Payer {
    ISink public sink;
    IToken public token;
    function pay() public payable {
        sink.fund{value: msg.value}();
        token.mint(msg.sender, 1);
        token.burn(1);
        payable(msg.sender).transfer(1);
    }
}
contract Shop is Payer {}
",
    ),
];

fn analysis_of(root: &PathBuf) -> ProjectAnalysis {
    let project = SolcFrontend.parse_project(root).expect("parse");
    ProjectAnalysis::build(&project, &AnalysisParams::new(1))
}

fn contract(key: &str) -> Party {
    Party::Contract(vec![key.into()])
}

fn expression(text: &str) -> Party {
    Party::Expression(Some(text.into()))
}

fn pairs<'a>(analysis: &'a ProjectAnalysis, context: &str) -> Vec<(&'a Party, &'a Party, AssetKind, usize)> {
    let of = |p: &'a ValuePair| (&p.from, &p.to, p.asset_kind, p.transfers.len());
    analysis.relations.value_pairs.iter().filter(|p| p.in_context == context).map(of).collect()
}

#[test]
fn resolved_calls_mints_and_burns_name_the_contract_on_the_other_side() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-value-pairs-{}", std::process::id()));
    for (path, text) in SOURCES {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let analysis = analysis_of(&root);
    fs::remove_dir_all(&root).unwrap();
    for context in ["Payer", "Shop"] {
        let (me, sink, token) = (contract(context), contract("Sink"), contract("Token"));
        let (payee, minted_to) = (expression("payable(msg.sender)"), expression("msg.sender"));
        let mut expected = vec![
            (&me, &sink, AssetKind::Native, 1),
            (&me, &payee, AssetKind::Native, 1),
            (&me, &token, AssetKind::Erc20, 1),
            (&token, &minted_to, AssetKind::Erc20, 1),
        ];
        expected.sort();
        let mut found = pairs(&analysis, context);
        found.sort();
        assert_eq!(found, expected, "{context}");
    }
}

#[test]
fn outside_parties_come_from_the_type_the_code_states() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/value");
    let project = SolcFrontend.parse_project(&root).expect("parse");
    let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
    let pool = Party::Declared("IPoolManager".into());
    let found = pairs(&analysis, "Payments");
    let me = contract("Payments");
    assert!(found.contains(&(&me, &pool, AssetKind::Unknown, 1)), "settle without claims pays the pool: {found:#?}");
    assert!(found.contains(&(&pool, &expression("msg.sender"), AssetKind::Unknown, 1)), "take pays the recipient: {found:#?}");
    assert!(found.contains(&(&Party::Declared("IMintableToken".into()), &expression("msg.sender"), AssetKind::Erc20, 1)), "{found:#?}");
    assert!(found.contains(&(&me, &expression("to"), AssetKind::Native, 5)), "a plain address stays its expression, one pair for all: {found:#?}");
    let transfers: usize = analysis.relations.value_pairs.iter().map(|p| p.transfers.len()).sum();
    let run = analysis.relations.values.transfers.iter().filter(|t| !project.contracts[project.contract_index[&t.in_context]].is_dependency);
    assert_eq!(transfers, run.count(), "every transfer a project contract runs is in exactly one pair");
}

const MANY: &[(&str, &str)] = &[
    ("foundry.toml", "[profile.default]\nsrc = 'src'\nlibs = ['lib']\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n"),
    ("remappings.txt", "@ext/=lib/ext/\n"),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IExtSink { function feed() external payable; }
abstract contract ExtPayer {
    IExtSink public extSink;
    function payOut() public payable { extSink.feed{value: msg.value}(); }
}
",
    ),
    (
        "src/Many.sol",
        "pragma solidity 0.8.28;
import {ExtPayer} from '@ext/Ext.sol';
interface ISink { function fund() external payable; }
contract SinkB is ISink { function fund() external payable {} }
contract SinkA is ISink { function fund() external payable {} }
contract Many is ExtPayer {
    ISink public sink;
    function pay() external payable { sink.fund{value: msg.value}(); }
}
",
    ),
];

#[test]
fn several_answering_contracts_sort_by_key_and_dependency_code_pairs_only_where_run() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-value-pairs-many-{}", std::process::id()));
    for (path, text) in MANY {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let analysis = analysis_of(&root);
    fs::remove_dir_all(&root).unwrap();
    let me = contract("Many");
    let sinks = Party::Contract(vec!["SinkA".into(), "SinkB".into()]);
    let outside = Party::Declared("IExtSink".into());
    let mut found = pairs(&analysis, "Many");
    found.sort();
    assert_eq!(found, vec![(&me, &sinks, AssetKind::Native, 1), (&me, &outside, AssetKind::Native, 1)]);
    let contexts: Vec<&str> = analysis.relations.value_pairs.iter().map(|p| p.in_context.as_str()).collect();
    assert_eq!(contexts, vec!["Many", "Many"], "no pair for code only its dependency declares");
    assert!(analysis.relations.values.transfers.iter().any(|t| t.in_context == "ExtPayer"), "the dependency's own facts stay");
}
