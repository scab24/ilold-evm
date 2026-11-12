//! Calls between contracts as each running contract makes them, inherited and dependency code included.
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{routes, AssignmentKind, ConnectionGroup, ContractCallKind, ContractLink, Relations, Tag, UnresolvedReason};

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

type Row = (String, ContractCallKind, Vec<(String, String, Tag)>, Option<UnresolvedReason>, Option<String>);

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

fn row(function: &str, kind: ContractCallKind, callees: Vec<(String, String, Tag)>, reason: Option<UnresolvedReason>, via: Option<&str>) -> Row {
    (function.into(), kind, callees, reason, via.map(str::to_string))
}

/// The rows of `Base::pay` up to the self call, the same in every contract that runs it.
fn pay(self_call: &str, self_contract: &str) -> Vec<Row> {
    let vault = Some("Base::vault");
    let hooks = vec![callee("HookA::onCall", "HookA", Tag::Ambiguous), callee("HookB::onCall", "HookB", Tag::Ambiguous)];
    vec![
        row("Base::pay", ContractCallKind::Library, vec![callee("Fees::cut", "Fees", Tag::TypeResolved)], None, None),
        row("Base::pay", ContractCallKind::Call, vec![callee("Vault::deposit", "Vault", Tag::Inferred)], None, vault),
        row("Base::pay", ContractCallKind::Call, hooks, None, Some("Base::hook")),
        row("Base::pay", ContractCallKind::Read, vec![callee("Vault::total", "Vault", Tag::Inferred)], None, vault),
        row("Base::pay", ContractCallKind::SelfCall, vec![callee(self_call, self_contract, Tag::Inferred)], None, None),
        row("Base::pay", ContractCallKind::Library, vec![callee("ExtLib::twice", "ExtLib", Tag::TypeResolved)], None, None),
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
    expected.push(row("Base::pay", ContractCallKind::Call, hooks, None, Some("Base::hook")));
    expected.push(row("ExtBase::poke", ContractCallKind::Call, vec![], Some(UnresolvedReason::OutOfScopeInterface), Some("ExtBase::ext")));
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
fn incoming_calls_come_from_every_deployed_contract_running_the_caller() {
    let into_vault: Vec<(&str, &str, ContractCallKind)> =
        analysis().relations.calls_into("Vault").map(|c| (c.in_context.as_str(), c.function.as_str(), c.kind)).collect();
    assert_eq!(
        into_vault,
        vec![("Router", "Base::pay", ContractCallKind::Call), ("Router", "Base::pay", ContractCallKind::Read)],
        "the abstract Base is never deployed, so it calls nothing on its own"
    );
    assert_eq!(rows("Base").len(), 6, "an abstract contract viewed directly keeps its own calls");
    assert!(routes(&analysis().relations, &analysis().call_graphs, "Base", "Vault", true).routes.is_empty());
    assert_eq!(routes(&analysis().relations, &analysis().call_graphs, "Router", "Vault", false).routes.len(), 1);
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
            row("ExtBase::_ping", ContractCallKind::Call, vec![], unresolved, Some("ExtBase::ext")),
            row("Keeper::_stash", ContractCallKind::Call, deposit.clone(), None, Some("Keeper::vault")),
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
            row("ExtBase::poke", ContractCallKind::Call, vec![], unresolved, Some("ExtBase::ext")),
            row("Mid::save", ContractCallKind::Call, vec![callee("Vault::deposit", "Vault", Tag::Inferred)], None, Some("Mid::vault")),
        ],
        "ExtBase::skip and Mid::keep are overridden and never called"
    );
    assert_eq!(analysis.relations.calls_from("Child")[0].target.as_deref(), Some("IExt::ping"));
    let links: Vec<(&str, &str, usize)> =
        analysis.relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str(), l.calls.len())).collect();
    assert_eq!(links, vec![("Mid", "Vault", 2)], "links stay per declaring contract");
}

const CONSTRUCTORS: &[(&str, &str)] = &[
    (SOURCES[0].0, SOURCES[0].1),
    (SOURCES[1].0, SOURCES[1].1),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IExt { function ping() external; }
abstract contract ExtBase {
    IExt public ext;
    constructor(IExt e) { ext = e; ext.ping(); }
}
",
    ),
    (
        "src/Ctor.sol",
        "pragma solidity 0.8.28;
import {ExtBase, IExt} from '@ext/Ext.sol';
interface IVault { function init() external; }
contract Vault is IVault { function init() external {} }
abstract contract Base {
    IVault public vault;
    constructor(IVault v) { vault = v; vault.init(); }
}
contract Child is Base, ExtBase {
    constructor(IVault v, IExt e) Base(v) ExtBase(e) {}
}
",
    ),
];

#[test]
fn every_base_constructor_runs_in_the_deploying_contract() {
    let analysis = ProjectAnalysis::build(&parse("constructors", CONSTRUCTORS), &AnalysisParams::new(1));
    assert_eq!(
        rows_of(&analysis, "Child"),
        vec![
            row("Base::constructor", ContractCallKind::Call, vec![callee("Vault::init", "Vault", Tag::Inferred)], None, Some("Base::vault")),
            row("ExtBase::constructor", ContractCallKind::Call, vec![], Some(UnresolvedReason::OutOfScopeInterface), Some("ExtBase::ext")),
        ]
    );
    let links: Vec<(&str, &str, usize)> =
        analysis.relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str(), l.calls.len())).collect();
    assert_eq!(links, vec![("Base", "Vault", 1)], "links stay per declaring contract");
}

fn groups(groups: &[ConnectionGroup]) -> Vec<(&str, usize)> {
    groups.iter().map(|g| (g.contract.as_str(), g.calls.len())).collect()
}

#[test]
fn connections_group_the_running_contract_calls_by_the_other_contract() {
    let router = analysis().relations.connections("Router");
    assert_eq!(router.contract, "Router");
    assert_eq!(groups(&router.outgoing), vec![("ExtLib", 1), ("Fees", 1), ("HookA", 2), ("HookB", 2), ("Router", 1), ("Vault", 2)]);
    let hook = &router.outgoing[2].calls[0];
    assert_eq!(router.outgoing[3].calls[0], *hook, "an ambiguous call is one row under each candidate");
    assert_eq!(hook.callees.len(), 2);
    let unresolved: Vec<(&str, Option<UnresolvedReason>)> = router.unresolved.iter().map(|c| (c.function.as_str(), c.unresolved_reason)).collect();
    assert_eq!(unresolved, vec![("ExtBase::poke", Some(UnresolvedReason::OutOfScopeInterface))]);
    assert!(groups(&router.incoming).is_empty(), "its own self call is not incoming");

    let vault = analysis().relations.connections("Vault");
    assert!(vault.outgoing.is_empty() && vault.unresolved.is_empty());
    assert_eq!(groups(&vault.incoming), vec![("Router", 2)]);
    assert_eq!((router.outgoing_calls, router.incoming_calls), (4, 0), "an ambiguous call once, library, self and unresolved calls not");
    assert_eq!((vault.outgoing_calls, vault.incoming_calls), (0, 2));
    let base = analysis().relations.connections("Base");
    assert_eq!(groups(&base.outgoing), vec![("Base", 1), ("ExtLib", 1), ("Fees", 1), ("HookA", 1), ("HookB", 1), ("Vault", 2)]);
}

#[test]
fn connections_carry_where_the_called_addresses_are_set() {
    let analysis = ProjectAnalysis::build(&parse("connections-ctor", CONSTRUCTORS), &AnalysisParams::new(1));
    let child = analysis.relations.connections("Child");
    let sites: Vec<(&str, Option<&str>, AssignmentKind)> =
        child.assignments.iter().map(|a| (a.state_var.as_str(), a.function.as_deref(), a.kind)).collect();
    assert_eq!(
        sites,
        vec![
            ("Base::vault", Some("Base::constructor"), AssignmentKind::ConstructorParam),
            ("ExtBase::ext", Some("ExtBase::constructor"), AssignmentKind::ConstructorParam),
        ]
    );
}

#[test]
fn context_links_add_up_the_calls_each_running_contract_makes() {
    let relations = &analysis().relations;
    let summary = |links: Vec<ContractLink>| links.into_iter().map(|l| (l.from_contract, l.to_contract, l.calls.len(), l.tag)).collect::<Vec<_>>();
    let (outgoing, incoming) = relations.context_links("Router");
    assert_eq!(
        summary(outgoing),
        vec![
            ("Router".into(), "HookA".into(), 2, Tag::Ambiguous),
            ("Router".into(), "HookB".into(), 2, Tag::Ambiguous),
            ("Router".into(), "Router".into(), 1, Tag::Inferred),
            ("Router".into(), "Vault".into(), 2, Tag::Inferred),
        ]
    );
    assert_eq!(summary(incoming), vec![], "its own self call is not incoming");
    let (_, into_vault) = relations.context_links("Vault");
    assert_eq!(summary(into_vault), vec![("Router".into(), "Vault".into(), 2, Tag::Inferred)]);
    let (base, _) = relations.context_links("Base");
    assert_eq!(base, relations.outgoing("Base").cloned().collect::<Vec<_>>(), "a contract running only its own code adds up to its links");
}

const TRIES: &[(&str, &str)] = &[
    (SOURCES[0].0, SOURCES[0].1),
    (
        "src/Tries.sol",
        "pragma solidity 0.8.28;
interface IVault { function deposit(uint256 a) external payable; }
contract Vault is IVault { function deposit(uint256) external payable {} }
contract Keeper {
    IVault public vault;
    function keep() external payable {
        try vault.deposit{value: msg.value}(1) {} catch {}
        vault.deposit(2);
    }
}
",
    ),
];

#[test]
fn rows_carry_the_value_they_move_whether_a_try_guards_them_and_their_file() {
    let analysis = ProjectAnalysis::build(&parse("tries", TRIES), &AnalysisParams::new(1));
    let made = analysis.relations.calls_from("Keeper");
    let rows: Vec<(u32, bool, &[String])> = made.iter().map(|c| (c.span.start_line, c.in_try, c.transfers.as_slice())).collect();
    assert_eq!(rows, vec![(7, true, &["Keeper::keep#0".to_string()][..]), (8, false, &[][..])]);
    let moved: Vec<(&str, &str)> = analysis.relations.transfers_at(&made[0]).iter().map(|t| (t.id.as_str(), t.call.as_str())).collect();
    assert_eq!(moved, vec![("Keeper::keep#0", "vault.deposit")]);
    assert!(analysis.relations.transfers_at(&made[1]).is_empty());
    assert!(made.iter().all(|c| c.file.as_deref().is_some_and(|f| f.ends_with("/src/Tries.sol"))), "{made:#?}");
    let connections = analysis.relations.connections("Keeper");
    let ids: Vec<&str> = connections.transfers.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["Keeper::keep#0"], "the view carries the facts its rows name");
}

const LIBRARIES: &[(&str, &str)] = &[
    (SOURCES[0].0, SOURCES[0].1),
    (SOURCES[1].0, SOURCES[1].1),
    (
        "lib/ext/Ext.sol",
        "pragma solidity 0.8.28;
interface IToken { function transfer(address to, uint256 a) external returns (bool); }
library SafeERC20 {
    function safeTransfer(IToken token, address to, uint256 a) internal { token.transfer(to, a); }
}
",
    ),
    (
        "src/Libraries.sol",
        "pragma solidity 0.8.28;
import {IToken, SafeERC20} from '@ext/Ext.sol';
contract Token is IToken { function transfer(address, uint256) external pure returns (bool) { return true; } }
library Payout {
    function send(IToken token, address to, uint256 a) internal { token.transfer(to, a); }
    function safe(IToken token, address to, uint256 a) internal { SafeERC20.safeTransfer(token, to, a); }
}
abstract contract Shop {
    IToken public token;
    function sell(address to) public { Payout.send(token, to, 1); }
}
contract Store is Shop {
    function refund(address to) external { Payout.safe(token, to, 2); }
}
",
    ),
];

#[test]
fn project_library_code_runs_in_the_calling_contract() {
    let analysis = ProjectAnalysis::build(&parse("libraries", LIBRARIES), &AnalysisParams::new(1));
    let transfer = vec![callee("Token::transfer", "Token", Tag::Inferred)];
    let library = |function: &str, contract: &str| vec![callee(function, contract, Tag::TypeResolved)];
    assert_eq!(
        rows_of(&analysis, "Store"),
        vec![
            row("Payout::safe", ContractCallKind::Library, library("SafeERC20::safeTransfer", "SafeERC20"), None, None),
            row("Payout::send", ContractCallKind::Call, transfer.clone(), None, None),
            row("Shop::sell", ContractCallKind::Library, library("Payout::send", "Payout"), None, None),
            row("Store::refund", ContractCallKind::Library, library("Payout::safe", "Payout"), None, None),
        ],
        "dependency library code stays at its call site"
    );
    assert_eq!(
        rows_of(&analysis, "Payout"),
        vec![
            row("Payout::safe", ContractCallKind::Library, library("SafeERC20::safeTransfer", "SafeERC20"), None, None),
            row("Payout::send", ContractCallKind::Call, transfer, None, None),
        ],
        "a library viewed directly keeps its own calls"
    );
    let relations = &analysis.relations;
    let links: Vec<(&str, &str, usize)> = relations.links.iter().map(|l| (l.from_contract.as_str(), l.to_contract.as_str(), l.calls.len())).collect();
    assert_eq!(links, vec![("Payout", "Token", 1)], "links stay per declaring contract");
    let into_token: Vec<(String, usize)> = relations.connections("Token").incoming.iter().map(|g| (g.contract.clone(), g.calls.len())).collect();
    assert_eq!(into_token, vec![("Store".to_string(), 1)], "the library calls only inside the contract running it");

    let moved: Vec<(&str, Option<&str>)> =
        relations.values.transfers.iter().filter(|t| t.in_context == "Store").map(|t| (t.function.as_str(), t.via_library.as_deref())).collect();
    assert_eq!(moved, vec![("Payout::safe", Some("SafeERC20::safeTransfer")), ("Payout::send", None)], "each move once, at its own call site");
    let pairs: Vec<(&str, String, String, usize)> = relations
        .value_pairs
        .iter()
        .map(|p| (p.in_context.as_str(), format!("{:?}", p.from), format!("{:?}", p.to), p.transfers.len()))
        .collect();
    assert_eq!(pairs, vec![("Store", "Contract([\"Store\"])".to_string(), "Expression(Some(\"to\"))".to_string(), 2)]);
}
