//! Calls between contracts as each running contract makes them, inherited and dependency code included.
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{routes, LinkKind, Relations, Tag, UnresolvedReason};

const SOURCES: &[(&str, &str)] = &[
    ("foundry.toml", "[profile.default]\nsrc = 'src'\nlibs = ['lib']\nsolc_version = '0.8.28'\nauto_detect_remappings = false\n"),
    ("remappings.txt", "@ext/=lib/ext/\n"),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IExt { function ping() external; }
library ExtLib { function twice(uint256 a) internal pure returns (uint256) { return a * 2; } }
abstract contract ExtBase {
    IExt public ext;
    function poke() public { ext.ping(); }
}
",
    ),
    (
        "src/Connections.sol",
        "pragma solidity 0.8.28;
import {ExtBase, ExtLib} from '@ext/Ext.sol';
interface IVault { function deposit(uint256 a) external; function total() external view returns (uint256); }
interface IHook { function onCall(uint256 x) external returns (uint256); }
library Fees { function cut(uint256 a) internal pure returns (uint256) { return a / 100; } }
contract Vault is IVault {
    uint256 public override total;
    function deposit(uint256 a) external { total += a; }
}
contract HookA is IHook { function onCall(uint256 x) external pure returns (uint256) { return x; } }
contract HookB is IHook { function onCall(uint256 x) external pure returns (uint256) { return x + 1; } }
abstract contract Base {
    IVault public vault;
    IHook public hook;
    modifier gate() virtual { _; }
    function pay(uint256 a) public gate {
        uint256 fee = Fees.cut(a);
        vault.deposit(a - fee);
        hook.onCall(vault.total());
        this.quote();
        ExtLib.twice(a);
    }
    function quote() external view virtual returns (uint256) { return 1; }
}
contract Router is Base, ExtBase {
    modifier gate() override { hook.onCall(0); _; }
    function route(uint256 a) external { pay(a); }
    function quote() external view override returns (uint256) { return 2; }
}
",
    ),
];

fn parse(name: &str, sources: &[(&str, &str)]) -> Project {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-connections-{name}-{}", std::process::id()));
    for (path, text) in sources {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let project = SolcFrontend.parse_project(&root).expect("parse connections project");
    fs::remove_dir_all(PathBuf::from(&root)).unwrap();
    project
}

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| parse("main", SOURCES))
}

fn analysis() -> &'static ProjectAnalysis {
    static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
    ANALYSIS.get_or_init(|| ProjectAnalysis::build(project(), &AnalysisParams::new(1)))
}

type Row = (String, LinkKind, Vec<(String, String, Tag)>, Option<UnresolvedReason>, Option<String>);

fn rows(context: &str) -> Vec<Row> {
    rows_of(analysis(), context)
}

fn rows_of(analysis: &ProjectAnalysis, context: &str) -> Vec<Row> {
    analysis
        .relations
        .calls_from(context)
        .iter()
        .map(|c| {
            let callees = c.callees.iter().map(|d| (d.function.clone(), d.contract.clone(), d.tag)).collect();
            (c.function.clone(), c.kind, callees, c.unresolved_reason, c.via.clone())
        })
        .collect()
}

fn callee(function: &str, contract: &str, tag: Tag) -> (String, String, Tag) {
    (function.into(), contract.into(), tag)
}

fn row(function: &str, kind: LinkKind, callees: Vec<(String, String, Tag)>, reason: Option<UnresolvedReason>, via: Option<&str>) -> Row {
    (function.into(), kind, callees, reason, via.map(str::to_string))
}

/// The rows of `Base::pay` up to the self call, the same in every contract that runs it.
fn pay(self_call: &str, self_contract: &str) -> Vec<Row> {
    let vault = Some("Base::vault");
    let hooks = vec![callee("HookA::onCall", "HookA", Tag::Ambiguous), callee("HookB::onCall", "HookB", Tag::Ambiguous)];
    vec![
        row("Base::pay", LinkKind::Library, vec![callee("Fees::cut", "Fees", Tag::TypeResolved)], None, None),
        row("Base::pay", LinkKind::Call, vec![callee("Vault::deposit", "Vault", Tag::Inferred)], None, vault),
        row("Base::pay", LinkKind::Call, hooks, None, Some("Base::hook")),
        row("Base::pay", LinkKind::Read, vec![callee("Vault::total", "Vault", Tag::Inferred)], None, vault),
        row("Base::pay", LinkKind::SelfCall, vec![callee(self_call, self_contract, Tag::Inferred)], None, None),
        row("Base::pay", LinkKind::Library, vec![callee("ExtLib::twice", "ExtLib", Tag::TypeResolved)], None, None),
    ]
}

#[test]
fn the_declaring_contract_runs_its_own_calls() {
    assert_eq!(rows("Base"), pay("Base::quote", "Base"));
}

#[test]
fn an_inheriting_contract_runs_inherited_and_dependency_code_with_its_own_overrides() {
    let mut expected = pay("Router::quote", "Router");
    let hooks = expected[2].2.clone();
    expected.push(row("Base::pay", LinkKind::Call, hooks, None, Some("Base::hook")));
    expected.push(row("ExtBase::poke", LinkKind::Call, vec![], Some(UnresolvedReason::OutOfScopeInterface), Some("ExtBase::ext")));
    assert_eq!(rows("Router"), expected);
    let unresolved = analysis().relations.calls_from("Router").last().unwrap();
    assert_eq!(unresolved.target.as_deref(), Some("IExt::ping"), "the dependency interface stays the callee");
}

#[test]
fn dependency_code_is_no_running_context_of_its_own() {
    assert!(rows("ExtBase").is_empty());
    assert!(analysis().relations.outgoing("ExtBase").next().is_none());
}

#[test]
fn incoming_calls_come_from_every_contract_running_the_caller() {
    let into_vault: Vec<(&str, &str, LinkKind)> =
        analysis().relations.calls_into("Vault").map(|c| (c.in_context.as_str(), c.function.as_str(), c.kind)).collect();
    assert_eq!(
        into_vault,
        vec![
            ("Base", "Base::pay", LinkKind::Call),
            ("Base", "Base::pay", LinkKind::Read),
            ("Router", "Base::pay", LinkKind::Call),
            ("Router", "Base::pay", LinkKind::Read),
        ]
    );
}

#[test]
fn links_stay_per_declaring_contract_without_library_calls() {
    let links: Vec<(&str, &str, usize)> =
        analysis().relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str(), l.calls.len())).collect();
    assert_eq!(links, vec![("Base", "Base", 1), ("Base", "HookA", 1), ("Base", "HookB", 1), ("Base", "Vault", 2)]);
}

const HELPERS: &[(&str, &str)] = &[
    (SOURCES[0].0, SOURCES[0].1),
    (SOURCES[1].0, SOURCES[1].1),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IExt { function ping() external; }
abstract contract ExtBase {
    IExt public ext;
    function poke() public { _ping(); }
    function _ping() private { ext.ping(); }
}
",
    ),
    (
        "src/Helpers.sol",
        "pragma solidity 0.8.28;
import {ExtBase} from '@ext/Ext.sol';
interface IVault { function deposit(uint256 a) external; }
contract Vault is IVault { function deposit(uint256) external {} }
abstract contract Keeper {
    IVault public vault;
    function keep(uint256 a) public { _stash(a); }
    function redo() public virtual { vault.deposit(0); }
    function _stash(uint256 a) private { vault.deposit(a); }
}
contract Kept is Keeper, ExtBase {
    function redo() public override {}
}
",
    ),
];

#[test]
fn private_helpers_of_project_and_dependency_bases_run_in_the_inheriting_contract() {
    let analysis = ProjectAnalysis::build(&parse("helpers", HELPERS), &AnalysisParams::new(1));
    let deposit = vec![callee("Vault::deposit", "Vault", Tag::Inferred)];
    let unresolved = Some(UnresolvedReason::OutOfScopeInterface);
    assert_eq!(
        rows_of(&analysis, "Kept"),
        vec![
            row("ExtBase::_ping", LinkKind::Call, vec![], unresolved, Some("ExtBase::ext")),
            row("Keeper::_stash", LinkKind::Call, deposit.clone(), None, Some("Keeper::vault")),
        ],
        "overridden code stays out"
    );
    let found = routes(&analysis.relations, &analysis.call_graphs, "Kept", "Vault", false);
    let hops: Vec<&str> = found.routes.iter().flat_map(|r| &r.steps).flat_map(|s| &s.calls).map(|c| c.call.function.as_str()).collect();
    assert_eq!(hops, vec!["Keeper::_stash"]);
    let links: Vec<(&str, &str)> = analysis.relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str())).collect();
    assert_eq!(links, vec![("Keeper", "Vault")], "links stay per declaring contract");
}

#[test]
fn building_the_relations_again_gives_the_same_bytes() {
    let stored = serde_json::to_string(&analysis().relations).unwrap();
    let again = serde_json::to_string(&Relations::build(project(), &analysis().cfgs)).unwrap();
    assert_eq!(again, stored);
    assert_eq!(serde_json::to_string(&Relations::build(project(), &analysis().cfgs)).unwrap(), stored);
}

const SUPER: &[(&str, &str)] = &[
    (SOURCES[0].0, SOURCES[0].1),
    (SOURCES[1].0, SOURCES[1].1),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IExt { function ping() external; function pong() external; }
abstract contract ExtBase {
    IExt public ext;
    function poke() public virtual { ext.ping(); }
    function skip() public virtual { ext.pong(); }
}
",
    ),
    (
        "src/Super.sol",
        "pragma solidity 0.8.28;
import {ExtBase} from '@ext/Ext.sol';
interface IVault { function deposit(uint256 a) external; }
contract Vault is IVault { function deposit(uint256) external {} }
abstract contract Mid {
    IVault public vault;
    function save() public virtual { vault.deposit(1); }
    function keep() public virtual { vault.deposit(2); }
}
contract Child is Mid, ExtBase {
    function poke() public override { super.poke(); }
    function skip() public override {}
    function save() public override { Mid.save(); }
    function keep() public override {}
}
",
    ),
];

#[test]
fn overridden_code_runs_in_the_inheriting_contract_only_where_its_code_calls_it() {
    let analysis = ProjectAnalysis::build(&parse("super", SUPER), &AnalysisParams::new(1));
    let unresolved = Some(UnresolvedReason::OutOfScopeInterface);
    assert_eq!(
        rows_of(&analysis, "Child"),
        vec![
            row("ExtBase::poke", LinkKind::Call, vec![], unresolved, Some("ExtBase::ext")),
            row("Mid::save", LinkKind::Call, vec![callee("Vault::deposit", "Vault", Tag::Inferred)], None, Some("Mid::vault")),
        ],
        "ExtBase::skip and Mid::keep are overridden and never called"
    );
    assert_eq!(analysis.relations.calls_from("Child")[0].target.as_deref(), Some("IExt::ping"));
    let links: Vec<(&str, &str, usize)> =
        analysis.relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str(), l.calls.len())).collect();
    assert_eq!(links, vec![("Mid", "Vault", 2)], "links stay per declaring contract");
}
