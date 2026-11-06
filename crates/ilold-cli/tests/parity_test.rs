//! One fixture through the export, the web API and the MCP tools: each fact must read the same everywhere.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use ilold_core::pathtree::walker::normalize_path;
use ilold_evm_mcp::{dispatch, IloldClient};
use serde_json::Value;
use tokio::sync::Mutex;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(name).canonicalize().unwrap()
}

/// The export's records, envelope first.
fn export(project: &Path) -> Vec<Value> {
    // tests run in parallel and the clock may only count microseconds
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "ilold-parity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("graph.jsonl");
    let run = Command::new(env!("CARGO_BIN_EXE_ilold"))
        .args(["graph", "--json"])
        .arg(&out)
        .arg(project)
        .env("ILOLD_PROJECT_CACHE_DIR", dir.join("cache"))
        .env_remove("ILOLD_DISABLE_PROJECT_CACHE")
        .env_remove("FOUNDRY_PROFILE")
        .output()
        .expect("run ilold graph");
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

struct Surfaces {
    client: IloldClient,
    records: Vec<Value>,
}

impl Surfaces {
    async fn of(name: &str) -> Self {
        let project = fixture(name);
        let (_state, port) = ilold_web::start_server(vec![project.clone()], 0, 2).await.unwrap();
        Self { client: IloldClient::new(format!("http://127.0.0.1:{port}")), records: export(&project) }
    }

    fn records<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Value> {
        self.records.iter().filter(move |r| r["kind"] == kind)
    }

    async fn web(&self, path: &str) -> Value {
        self.client.get(path).await.unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    async fn mcp(&self, tool: &str, arguments: Value) -> Value {
        let result = dispatch(&self.client, &Mutex::new(None), tool, Some(&arguments)).await;
        assert!(!result.is_error.unwrap_or(false), "{tool}: {:?}", result.content);
        result.structured_content.unwrap_or_else(|| panic!("{tool} returned no JSON"))
    }
}

fn names(values: &Value, field: &str) -> BTreeSet<String> {
    values.as_array().unwrap().iter().map(|v| v[field].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn contracts_are_the_same_in_every_surface() {
    let s = Surfaces::of("cross").await;
    let exported: BTreeSet<String> =
        s.records("contract").filter(|c| c["is_dependency"] == false).map(|c| c["name"].as_str().unwrap().to_string()).collect();
    let web = names(&s.web("/api/project").await["contracts"], "name");
    let mcp = names(&s.mcp("ilold_project_overview", serde_json::json!({})).await["contracts"], "name");
    assert!(!exported.is_empty());
    assert_eq!(exported, web);
    assert_eq!(web, mcp);
}

/// A contract link as every surface shows it: endpoints, basis, tag, address variables and call positions.
type Link = (String, String, String, String, Vec<String>, BTreeSet<(u64, u64)>);

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("not a string: {value}")).to_string()
}

fn web_link(link: &Value) -> Link {
    let calls = link["calls"].as_array().unwrap().iter().map(|c| (c["span"]["start_line"].as_u64().unwrap(), c["span"]["start_col"].as_u64().unwrap())).collect();
    let vars = link["via_state_vars"].as_array().unwrap().iter().map(text).collect();
    (text(&link["from_contract"]), text(&link["to_contract"]), text(&link["basis"]), text(&link["tag"]), vars, calls)
}

/// The map rows of kind `ContractLink` carry the link without its calls; their count stands for them.
fn map_links(map: &Value) -> BTreeSet<(String, String, String, String, Vec<String>, u64)> {
    map["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "ContractLink")
        .map(|r| {
            let vars = r["via_state_vars"].as_array().unwrap().iter().map(text).collect();
            (text(&r["from_contract"]), text(&r["to_contract"]), text(&r["basis"]), text(&r["tag"]), vars, r["call_count"].as_u64().unwrap())
        })
        .collect()
}

fn depgraph_links(graph: &Value) -> BTreeSet<Link> {
    graph["edges"].as_array().unwrap().iter().map(|e| &e["data"]["link"]).filter(|l| !l.is_null()).map(web_link).collect()
}

fn without_calls(links: &BTreeSet<Link>) -> BTreeSet<(String, String, String, String, Vec<String>, u64)> {
    links.iter().map(|(f, t, b, g, v, c)| (f.clone(), t.clone(), b.clone(), g.clone(), v.clone(), c.len() as u64)).collect()
}

impl Surfaces {
    /// Export ids of contracts and members to the names the web API uses.
    fn names(&self) -> std::collections::BTreeMap<String, String> {
        self.records.iter().filter(|r| r["kind"] == "contract").map(|c| (text(&c["id"]), text(&c["name"]))).collect()
    }

    fn exported_links(&self) -> BTreeSet<Link> {
        let names = self.names();
        let calls: std::collections::BTreeMap<String, &Value> = self.records("calls").map(|c| (text(&c["id"]), c)).collect();
        self.records("contract_link")
            .map(|l| {
                let vars = l["via_state_vars"].as_array().unwrap().iter().map(|v| text(v).rsplit_once('.').unwrap().1.to_string()).collect();
                let spans = l["call_ids"].as_array().unwrap().iter().map(|id| &calls[&text(id)]["span"]).map(|s| (s["start_line"].as_u64().unwrap(), s["start_col"].as_u64().unwrap())).collect();
                (names[&text(&l["from_contract"])].clone(), names[&text(&l["to_contract"])].clone(), text(&l["basis"]), text(&l["tag"]), vars, spans)
            })
            .collect()
    }
}

#[tokio::test]
async fn contract_links_are_the_same_in_every_surface() {
    let s = Surfaces::of("dispatch").await;
    let exported = s.exported_links();
    assert_eq!(exported.len(), 5, "{exported:#?}");

    assert_eq!(depgraph_links(&s.web("/api/project/depgraph").await), exported, "/api/project/depgraph");
    assert_eq!(depgraph_links(&s.mcp("ilold_dependency_graph", serde_json::json!({})).await), exported, "ilold_dependency_graph");
    assert_eq!(map_links(&s.web("/api/project/map").await), without_calls(&exported), "/api/project/map");
    assert_eq!(map_links(&s.mcp("ilold_project_map", serde_json::json!({})).await), without_calls(&exported), "ilold_project_map");

    let contracts: BTreeSet<String> = exported.iter().flat_map(|(f, t, ..)| [f.clone(), t.clone()]).collect();
    for contract in contracts {
        let touching: BTreeSet<Link> = exported.iter().filter(|(f, t, ..)| *f == contract || *t == contract).cloned().collect();
        let detail = s.web(&format!("/api/contract/{contract}")).await;
        let listed: BTreeSet<Link> = ["outgoing_links", "incoming_links"].iter().flat_map(|k| detail[k].as_array().unwrap().iter().map(web_link)).collect();
        assert_eq!(listed, touching, "/api/contract/{contract}");
        let deps = s.mcp("ilold_contract_dependencies", serde_json::json!({"contract": contract})).await;
        assert_eq!(depgraph_links(&deps), touching, "ilold_contract_dependencies {contract}");
    }
}

/// Per call of `contract`, keyed by caller and static target names: the candidates as (contract, basis, tag), or the reason.
type Candidates = std::collections::BTreeMap<(String, String), (BTreeSet<(String, String, String)>, Option<String>)>;

fn callgraph_candidates(graph: &Value) -> Candidates {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["data"])
        .filter(|d| d.get("dispatch_candidates").is_some())
        .map(|d| {
            let candidates = d["dispatch_candidates"].as_array().unwrap().iter().map(|c| (text(&c["contract"]), text(&c["basis"]), text(&c["tag"]))).collect();
            ((text(&d["source"]), text(&d["target"])), (candidates, d["unresolved_reason"].as_str().map(str::to_string)))
        })
        .collect()
}

#[tokio::test]
async fn dispatch_candidates_are_the_same_in_every_surface() {
    let s = Surfaces::of("dispatch").await;
    let names = s.names();
    // `evm:<file>:<Contract>.<name>(<types>)` as the call graph names it, `<Contract>::<name>`
    let member = |id: &str| {
        let (contract, rest) = id.rsplit_once(':').unwrap().1.split_once('.').unwrap();
        format!("{contract}::{}", rest.split('(').next().unwrap())
    };
    let exported: Candidates = s
        .records("calls")
        .filter(|c| c.get("dispatch_candidates").is_some() && !c["static_target"].is_null())
        .map(|c| {
            let candidates = c["dispatch_candidates"].as_array().unwrap().iter().map(|d| (names[&text(&d["contract"])].clone(), text(&d["basis"]), text(&d["tag"]))).collect();
            ((member(&text(&c["from"])), member(&text(&c["static_target"]))), (candidates, c["unresolved_reason"].as_str().map(str::to_string)))
        })
        .collect();
    assert_eq!(exported.len(), 6, "{exported:#?}");

    let web: Candidates = callgraph_candidates(&s.web("/api/contract/Router/callgraph").await).into_iter().filter(|(k, _)| exported.contains_key(k)).collect();
    assert_eq!(web, exported, "/callgraph");
    let mcp: Candidates = callgraph_candidates(&s.mcp("ilold_callgraph", serde_json::json!({"contract": "Router"})).await).into_iter().filter(|(k, _)| exported.contains_key(k)).collect();
    assert_eq!(mcp, exported, "ilold_callgraph");
}

/// `(text, kind, <Contract>.<member>)` of each index level; parameters name their function or modifier.
type Keys = Vec<(String, String, Option<String>)>;
/// A direct access site: function, read or write, path without keys, line and keys.
type KeySite = (String, String, String, u64, Keys);

fn keys(keys: &Value, reference: impl Fn(&str) -> String) -> Keys {
    keys.as_array().unwrap().iter().map(|k| (text(&k["text"]), text(&k["kind"]), k["ref"].as_str().map(&reference))).collect()
}

fn line(span: &Value) -> u64 {
    span["start_line"].as_u64().unwrap()
}

fn api_sites(function: &str, accesses: &Value) -> BTreeSet<KeySite> {
    let reference = |r: &str| r.replace("::", ".");
    accesses
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let kind = if a["kind"] == "read" { "reads" } else { "writes" };
            (function.to_string(), kind.to_string(), normalize_path(&text(&a["path"])), line(&a["span"]), keys(&a["keys"], reference))
        })
        .collect()
}

#[tokio::test]
async fn mapping_keys_are_the_same_in_every_surface() {
    let s = Surfaces::of("keys").await;
    // `evm:<file>:<Contract>.<name>(<types>)` as `<Contract>.<name>`
    let member = |id: &str| id.rsplit_once(':').unwrap().1.split('(').next().unwrap().to_string();
    let function = |id: &str| member(id).split_once('.').unwrap().1.to_string();
    let accesses = || s.records("reads").chain(s.records("writes"));
    let exported: BTreeSet<KeySite> = accesses()
        .flat_map(|r| {
            r["key_sites"].as_array().unwrap().iter().map(move |site| {
                (function(&text(&r["function"])), text(&r["kind"]), text(&r["path"]), line(&site["span"]), keys(&site["keys"], member))
            })
        })
        .collect();
    assert!(exported.iter().any(|(_, _, _, _, k)| k.iter().any(|(_, kind, _)| kind == "msg_sender")), "{exported:#?}");

    let detail = s.web("/api/contract/Keys").await;
    let functions = detail["functions"].as_array().unwrap();
    let web: BTreeSet<KeySite> = functions.iter().flat_map(|f| api_sites(&text(&f["name"]), &f["accesses"])).collect();
    assert_eq!(web, exported, "/contract/Keys");

    let mut mcp = BTreeSet::new();
    for f in functions {
        let name = text(&f["name"]);
        let narrative = s.mcp("ilold_function_analysis", serde_json::json!({"contract": "Keys", "function": name})).await;
        mcp.extend(api_sites(&name, narrative.get("accesses").unwrap_or(&serde_json::json!([]))));
    }
    assert_eq!(mcp, exported, "ilold_function_analysis");

    for var in ["balances", "allowance", "slots", "positions", "total"] {
        let exported_var: BTreeSet<(String, String, u64, Keys)> = exported
            .iter()
            .filter(|(_, _, path, ..)| path.split(['[', '.']).next() == Some(var))
            .map(|(f, kind, _, line, keys)| (f.clone(), kind.clone(), *line, keys.clone()))
            .collect();
        let transitive: BTreeSet<(String, String)> = accesses()
            .filter(|r| r["keys_reason"] == "transitive" && text(&r["path"]).split(['[', '.']).next() == Some(var))
            .map(|r| (function(&text(&r["function"])), text(&r["kind"])))
            .collect();
        let who = &s.mcp("ilold_who_touches", serde_json::json!({"contract": "Keys", "variable": var})).await["VariableInfo"]["accesses"];
        let mut who_sites = BTreeSet::new();
        let mut who_transitive = BTreeSet::new();
        for a in who.as_array().unwrap() {
            let kind = if a["kind"] == "read" { "reads" } else { "writes" };
            if a["keys_reason"] == "transitive" {
                who_transitive.insert((text(&a["function"]), kind.to_string()));
            }
            for site in a["key_sites"].as_array().unwrap() {
                who_sites.insert((text(&a["function"]), kind.to_string(), line(&site["span"]), keys(&site["keys"], |r| r.replace("::", "."))));
            }
        }
        assert_eq!(who_sites, exported_var, "ilold_who_touches {var}");
        assert_eq!(who_transitive, transitive, "ilold_who_touches {var}: transitive");
    }
}

/// A catch clause as every surface shows it: kind, first line, empty, written paths, call texts, events and whether it reverts.
type Clause = (String, u64, bool, Vec<String>, Vec<String>, Vec<String>, bool);
/// A try statement: function, line, guarded call text and clauses.
type Try = (String, u64, String, Vec<Clause>);

/// `fields` name the text of a write, a call and an event: the export says path and event, the API text.
fn clauses(fact: &Value, fields: [&str; 3]) -> Vec<Clause> {
    fact["catch_clauses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let [writes, calls, emits] = [("writes", fields[0]), ("calls", fields[1]), ("emits", fields[2])]
                .map(|(list, field)| c["effects"][list].as_array().unwrap().iter().map(|e| text(&e[field])).collect::<Vec<_>>());
            (text(&c["kind"]), line(&c["span"]), c["empty"] == true, writes, calls, emits, c["effects"]["reverts"] == true)
        })
        .collect()
}

fn api_tries(function: &str, facts: Option<&Value>) -> BTreeSet<Try> {
    facts
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|f| (function.to_string(), line(&f["span"]), text(&f["guarded_call"]["text"]), clauses(f, ["text"; 3])))
        .collect()
}

#[tokio::test]
async fn try_catches_are_the_same_in_every_surface() {
    let s = Surfaces::of("trycatch").await;
    let function = |id: &str| id.rsplit_once('.').unwrap().1.split('(').next().unwrap().to_string();
    let calls: std::collections::BTreeMap<String, &Value> = s.records("calls").map(|c| (text(&c["id"]), c)).collect();
    let exported: BTreeSet<Try> = s
        .records("try_catch")
        .map(|t| (function(&text(&t["function"])), line(&t["span"]), text(&calls[&text(&t["guarded_call"])]["target_text"]), clauses(t, ["path", "text", "event"])))
        .collect();
    assert_eq!(exported.len(), 8, "{exported:#?}");
    assert!(exported.iter().any(|(_, _, _, c)| c.iter().any(|(kind, ..)| kind == "bytes")) && exported.iter().any(|(_, _, _, c)| c.iter().any(|(kind, ..)| kind == "all")));

    let detail = s.web("/api/contract/TryCatch").await;
    let functions: Vec<String> = detail["functions"].as_array().unwrap().iter().map(|f| text(&f["name"])).collect();
    let summaries: BTreeSet<Try> = detail["functions"].as_array().unwrap().iter().flat_map(|f| api_tries(&text(&f["name"]), f.get("try_catches"))).collect();
    assert_eq!(summaries, exported, "/contract/TryCatch");

    let (mut web, mut cfg, mut narrative) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for name in &functions {
        web.extend(api_tries(name, s.web(&format!("/api/contract/TryCatch/{name}/cfg")).await.get("try_catches")));
        let args = serde_json::json!({"contract": "TryCatch", "function": name});
        cfg.extend(api_tries(name, s.mcp("ilold_cfg", args.clone()).await.get("try_catches")));
        narrative.extend(api_tries(name, s.mcp("ilold_function_analysis", args).await.get("try_catches")));
    }
    assert_eq!(web, exported, "/cfg");
    assert_eq!(cfg, exported, "ilold_cfg");
    assert_eq!(narrative, exported, "ilold_function_analysis");
}

/// A value transfer as every surface shows it: function, line, asset, operation, token, from, to, amount, id, library function, tag.
type Transfer = (String, u64, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, String);
/// A balance read: function, line, asset, token, owner, id, library function, tag.
type Read = (String, u64, String, Option<String>, Option<String>, Option<String>, Option<String>, String);

fn opt(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// The facts a surface lists under one field, none when it leaves the field out.
fn listed(facts: Option<&Value>) -> impl Iterator<Item = &Value> {
    facts.and_then(Value::as_array).into_iter().flatten()
}

/// `library` turns the surface's library reference into `<library>::<function>`.
fn transfers<'a>(function: &str, facts: impl Iterator<Item = &'a Value>, library: &dyn Fn(&Value) -> Option<String>) -> BTreeSet<Transfer> {
    let fields = |t: &Value| [&t["token_expr"], &t["from_expr"], &t["to_expr"], &t["amount_expr"], &t["id_expr"]].map(opt);
    facts
        .map(|t| {
            let [token, from, to, amount, id] = fields(t);
            (function.to_string(), line(&t["span"]), text(&t["asset_kind"]), text(&t["operation"]), token, from, to, amount, id, library(&t["via_library"]), text(&t["tag"]))
        })
        .collect()
}

fn reads<'a>(function: &str, facts: impl Iterator<Item = &'a Value>, library: &dyn Fn(&Value) -> Option<String>) -> BTreeSet<Read> {
    facts
        .map(|r| {
            let [token, owner, id] = [&r["token_expr"], &r["owner_expr"], &r["id_expr"]].map(opt);
            (function.to_string(), line(&r["span"]), text(&r["asset_kind"]), token, owner, id, library(&r["via_library"]), text(&r["tag"]))
        })
        .collect()
}

#[tokio::test]
async fn value_transfers_and_balance_reads_are_the_same_in_every_surface() {
    let s = Surfaces::of("value").await;
    let payments = "evm:tests/fixtures/solc/value/src/Value.sol:Payments.";
    let member = |id: &str| id.rsplit_once(':').unwrap().1.split('(').next().unwrap().to_string();
    let exported_library = |v: &Value| v.as_str().map(|id| member(id).replacen('.', "::", 1));
    let api_library = |v: &Value| opt(v);
    let mut exported = (BTreeSet::new(), BTreeSet::new());
    for record in s.records("value_transfer").chain(s.records("balance_read")) {
        let Some(function) = text(&record["function"]).strip_prefix(payments).map(|f| f.split('(').next().unwrap().to_string()) else { continue };
        if record["kind"] == "value_transfer" {
            exported.0.extend(transfers(&function, std::iter::once(record), &exported_library));
        } else {
            exported.1.extend(reads(&function, std::iter::once(record), &exported_library));
        }
    }
    assert_eq!((exported.0.len(), exported.1.len()), (29, 8), "{exported:#?}");

    let summaries = |detail: &Value| {
        let mut found = (BTreeSet::new(), BTreeSet::new());
        for f in detail["functions"].as_array().unwrap() {
            found.0.extend(transfers(&text(&f["name"]), listed(f.get("value_transfers")), &api_library));
            found.1.extend(reads(&text(&f["name"]), listed(f.get("balance_reads")), &api_library));
        }
        found
    };
    let detail = s.web("/api/contract/Payments").await;
    assert_eq!(summaries(&detail), exported, "/contract/Payments");
    let mcp_detail = s.mcp("ilold_contract_detail", serde_json::json!({"contract": "Payments"})).await;
    assert_eq!(summaries(&mcp_detail), exported, "ilold_contract_detail");
    assert_eq!(mcp_detail["value_flows"], detail["value_flows"], "ilold_contract_detail value_flows");

    let mut narrative = (BTreeSet::new(), BTreeSet::new());
    for f in detail["functions"].as_array().unwrap() {
        let name = text(&f["name"]);
        let analysis = s.mcp("ilold_function_analysis", serde_json::json!({"contract": "Payments", "function": name})).await;
        narrative.0.extend(transfers(&name, listed(analysis.get("value_transfers")), &api_library));
        narrative.1.extend(reads(&name, listed(analysis.get("balance_reads")), &api_library));
    }
    assert_eq!(narrative, exported, "ilold_function_analysis");
}
