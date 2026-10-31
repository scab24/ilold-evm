//! Model completeness: qualified contract ids, dependency contracts, inherited members,
//! storage order, declaration ids, operators and literals, partial compilation and a
//! read-only analysis of the project. Fixtures: tests/fixtures/solc/model.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::depgraph::ContractDeps;
use ilold_core::model::contract::ContractDef;
use ilold_core::model::decl_id::DeclKind;
use ilold_core::model::project::{ContractLookupError, Project};
use ilold_core::parse::error::ParseError;
use ilold_core::parse::solc_frontend::SolcFrontend;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/model").join(name)
}

fn project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| SolcFrontend.parse_project(&fixture("project")).expect("parse model fixture"))
}

fn contract(query: &str) -> &'static ContractDef {
    project().lookup_contract(query).unwrap_or_else(|e| panic!("{query}: {e:?}"))
}

#[test]
fn duplicate_names_get_qualified_ids_and_ambiguous_bare_names_fail() {
    let p = project();
    let ids: BTreeSet<&str> =
        p.contracts.iter().filter(|c| c.name == "IToken").map(|c| c.qualified_id.as_str()).collect();
    assert_eq!(ids, BTreeSet::from(["src/core/IToken.sol:IToken", "src/ext/IToken.sol:IToken"]));
    for c in p.contracts.iter().filter(|c| c.name == "IToken" || c.name == "Token") {
        assert_eq!(c.key, c.qualified_id, "duplicated names are keyed by qualified id");
    }
    assert_eq!(contract("Vault").key, "Vault", "unique names keep the bare key");
    assert_eq!(contract("Vault").qualified_id, "src/Vault.sol:Vault");

    match p.lookup_contract("IToken") {
        Err(ContractLookupError::Ambiguous(candidates)) => assert_eq!(
            candidates.into_iter().collect::<BTreeSet<_>>(),
            BTreeSet::from(["src/core/IToken.sol:IToken".to_string(), "src/ext/IToken.sol:IToken".to_string()])
        ),
        other => panic!("expected ambiguity, got {other:?}"),
    }
    let message = p.find_contract(Some("Token")).unwrap_err();
    assert!(message.contains("ambiguous") && message.contains("src/ext/Token.sol:Token"), "{message}");
    assert_eq!(p.lookup_contract("Nope").unwrap_err(), ContractLookupError::NotFound);

    assert_eq!(contract("src/core/IToken.sol:IToken").functions.len(), 3);
    assert_eq!(contract("src/ext/IToken.sol:IToken").functions.len(), 1);
}

#[test]
fn inheritance_resolves_by_declaration_not_by_name() {
    let p = project();
    let ext_token = contract("src/ext/Token.sol:Token");
    assert_eq!(ext_token.inherits, vec!["CoreToken"], "the base as written (an alias)");
    let core_itoken = contract("src/core/IToken.sol:IToken");
    assert_eq!(ext_token.base_ids, vec![core_itoken.id.unwrap()]);

    let deps = ContractDeps::from_call_graphs(p, &Default::default());
    let parents = |key: &str| -> BTreeSet<String> {
        deps.dependencies(key).iter().filter(|d| d.edge.inherits).map(|d| d.node.name.clone()).collect()
    };
    assert_eq!(parents("src/ext/Token.sol:Token"), BTreeSet::from(["src/core/IToken.sol:IToken".to_string()]));
    assert_eq!(parents("src/core/Token.sol:Token"), BTreeSet::from(["src/core/IToken.sol:IToken".to_string()]));
    assert_eq!(parents("Wrapper"), BTreeSet::from(["src/ext/IToken.sol:IToken".to_string()]));
    assert_eq!(
        parents("Vault"),
        BTreeSet::from(["IVault", "Multicall", "Ownable", "VaultBase"].map(String::from))
    );
}

#[test]
fn dependency_contracts_are_loaded_and_flagged() {
    let p = project();
    for name in ["Multicall", "Context", "Ownable", "Math"] {
        let c = contract(name);
        assert!(c.is_dependency, "{name} comes from lib/");
        assert!(c.qualified_id.starts_with("lib/ozlike/contracts/"), "{}", c.qualified_id);
    }
    assert!(p.lookup_contract("Unused").is_err(), "lib files no source imports stay out");
    let listed: BTreeSet<&str> = p.project_contracts().map(|c| c.key.as_str()).collect();
    assert!(listed.contains("Vault") && listed.contains("Ops"));
    assert!(!listed.iter().any(|k| ["Multicall", "Context", "Ownable", "Math"].contains(k)), "{listed:?}");
    assert!(p.project_contracts().all(|c| !c.is_dependency));

    // `_msgSender` and `onlyOwner` resolve to the dependency declarations
    let vault = contract("Vault");
    assert_eq!(p.resolve_modifier(vault, "onlyOwner").map(|m| m.name.as_str()), Some("onlyOwner"));
    let (owner, f) = p.resolve_function(vault, "_msgSender").expect("_msgSender inherited");
    assert_eq!((owner.name.as_str(), f.name.as_str()), ("Context", "_msgSender"));
}

#[test]
fn inherited_functions_follow_visibility_and_skip_getter_implemented_declarations() {
    let p = project();
    let vault = contract("Vault");
    let inherited: BTreeSet<(String, String)> = p
        .accessible_functions(vault)
        .into_iter()
        .filter(|af| af.is_inherited)
        .map(|af| (af.origin, af.function.name.clone()))
        .collect();
    let expected: BTreeSet<(String, String)> = [
        ("Multicall", "multicall"),
        ("Ownable", "owner"),
        ("Ownable", "_transferOwnership"),
        ("Context", "_msgSender"),
        ("VaultBase", "_helper"),
        ("VaultBase", "useSecret"),
    ]
    .iter()
    .map(|(c, f)| (c.to_string(), f.to_string()))
    .collect();
    // not ADDRESSES (implemented by the public immutable), not deposit (own), not the private
    // _secret, not the Ownable constructor
    assert_eq!(inherited, expected);
}

#[test]
fn inherited_state_vars_follow_storage_order() {
    let p = project();
    let names: Vec<String> = p.inherited_state_vars(contract("Vault")).into_iter().map(|v| v.name).collect();
    assert_eq!(names, ["baseVar", "_owner", "ADDRESSES", "total"]);
    let origins: Vec<(String, bool)> =
        p.accessible_state_vars(contract("Vault")).into_iter().map(|(_, o, inherited)| (o, inherited)).collect();
    assert_eq!(origins[0], ("VaultBase".to_string(), true));
    assert_eq!(origins[3], ("Vault".to_string(), false));
}

#[test]
fn declarations_have_stable_ids_and_readable_keys() {
    let p = project();
    let vault = contract("Vault");
    let total = vault.state_vars.iter().find(|v| v.name == "total").unwrap();
    let target = p.decl_table.lookup(total.id.expect("state var id")).unwrap();
    assert_eq!((target.kind, target.key.as_str()), (DeclKind::StateVariable, "src/Vault.sol:Vault::total"));
    assert!(target.function.is_empty(), "a state variable is never a call target");

    let ownable = contract("Ownable");
    let key_of = |id| p.decl_table.lookup(id).map(|t| (t.kind, t.key.clone())).unwrap();
    let prefix = "lib/ozlike/contracts/access/Ownable.sol:Ownable";
    assert_eq!(key_of(ownable.modifiers[0].id.unwrap()), (DeclKind::Modifier, format!("{prefix}::onlyOwner")));
    assert_eq!(key_of(ownable.events[0].id.unwrap()), (DeclKind::Event, format!("{prefix}::OwnershipTransferred")));
    assert_eq!(key_of(ownable.errors[0].id.unwrap()), (DeclKind::Error, format!("{prefix}::NotOwner")));
    assert_eq!(key_of(ownable.id.unwrap()), (DeclKind::Contract, prefix.to_string()));
    let core_token = contract("src/core/Token.sol:Token");
    let mint = core_token.functions.iter().find(|f| f.name == "mint").unwrap();
    let target = p.decl_table.lookup(mint.id.unwrap()).unwrap();
    assert_eq!(target.contract, "src/core/Token.sol:Token");
    assert_eq!(target.key, "src/core/Token.sol:Token::mint");
    let getter = vault.state_vars.iter().find(|v| v.name == "ADDRESSES").unwrap();
    assert!(getter.selector.is_some(), "public variables keep their getter selector");
}

fn collect(value: &serde_json::Value, key: &str, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    if let Some(s) = v.as_str() {
                        out.insert(s.to_string());
                    }
                }
                collect(v, key, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect(v, key, out)),
        _ => {}
    }
}

fn values(function: &str, key: &str) -> BTreeSet<String> {
    let f = contract("Ops").functions.iter().find(|f| f.name == function).unwrap();
    let mut out = BTreeSet::new();
    collect(&serde_json::to_value(&f.body).unwrap(), key, &mut out);
    out
}

#[test]
fn every_operator_is_mapped_explicitly() {
    let all = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
    let operators = values("arith", "operator");
    assert!(!operators.contains("Unknown"), "{operators:?}");
    let binary = all(&[
        "Add", "Sub", "Mul", "Div", "Mod", "Pow", "Eq", "Neq", "Lt", "Gt", "Lte", "Gte", "And", "Or", "BitAnd",
        "BitOr", "BitXor", "Shl", "Shr",
    ]);
    let assign = all(&[
        "Assign", "AddAssign", "SubAssign", "MulAssign", "DivAssign", "ModAssign", "BitAndAssign", "BitOrAssign",
        "BitXorAssign", "ShlAssign", "ShrAssign",
    ]);
    let unary =
        all(&["Not", "Neg", "BitNot", "PreIncrement", "PostIncrement", "PreDecrement", "PostDecrement", "Delete"]);
    for expected in [binary, assign, unary] {
        let missing: Vec<_> = expected.difference(&operators).collect();
        assert!(missing.is_empty(), "missing {missing:?} in {operators:?}");
    }
}

#[test]
fn literals_keep_their_kind() {
    let kinds = values("literals", "literal_type");
    assert_eq!(kinds, ["Address", "Bool", "HexString", "Number", "UnicodeString"].map(String::from).into());
    let literal_values = values("literals", "value");
    assert!(literal_values.iter().any(|v| v.contains("héllo")), "{literal_values:?}");
    assert!(literal_values.contains("0x5B38Da6a701c568545dCfcB03FcB875f56beddC4"), "{literal_values:?}");
}

#[test]
fn analysis_builds_dependencies_only_through_derived_contracts() {
    let p = project();
    let analysis = ProjectAnalysis::build(p, &AnalysisParams::new(2));
    let pt = |c: &str, f: &str| analysis.path_trees.contains_key(&(c.to_string(), f.to_string()));
    assert!(pt("Multicall", "multicall"), "inherited dependency functions are analysed");
    assert!(pt("Context", "_msgSender"));
    // a dependency library that nothing inherits is analysed only where project code calls it
    assert!(pt("Math", "max"), "Vault.deposit calls Math.max");
    assert!(!analysis.sequence_analyses.contains_key("Math"), "no behaviors listed for dependencies");
    assert!(!analysis.call_graphs.contains_key("Multicall"), "no call graph for dependencies");
    assert!(!analysis.sequence_trees.contains_key("Multicall"), "no sequences for dependencies");
    assert!(pt("src/ext/Token.sol:Token", "mint") && pt("src/core/Token.sol:Token", "mint"));
    let vault = &analysis.call_graphs["Vault"];
    let callees: BTreeSet<String> = vault.node_indices().map(|n| vault[n].id()).collect();
    assert!(callees.contains("Math::max"), "{callees:?}");
    assert!(callees.contains("Context::_msgSender"), "{callees:?}");
    let deps = analysis.dep_graph.without_dependencies();
    assert!(deps.node("Multicall").is_none() && deps.node("Vault").is_some());
    assert!(analysis.dep_graph.node("Multicall").is_some());
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "out" || name == "cache" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn listing(dir: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            out.insert(path.strip_prefix(dir).unwrap().to_path_buf());
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    out
}

fn scratch(fixture_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "ilold-model-{fixture_name}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    copy_dir(&fixture(fixture_name), &dir);
    dir
}

#[test]
fn parsing_does_not_write_into_the_project() {
    let dir = scratch("project");
    let before = listing(&dir);
    let parsed = SolcFrontend.parse_project(&dir).expect("parse copy");
    assert!(parsed.lookup_contract("Vault").is_ok());
    let after = listing(&dir);
    let created: Vec<_> = after.difference(&before).collect();
    assert!(created.is_empty(), "parse wrote into the project: {created:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn broken_test_files_keep_the_source_model_with_a_warning() {
    let dir = scratch("partial");
    for attempt in 0..2 {
        // the second parse may come from the project cache; the warning must survive it
        let parsed = SolcFrontend.parse_project(&dir).expect("sources compile");
        assert!(parsed.lookup_contract("Good").is_ok(), "attempt {attempt}");
        assert!(parsed.lookup_contract("BrokenTest").is_err());
        assert_eq!(parsed.warnings.len(), 1, "attempt {attempt}: {:?}", parsed.warnings);
        assert!(parsed.warnings[0].contains("test/Broken.t.sol"), "{}", parsed.warnings[0]);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn broken_sources_fail_with_the_message_and_a_span() {
    match SolcFrontend.parse_project(&fixture("broken_src")) {
        Err(ParseError::SyntaxError { path, message, span }) => {
            assert!(path.ends_with("src/Bad.sol"), "{path}");
            assert!(message.contains("not a number"), "{message}");
            let span = span.expect("error span");
            assert_eq!(span.start_line, 6, "{span:?}");
        }
        other => panic!("expected a syntax error, got {:?}", other.map(|p| p.contracts.len())),
    }
}
