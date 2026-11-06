//! `ilold graph --json`: exit codes, determinism, git-root-relative ids and one sample of every record kind.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ilold_core::export::as_1_0;
use ilold_core::parse::digest;
use serde_json::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn fixture(name: &str) -> PathBuf {
    repo().join("tests/fixtures/solc").join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "ilold-graph-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn graph(project: &Path, out: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ilold"))
        .arg("graph")
        .arg("--json")
        .arg(out)
        .arg(project)
        .env("ILOLD_PROJECT_CACHE_DIR", cache)
        .env_remove("ILOLD_DISABLE_PROJECT_CACHE")
        .env_remove("FOUNDRY_PROFILE")
        .output()
        .expect("run ilold graph")
}

fn records(out: &Path) -> Vec<Value> {
    fs::read_to_string(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect()
}

/// Runs the export into a scratch directory and returns its records.
fn export(project: &Path, name: &str, code: i32) -> Vec<Value> {
    let dir = scratch(name);
    let out = dir.join("graph.jsonl");
    let run = graph(project, &out, &dir.join("cache"));
    assert_eq!(run.status.code(), Some(code), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let found = records(&out);
    fs::remove_dir_all(&dir).unwrap();
    found
}

fn of_kind<'a>(records: &'a [Value], kind: &str) -> Vec<&'a Value> {
    records.iter().filter(|r| r["kind"] == kind).collect()
}

fn find<'a>(records: &'a [Value], kind: &str, matches: impl Fn(&Value) -> bool) -> &'a Value {
    let found: Vec<&Value> = of_kind(records, kind).into_iter().filter(|r| matches(r)).collect();
    match found.as_slice() {
        [only] => only,
        _ => panic!("expected one {kind}, found {}: {found:#?}", found.len()),
    }
}

fn file_status<'a>(envelope: &'a Value, path: &str) -> &'a Value {
    envelope["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("{path} missing from {:#}", envelope["files"]))
}

const FACTS: &str = "tests/fixtures/solc/facts/src/Facts.sol";
const KINDS: &str = "tests/fixtures/solc/export/kinds";

fn facts_id(member: &str) -> String {
    format!("evm:{FACTS}:{member}")
}

fn kinds_id(file: &str, member: &str) -> String {
    format!("evm:{KINDS}/{file}:{member}")
}

fn is_hex(value: &Value, len: usize) -> bool {
    value.as_str().is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[test]
fn every_record_has_the_common_fields_and_sorted_ids() {
    let records = export(&fixture("facts"), "common", 0);
    assert_eq!(records[0]["kind"], "envelope");
    let body = &records[1..];
    let nodes = body.iter().take_while(|r| is_node(r)).count();
    assert!(body[nodes..].iter().all(|r| !is_node(r)), "nodes come first");
    for part in [&body[..nodes], &body[nodes..]] {
        let ids: Vec<&str> = part.iter().map(|r| r["id"].as_str().unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids, sorted, "sorted by id, ids unique");
    }
    for record in body {
        let tag = record["tag"].as_str().unwrap_or_else(|| panic!("tag: {record}"));
        assert!(["EXTRACTED", "TYPE_RESOLVED", "INFERRED", "AMBIGUOUS"].contains(&tag), "{record}");
        assert!(record["rule"].as_str().is_some_and(|r| !r.is_empty()), "{record}");
        let span = &record["span"];
        assert!(span["file"].as_str().is_some_and(|f| f.starts_with("tests/fixtures/solc/facts/")), "{record}");
        assert!(span["start_line"].as_u64().is_some() && span["end_line"].as_u64().is_some(), "{record}");
    }
}

fn is_node(record: &Value) -> bool {
    ["contract", "function", "state_var", "event", "error", "modifier"].contains(&record["kind"].as_str().unwrap_or(""))
}

#[test]
fn exit_0_on_a_complete_export_with_guards_calls_and_code_facts() {
    let records = export(&fixture("facts"), "facts", 0);
    let envelope = &records[0];
    assert_eq!(envelope["status"], "complete");
    assert_eq!(envelope["schema_version"], "1.0.0");
    assert_eq!(envelope["project_root"], "tests/fixtures/solc/facts");
    assert_eq!(envelope["solc_versions"], serde_json::json!(["0.8.28"]));
    assert_eq!(envelope["profile"], "default");
    assert!(is_hex(&envelope["remappings_sha256"], 64) && is_hex(&envelope["target"]["sources_sha256"], 64), "{envelope}");
    assert!(is_hex(&envelope["target"]["git_commit"], 40), "{envelope}");
    let tool = &envelope["tool"];
    let built_in_git = is_hex(&tool["git_commit"], 40) && tool["dirty"].is_boolean();
    let built_elsewhere = tool["git_commit"].is_null() && tool["dirty"].is_null();
    assert!(built_in_git || built_elsewhere, "{tool}");
    assert_eq!((&tool["name"], &tool["version"]), (&Value::from("ilold"), &Value::from(env!("CARGO_PKG_VERSION"))));
    assert!(envelope["target"]["dirty"].is_boolean(), "{envelope}");
    assert_eq!(file_status(envelope, FACTS)["status"], "analysed");

    let guard = |function: &str| find(&records, "guards", |g| g["function"] == facts_id(function) && g["guard_kind"] == "caller_equals");
    let one_branch = guard("Guards.oneBranch(uint256)");
    assert_eq!((&one_branch["scope"], &one_branch["tag"]), (&Value::from("conditional"), &Value::from("INFERRED")));
    let after_write = guard("Guards.writeThenCheck(uint256)");
    assert_eq!((&after_write["scope"], &after_write["tag"]), (&Value::from("unconditional"), &Value::from("EXTRACTED")));
    assert_eq!(after_write["compared_against"]["kind"], "state_var");
    assert_eq!(after_write["compared_against"]["id"], facts_id("Guards.owner"));
    assert_eq!(after_write["in_context"], facts_id("Guards"));
    assert_eq!(guard("Guards.toCheck(address)")["compared_against"], serde_json::json!({"kind": "param", "index": 0, "name": "to"}));
    let role = find(&records, "guards", |g| g["function"] == facts_id("Guards.roleViaModifier()"));
    assert_eq!(role["via"], serde_json::json!([facts_id("Guards.onlyRole")]));
    assert_eq!(role["checked_by"], "hasRole");
    assert_eq!(role["role"], serde_json::json!({"kind": "constant", "name": "ADMIN", "id": facts_id("Guards.ADMIN")}));
    assert_eq!((&role["account"], &role["role_implied_by"]), (&serde_json::json!({"kind": "msg_sender"}), &Value::Null));
    assert_eq!((&role["compared_against"], &role["alternatives"]), (&Value::Null, &serde_json::json!([])));
    let either = find(&records, "guards", |g| g["function"] == facts_id("Guards.eitherRole()"));
    let implied: Vec<&Value> = either["alternatives"].as_array().unwrap().iter().map(|a| &a["role_implied_by"]).collect();
    assert_eq!(implied, ["isA", "isB"]);
    for field in ["checked_by", "role", "role_implied_by", "account", "compared_against"] {
        assert_eq!(either[field], Value::Null, "{field}: a compound guard leaves it to its parts");
    }
    assert_eq!((&either["restricts"], &role["restricts"]), (&Value::from(true), &Value::from(true)));
    let open = find(&records, "guards", |g| g["function"] == facts_id("Guards.ownerOrOpen()"));
    assert_eq!((&open["restricts"], &open["guard_kind"], &open["account"]), (&Value::from(false), &Value::from("caller_equals"), &Value::Null));
    assert_eq!((&open["alternatives"][1]["guard_kind"], &open["alternatives"][1]["condition_text"]), (&Value::from("condition"), &Value::from("open")));
    let alternative = &either["alternatives"][1];
    assert_eq!((&alternative["checked_by"], &alternative["role"]), (&Value::from("isB"), &Value::Null));
    assert_eq!((&alternative["guard_kind"], &alternative["account"]), (&Value::from("role_check"), &serde_json::json!({"kind": "msg_sender"})));
    let guard_of = |function: &str| find(&records, "guards", |g| g["function"] == facts_id(function));
    let unpaused = &guard_of("Guards.ownerUnpaused()")["conjuncts"];
    assert_eq!((&unpaused[1]["guard_kind"], &unpaused[1]["condition_text"], &unpaused[1]["negated"]), (&Value::from("pause"), &Value::from("paused"), &Value::from(true)));
    assert_eq!(unpaused[0]["conjuncts"], serde_json::json!([]));
    let deny = guard_of("Guards.notBlacklisted()");
    assert_eq!((&deny["negated"], &deny["tag"]), (&Value::from(true), &Value::from("EXTRACTED")));
    let unreadable = guard_of("Guards.unreadable()");
    assert_eq!((&unreadable["tag"], &unreadable["conjuncts"]), (&Value::from("AMBIGUOUS"), &serde_json::json!([])));
    assert_eq!(unreadable["condition_text"], "require(hasRole(ADMIN, msg.sender) && uint160(msg.sender) > 7)");

    let call = |function: &str| find(&records, "calls", |c| c["from"] == facts_id(function));
    let from_state = call("Targets.fromState()");
    assert_eq!(as_1_0(from_state)["target_source"], serde_json::json!({"kind": "state_var", "name": "pool", "id": facts_id("Targets.pool")}));
    assert_eq!(from_state["target_source"]["assigned_at"], serde_json::json!([]), "never assigned");
    assert_eq!((&from_state["kind"], &from_state["call_kind"]), (&Value::from("calls"), &Value::from("external")));
    assert_eq!(from_state["to"], facts_id("IPool.supply(address,uint256)"));
    assert_eq!(from_state["tag"], "TYPE_RESOLVED");
    assert_eq!(call("Targets.fromSender()")["target_source"], serde_json::json!({"kind": "msg_sender"}));
    let ignored = call("LowLevel.ignored(address)");
    assert_eq!((&ignored["to"], &ignored["tag"], &ignored["result_used"]), (&Value::Null, &Value::from("AMBIGUOUS"), &Value::from(false)));
    assert_eq!(ignored["call_kind"], "low_level_call");
    assert_eq!(call("LowLevel.checked(address)")["result_used"], true);

    let builtins: Vec<&Value> = of_kind(&records, "builtin_use").into_iter()
        .filter(|b| b["function"] == facts_id("Builtins.all(bytes32,uint8,bytes32,bytes32,address)"))
        .map(|b| &b["name"]).collect();
    for name in ["ecrecover", "tx.origin", "block.timestamp", "gasleft", "msg.value", "abi.encodePacked"] {
        assert!(builtins.contains(&&Value::from(name)), "{name}: {builtins:?}");
    }
    find(&records, "unchecked_block", |u| u["function"] == facts_id("Builtins.all(bytes32,uint8,bytes32,bytes32,address)"));
    let asm = find(&records, "opaque_assembly", |a| a["function"] == facts_id("Asm.writesAsm()"));
    assert_eq!(asm["rule"], "cfg");
    let writes_asm = find(&records, "function", |f| f["id"] == facts_id("Asm.writesAsm()"));
    let flags = find_context(writes_asm, &facts_id("Asm"));
    assert_eq!((&flags["effects_incomplete"], &flags["reverts_incomplete"]), (&Value::from(true), &Value::from(true)));
    let deep = find(&records, "function", |f| f["id"] == facts_id("Guards.deep()"));
    assert_eq!(find_context(deep, &facts_id("Guards"))["guards_incomplete"], true);

    let rare = find(&records, "writes", |w| w["function"] == facts_id("PathCap.branches(uint256)") && w["state_var"] == facts_id("PathCap.rare"));
    assert_eq!((&rare["path"], &rare["tag"]), (&Value::from("rare"), &Value::from("TYPE_RESOLVED")));
    let branches = find(&records, "function", |f| f["id"] == facts_id("PathCap.branches(uint256)"));
    assert!(branches["paths"]["total_paths"].as_u64().is_some(), "{branches}");

    let exposed = find(&records, "exposes", |e| e["contract"] == facts_id("Guards") && e["function"] == facts_id("Guards.oneBranch(uint256)"));
    assert_eq!(exposed["via_base"], Value::Null);
    let truncation = &envelope["truncation"];
    assert!(truncation["guards_incomplete"].as_u64().unwrap() >= 1, "{truncation}");
    assert!(truncation["limits"]["max_paths"].as_u64().is_some(), "{truncation}");
}

fn find_context<'a>(function: &'a Value, context: &str) -> &'a Value {
    function["analysis"].as_array().unwrap().iter().find(|a| a["in_context"] == context)
        .unwrap_or_else(|| panic!("{context} missing: {function}"))
}

#[test]
fn call_resolution_records_dispatch_inheritance_and_exposure() {
    let records = export(&fixture("call_resolution"), "calls", 0);
    let id = |file: &str, member: &str| format!("evm:tests/fixtures/solc/call_resolution/src/{file}:{member}");
    let leaf = id("Hierarchy.sol", "Leaf");
    let leaf_node = find(&records, "contract", |c| c["id"] == leaf);
    assert_eq!(leaf_node["bases"], serde_json::json!([id("Hierarchy.sol", "Middle"), id("Hierarchy.sol", "Root")]));
    assert_eq!(leaf_node["deployable"], true);
    let inherits = find(&records, "inherits", |e| e["child"] == leaf && e["base"] == id("Hierarchy.sol", "Root"));
    assert_eq!(inherits["order"], 2);

    let hook = find(&records, "calls", |c| {
        c["from"] == id("Hierarchy.sol", "Root.run(uint256)") && c["in_context"] == leaf && c["target_text"] == "hook"
    });
    assert_eq!(hook["static_target"], id("Hierarchy.sol", "Root.hook()"));
    assert_eq!(hook["dispatch_target"], id("Hierarchy.sol", "Leaf.hook()"));
    assert_eq!(hook["to"], id("Hierarchy.sol", "Leaf.hook()"));
    assert_eq!(hook["call_kind"], "internal");

    let run = find(&records, "exposes", |e| e["contract"] == leaf && e["function"] == id("Hierarchy.sol", "Root.run(uint256)"));
    assert_eq!(run["via_base"], id("Hierarchy.sol", "Root"));
    let over = find(&records, "function", |f| f["id"] == id("Overloads.sol", "Over.put(uint256)"));
    assert!(is_hex(&over["selector"], 8), "{over}");
    assert_eq!(over["signature"], "put(uint256)");
    find(&records, "function", |f| f["id"] == id("Overloads.sol", "Over.receive()"));

    let tracked = find(&records, "writes", |w| {
        w["function"] == id("Hierarchy.sol", "Leaf.move(uint256)") && w["path"] == "tracked" && w["in_context"] == leaf
    });
    assert_eq!(tracked["via"], serde_json::json!([id("Hierarchy.sol", "Root.track"), id("Hierarchy.sol", "Root._track()")]));
    assert_eq!(tracked["rule"], "effects");
}

#[test]
fn every_node_kind_dependency_stubs_and_ambiguous_facts() {
    let records = export(&fixture("export/kinds"), "kinds", 0);
    let envelope = &records[0];
    assert_eq!(file_status(envelope, &format!("{KINDS}/src/Vault.sol"))["status"], "analysed");
    assert_eq!(file_status(envelope, &format!("{KINDS}/lib/dep/Ownable.sol"))["status"], "dependency");
    let unused = file_status(envelope, &format!("{KINDS}/lib/dep/Unused.sol"));
    assert_eq!(unused["status"], "excluded");
    assert!(unused["reason"].as_str().is_some_and(|r| !r.is_empty()), "{unused}");
    assert_eq!(file_status(envelope, &format!("{KINDS}/test/Vault.t.sol"))["status"], "test_or_script");

    let vault = kinds_id("src/Vault.sol", "Vault");
    find(&records, "event", |e| e["id"] == kinds_id("src/Vault.sol", "Vault.Deposited(address,uint256)"));
    find(&records, "event", |e| e["id"] == kinds_id("src/Vault.sol", "Vault.Deposited(address)"));
    find(&records, "event", |e| e["id"] == kinds_id("lib/dep/Ownable.sol", "Ownable.OwnerSet"));
    find(&records, "error", |e| e["id"] == kinds_id("src/Vault.sol", "Vault.TooSmall"));
    find(&records, "modifier", |m| m["id"] == kinds_id("lib/dep/Ownable.sol", "Ownable.onlyOwner"));
    let total = find(&records, "state_var", |v| v["id"] == kinds_id("src/Vault.sol", "Vault.total"));
    assert_eq!((&total["type"], &total["order"]), (&Value::from("uint256"), &Value::from(0)));
    find(&records, "function", |f| f["id"] == kinds_id("src/Vault.sol", "Vault._sum(uint256[] storage)"));
    find(&records, "function", |f| f["id"] == kinds_id("src/Vault.sol", "Vault._sum(uint256[] memory)"));
    let clamp = find(&records, "function", |f| f["id"] == kinds_id("src/Vault.sol", "clamp(uint256)"));
    assert_eq!(clamp["contract"], Value::Null);

    let ownable = find(&records, "contract", |c| c["id"] == kinds_id("lib/dep/Ownable.sol", "Ownable"));
    assert_eq!((&ownable["is_dependency"], &ownable["reachable_from_project"]), (&Value::from(true), &Value::from(true)));
    let feed = find(&records, "contract", |c| c["id"] == kinds_id("lib/dep/MathLib.sol", "IFeed"));
    assert_eq!(feed["is_dependency"], true);
    find(&records, "function", |f| f["id"] == kinds_id("lib/dep/MathLib.sol", "MathLib.twice(uint256)"));
    assert!(of_kind(&records, "function").iter().all(|f| f["id"] != kinds_id("lib/dep/MathLib.sol", "MathLib.thrice(uint256)")),
        "unreached dependency functions get no facts");
    let twice = find(&records, "reverts", |r| r["function"] == kinds_id("lib/dep/MathLib.sol", "MathLib.twice(uint256)"));
    assert_eq!(twice["in_context"], kinds_id("lib/dep/MathLib.sol", "MathLib"));

    let deposit = kinds_id("src/Vault.sol", "Vault.deposit(uint256)");
    let emitted = find(&records, "emits", |e| e["function"] == deposit);
    assert_eq!(emitted["event"], kinds_id("src/Vault.sol", "Vault.Deposited(address,uint256)"));
    assert_eq!(emitted["tag"], "TYPE_RESOLVED", "an overload resolves by declaration id");
    let ping = find(&records, "emits", |e| e["function"] == kinds_id("src/Vault.sol", "Vault.ping()"));
    assert_eq!((&ping["event"], &ping["tag"]), (&Value::from(kinds_id("src/Vault.sol", "Vault.Deposited(address)")), &Value::from("TYPE_RESOLVED")));
    find(&records, "event", |e| e["id"] == kinds_id("src/Vault.sol", "Swept"));
    let touch = kinds_id("src/Vault.sol", "Vault.touch()");
    let announce = kinds_id("src/Vault.sol", "Vault._announce()");
    for event in [kinds_id("src/Vault.sol", "Vault.Deposited(address)"), kinds_id("src/Vault.sol", "Swept")] {
        let direct = find(&records, "emits", |e| e["function"] == announce && e["event"] == event);
        assert_eq!((&direct["tag"], &direct["via"]), (&Value::from("TYPE_RESOLVED"), &serde_json::json!([])), "{event}");
        let lifted = find(&records, "emits", |e| e["function"] == touch && e["event"] == event);
        assert_eq!((&lifted["tag"], &lifted["via"]), (&Value::from("TYPE_RESOLVED"), &serde_json::json!([announce])), "{event}");
    }
    assert!(of_kind(&records, "emits").iter().all(|e| e["tag"] != "AMBIGUOUS"), "every emit resolves");

    let set_feed = kinds_id("src/Vault.sol", "Vault.setFeed(IFeed)");
    let only_owner = serde_json::json!([kinds_id("lib/dep/Ownable.sol", "Ownable.onlyOwner")]);
    let read = find(&records, "reads", |r| r["function"] == set_feed && r["path"] == "owner");
    assert_eq!(read["via"], only_owner, "a read in modifier code names the modifier");
    let revert = find(&records, "reverts", |r| r["function"] == set_feed);
    assert_eq!(revert["via"], only_owner, "a revert in modifier code names the modifier");
    let body_read = find(&records, "reads", |r| r["function"] == kinds_id("src/Vault.sol", "Vault.price()") && r["path"] == "feed");
    assert_eq!(body_read["via"], serde_json::json!([]));
    let owner_set = find(&records, "emits", |e| e["function"] == kinds_id("src/Vault.sol", "Vault.constructor(IFeed)"));
    assert_eq!(owner_set["event"], kinds_id("lib/dep/Ownable.sol", "Ownable.OwnerSet"));
    assert_eq!(owner_set["via"], serde_json::json!([kinds_id("lib/dep/Ownable.sol", "Ownable._setOwner(address)")]));
    find(&records, "reverts", |r| r["function"] == deposit && r["via"] == serde_json::json!([]) && r["reason"].as_str().unwrap().contains("TooSmall"));
    let free = find(&records, "calls", |c| c["from"] == deposit && c["target_text"] == "clamp");
    assert_eq!(free["to"], kinds_id("src/Vault.sol", "clamp(uint256)"));
    let price = find(&records, "calls", |c| c["from"] == kinds_id("src/Vault.sol", "Vault.price()"));
    assert_eq!(price["to"], kinds_id("lib/dep/MathLib.sol", "IFeed.price()"));

    let spawn = find(&records, "calls", |c| c["from"] == kinds_id("src/Vault.sol", "Vault.spawn(uint256)"));
    assert_eq!((&spawn["to"], &spawn["tag"], &spawn["call_kind"]), (&Value::from(kinds_id("src/Vault.sol", "Receipt")), &Value::from("INFERRED"), &Value::from("creation")));
    assert_eq!(spawn["options"], serde_json::json!({"salt": "bytes32(amount)"}));

    let gated = find(&records, "uses_modifier", |m| m["function"] == kinds_id("src/Vault.sol", "Vault.setFeed(IFeed)"));
    assert_eq!((&gated["modifier"], &gated["order"]), (&Value::from(kinds_id("lib/dep/Ownable.sol", "Ownable.onlyOwner")), &Value::from(0)));
    let pending = find(&records, "uses_modifier", |m| m["function"] == kinds_id("src/Vault.sol", "Pending.mark()"));
    assert_eq!(pending["modifier"], kinds_id("src/Vault.sol", "Pending.pending"), "declared without code");
    assert_eq!((&pending["tag"], &pending["name"]), (&Value::from("AMBIGUOUS"), &Value::from("pending")));

    let getter = find(&records, "exposes", |e| e["contract"] == vault && e["function"] == kinds_id("lib/dep/Ownable.sol", "Ownable.owner"));
    assert_eq!((&getter["getter"], &getter["via_base"]), (&Value::from(true), &Value::from(kinds_id("lib/dep/Ownable.sol", "Ownable"))));
}

#[test]
fn output_is_byte_identical_across_runs_and_cold_or_warm_caches() {
    let dir = scratch("determinism");
    let cache = dir.join("cache");
    let outputs: Vec<Vec<u8>> = (0..3)
        .map(|run| {
            let out = dir.join(format!("run-{run}.jsonl"));
            // the third run uses a cold cache of its own
            let cache = if run == 2 { dir.join("fresh-cache") } else { cache.clone() };
            let output = graph(&fixture("export/kinds"), &out, &cache);
            assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
            if run == 1 {
                assert!(String::from_utf8_lossy(&output.stderr).contains("ilold analysis cache hit"), "warm run");
            }
            fs::read(&out).unwrap()
        })
        .collect();
    assert!(outputs[0] == outputs[1] && outputs[1] == outputs[2], "runs differ");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn output_holds_no_absolute_path_or_timestamp() {
    let dir = scratch("paths");
    let out = dir.join("graph.jsonl");
    assert_eq!(graph(&fixture("export/kinds"), &out, &dir.join("cache")).status.code(), Some(0));
    let text = fs::read_to_string(&out).unwrap();
    let home = std::env::var("HOME").unwrap();
    for needle in [repo().to_string_lossy().as_ref(), home.as_str(), "/Users/", "/home/", "/private/", "/tmp/", "\"/"] {
        assert!(!text.contains(needle), "{needle} in the output");
    }
    let year_like = text.as_bytes().windows(11).any(|w| {
        w[..4].iter().all(u8::is_ascii_digit) && w[4] == b'-' && w[5..7].iter().all(u8::is_ascii_digit) && w[7] == b'-'
            && w[8..10].iter().all(u8::is_ascii_digit) && w[10] == b'T'
    });
    assert!(!year_like, "timestamp-like text in the output");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ids_are_relative_to_the_git_root_of_a_nested_project() {
    let dir = scratch("git");
    let project = dir.join("repo/contracts/core");
    copy_dir(&fixture("facts"), &project);
    let head = commit_all(&dir.join("repo"));

    let out = dir.join("graph.jsonl");
    let run = graph(&project.join("src"), &out, &dir.join("cache"));
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    let records = records(&out);
    let envelope = &records[0];
    assert_eq!(envelope["project_root"], "contracts/core");
    assert_eq!(envelope["target"]["git_commit"], head.as_str());
    assert_eq!(envelope["target"]["dirty"], false);
    assert_eq!(file_status(envelope, "contracts/core/src/Facts.sol")["status"], "analysed");
    find(&records, "contract", |c| c["id"] == "evm:contracts/core/src/Facts.sol:Guards");
    assert!(records[1..].iter().all(|r| r["span"]["file"].as_str().is_some_and(|f| f.starts_with("contracts/core/"))));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn edge_ids_survive_moving_unrelated_code() {
    let dir = scratch("moved");
    let repo = dir.join("repo");
    copy_dir(&fixture("facts"), &repo);
    commit_all(&repo);
    let edge_ids = |name: &str| -> Vec<String> {
        let out = dir.join(name);
        let run = graph(&repo, &out, &dir.join("cache"));
        assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
        records(&out)[1..].iter().filter(|r| !is_node(r))
            .map(|r| r["id"].as_str().unwrap().to_string()).filter(|id| !id.contains("Moved")).collect()
    };
    let before = edge_ids("before.jsonl");
    let source = repo.join("src/Facts.sol");
    let text = fs::read_to_string(&source).unwrap();
    let (head, rest) = text.split_once("// Facts a code map").unwrap();
    fs::write(&source, format!("{head}\n\n\ncontract Moved {{\n    function added() external {{}}\n}}\n\n// Facts a code map{rest}")).unwrap();
    let after = edge_ids("after.jsonl");
    assert!(before.len() > 100, "{}", before.len());
    let changed: Vec<&String> = before.iter().filter(|id| !after.contains(id)).collect();
    assert!(changed.is_empty() && before.len() == after.len(), "{changed:?}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn identical_calls_in_one_function_get_distinct_ids_ordered_by_position() {
    let records = export(&fixture("facts"), "twice", 0);
    let calls: Vec<&Value> = of_kind(&records, "calls").into_iter().filter(|c| c["from"] == facts_id("Targets.twice()")).collect();
    let ids: Vec<&str> = calls.iter().map(|c| c["id"].as_str().unwrap()).collect();
    let [first, second] = ids.as_slice() else { panic!("{ids:?}") };
    let prefix = format!("calls:{}#", facts_id("Targets.twice()"));
    let hash = |id: &str| id.strip_prefix(&prefix).and_then(|h| h.split_once('.')).map(|(h, n)| (h.to_string(), n.to_string()));
    let ((h0, n0), (h1, n1)) = (hash(first).unwrap(), hash(second).unwrap());
    assert!(h0 == h1 && is_hex(&Value::from(h0.as_str()), 16), "{ids:?}");
    assert_eq!((n0.as_str(), n1.as_str()), ("0", "1"));
    assert!(calls[0]["span"]["start_line"].as_u64() < calls[1]["span"]["start_line"].as_u64(), "{calls:?}");
}

#[cfg(unix)]
#[test]
fn output_does_not_depend_on_where_the_project_lives() {
    let dir = scratch("relocated");
    let warning = |lib: &str| serde_json::json!({"kind": "empty_dependency", "message": format!("{lib} is empty (git submodule not initialised?)")});
    for (case, populated, code, expected) in
        [("complete", true, 0, vec![]), ("failing", false, 4, vec![warning("lib/dep"), warning("lib/gone")])]
    {
        let copies = [dir.join(case).join("first/repo"), dir.join(case).join("second/deeper/elsewhere")];
        // the second copy is reached through a symlink, so the given path is not the canonical one
        let given = [copies[0].clone(), dir.join(case).join("link")];
        fs::create_dir_all(&copies[1]).unwrap();
        std::os::unix::fs::symlink(&copies[1], &given[1]).unwrap();
        let outputs: Vec<String> = copies.iter().zip(&given).enumerate().map(|(i, (copy, given))| {
            copy_dir(&fixture("export/kinds"), copy);
            if !populated {
                fs::remove_dir_all(copy.join("lib/dep")).unwrap();
                fs::create_dir(copy.join("lib/dep")).unwrap();
            }
            let modules = "[submodule \"dep\"]\n\tpath = lib/dep\n\turl = https://example.invalid/dep\n";
            let gone = "[submodule \"gone\"]\n\tpath = lib/gone\n\turl = https://example.invalid/gone\n";
            fs::write(copy.join(".gitmodules"), if populated { modules.to_string() } else { format!("{modules}{gone}") }).unwrap();
            commit_all(copy);
            let out = dir.join(format!("{case}-{i}.jsonl"));
            let run = graph(given, &out, &dir.join(format!("cache-{case}-{i}")));
            assert_eq!(run.status.code(), Some(code), "{}", String::from_utf8_lossy(&run.stderr));
            fs::read_to_string(&out).unwrap()
        }).collect();
        assert!(outputs[0] == outputs[1], "{case}: the copies differ\n{}\n{}", outputs[0], outputs[1]);
        let mut needles = vec!["/private".to_string(), "/var".into(), "/Users".into(), "/tmp".into(), "\"/".into()];
        for path in copies.iter().chain(&given) {
            needles.extend(path.components().skip(1).map(|c| format!("/{}/", c.as_os_str().to_string_lossy())));
        }
        for needle in needles {
            assert!(!outputs[0].contains(&needle), "{case}: {needle} in {}", outputs[0]);
        }
        let envelope: Value = serde_json::from_str(outputs[0].lines().next().unwrap()).unwrap();
        let empty: Vec<&Value> = envelope["warnings"].as_array().unwrap().iter().filter(|w| w["kind"] == "empty_dependency").collect();
        assert_eq!(empty, expected.iter().collect::<Vec<_>>(), "{case}");
        if !populated {
            let vault = file_status(&envelope, "src/Vault.sol");
            assert_eq!(vault["status"], "failed_compile");
            assert!(vault["error"].as_str().is_some_and(|e| e.contains("dep/Ownable.sol")), "{vault}");
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A git repository at `repo` with everything in it committed; returns HEAD.
fn commit_all(repo: &Path) -> String {
    let git = |args: &[&str]| {
        // fixed dates keep HEAD identical for identical content
        let out = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z").env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .args(args).current_dir(repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "fixture"]);
    git(&["rev-parse", "HEAD"])
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().map(Result::unwrap) {
        let name = entry.file_name();
        if ["out", "cache"].contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to.join(name));
        } else {
            fs::copy(entry.path(), to.join(name)).unwrap();
        }
    }
}

#[test]
fn exit_1_on_a_tool_error_writes_no_file() {
    let dir = scratch("unwritable");
    let out = dir.join("missing/graph.jsonl");
    let run = graph(&fixture("facts"), &out, &dir.join("cache"));
    assert_eq!(run.status.code(), Some(1), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(!dir.join("missing").exists());

    let not_foundry = dir.join("empty");
    fs::create_dir_all(&not_foundry).unwrap();
    let out = dir.join("graph.jsonl");
    assert_eq!(graph(&not_foundry, &out, &dir.join("cache")).status.code(), Some(1));
    assert!(!out.exists(), "no output on a tool error");
    let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(leftovers.iter().all(|n| !n.to_string_lossy().contains("tmp")), "{leftovers:?}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn exit_2_on_bad_arguments() {
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_ilold")).args(args).output().unwrap();
    for args in [&["graph"][..], &["graph", "--json"], &["graph", "--json", "a.jsonl", ".", "extra"], &["graph", "--nope"]] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let help = String::from_utf8_lossy(&run(&["graph", "--help"]).stdout).into_owned();
    for needle in ["0  ", "1  ", "2  ", "3  ", "4  ", "analyze"] {
        assert!(help.contains(needle), "{needle} missing from: {help}");
    }
}

#[test]
fn exit_3_when_a_test_file_fails_and_the_sources_are_exported() {
    let records = export(&fixture("model/partial"), "partial", 3);
    let envelope = &records[0];
    assert_eq!(envelope["status"], "complete_with_warnings");
    let broken = file_status(envelope, "tests/fixtures/solc/model/partial/test/Broken.t.sol");
    assert_eq!(broken["status"], "failed_compile");
    assert!(broken["error"].as_str().is_some_and(|e| e.contains("not a number")), "{broken}");
    assert_eq!(file_status(envelope, "tests/fixtures/solc/model/partial/src/Good.sol")["status"], "analysed");
    assert!(!envelope["warnings"].as_array().unwrap().is_empty(), "{envelope}");
    find(&records, "contract", |c| c["id"] == "evm:tests/fixtures/solc/model/partial/src/Good.sol:Good");
}

#[test]
fn exit_4_on_a_compile_error_inside_the_sources_still_writes_the_inventory() {
    let records = export(&fixture("export/broken_src"), "broken", 4);
    assert_eq!(records.len(), 1, "envelope only: {records:#?}");
    let envelope = &records[0];
    assert_eq!(envelope["status"], "no_facts");
    let root = "tests/fixtures/solc/export/broken_src";
    let bad = file_status(envelope, &format!("{root}/src/Bad.sol"));
    assert_eq!(bad["status"], "failed_compile");
    assert!(bad["error"].as_str().is_some_and(|e| e.contains("not a number")), "{bad}");
    assert_eq!(bad["line"], 7);
    for entry in envelope["files"].as_array().unwrap() {
        let has_reason = entry["reason"].as_str().is_some_and(|r| !r.is_empty());
        let has_error = entry["error"].as_str().is_some_and(|e| !e.is_empty());
        match entry["status"].as_str() {
            Some("excluded") => assert!(has_reason, "an excluded file says why: {entry}"),
            Some("failed_compile") => assert!(has_error, "a failed file carries its error: {entry}"),
            _ => panic!("only excluded or failed files without facts: {entry}"),
        }
    }
    for other in ["src/Good.sol", "test/Good.t.sol"] {
        let entry = file_status(envelope, &format!("{root}/{other}"));
        assert_eq!(entry["status"], "excluded", "{entry}");
        assert!(entry["reason"].as_str().is_some_and(|r| r.contains("compile error")), "{entry}");
    }
}

/// Every fixture's export as schema 1.0 wrote it: exit code, record count, then sha256 of the sorted ids and of the sorted records.
const V1_0_EXPORTS: &[(&str, i32, usize, &str, &str)] = &[
    ("tests/fixtures/erc20", 0, 47, "c7e8c077ae99b3a1d125c6b319735235a3e11edc18dafb20ca581caf5025c7d2", "dc3d2fb7a0b32e0b9ccfa097179d7eb0e35998386d250213a9952ed59c80be8b"),
    ("tests/fixtures/foundry_config", 0, 6, "63ba4cc3c6aeea2266cbc1ed41955550558e30c6fdbefd0bad160d58cad5503a", "0e2476236a2dac7e03154ff366671654cba5b6482d1eb2ed3b3ce7518f36a5d2"),
    ("tests/fixtures/governor", 0, 124, "c9311c0188d17de8ed88ac3afdb974ff9666fe1f07938304695ca9042c2c1c6b", "607fc903649afca3e18232b8a0b5d90e658a5961d863528e6f37f83ddeed2524"),
    ("tests/fixtures/multi", 0, 36, "4e1f1aa4eb3e7849adfabc18cd0863bdfcc22a1295a181548f58e799211bd401", "b22d8f0a991108291ec87a1271b77accf111f2b23630fe7cf4e290655fb0801c"),
    ("tests/fixtures/nested_remappings", 0, 10, "843106a87f89e4b5000713ced26a12335be0b356001b0c7c7a229ae5b2b007f0", "77c5cc162a2662d4faa6de0de996488f315aef7713c8275afcc4a81817aea089"),
    ("tests/fixtures/simple_storage", 0, 11, "0336d194a3163368d68f884f34d15a305fb94db267d6ddbc5f56f58681a24e3e", "127237628110adcbe7071c8d4a96081546dbd676847b8e5c5ba84ddc37f92e33"),
    ("tests/fixtures/solc/access", 0, 431, "ca4858fa061a26a2b12ba2d645efe4c6a47f3277175162c221380bb51a532abb", "377d0b71aacf60edd22d6bad31b236eefd4f746907646e0203d052bd33262260"),
    ("tests/fixtures/solc/call_resolution", 0, 293, "f191c8d67672e9d4b2a2b8f4a3a564e4df32fc0168382a1e98821b7c18e94977", "15c3a15355b5dc2d3f97fcef860ba2442eab9c31acafe98659b5b21a92e00ad4"),
    ("tests/fixtures/solc/control_flow", 0, 88, "f7d4cc63341e350901d97df09f1c9994fb00d712c10b42a88c6dee1b5d8a8a23", "4634a56f31ed4c057a0cf248443b6326aa47ef0e87226e4eb25cb36b621fe68e"),
    ("tests/fixtures/solc/cross", 0, 53, "807f9738667b52787b28fd4fadf456a3719c3bb01e19b4b9c1f1125cf6a3cb53", "de1eac0fb22dbaca65a590493c4b7ac3e06c336c0cb0a243ee473127c83e7060"),
    ("tests/fixtures/solc/entry_overload", 0, 9, "af2e7f7ca8d654aa03a84c86902dc368a5ecfe1512fab2f8f162e83a3c753d10", "653ac4dff401f433a38a26ddf067daf3924f868b36a1549e0753a94fa6e01558"),
    ("tests/fixtures/solc/export/broken_src", 4, 0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    ("tests/fixtures/solc/export/kinds", 0, 101, "8200e1699b9ae79fe2e39d11fb8d8a0aab8b4874e5f89c60248ecf19e1958b76", "e5d397a7efbbffb2c6d8cba4286476a65bf88dc689f531fbbbe51facd0c111ba"),
    ("tests/fixtures/solc/facts", 0, 396, "bbcae12fb9fb620f04725eaa91a6dd0cf80734fe30f6022b31a1553c07337197", "b5c66e862a125ab4bbfd31f2db1d9cb914b15cc02dc3732e0729f1d9a9dcd868"),
    ("tests/fixtures/solc/model/broken_src", 4, 0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    ("tests/fixtures/solc/model/default_pick", 0, 37, "f9b2952fcff6065faaaedf1182fbf2714a47eed886201a30ef3ecafe59253f33", "72c696bb0a85c1b1cc043c43a590ae12f32f24ec369161b9bf819b24c59a8181"),
    ("tests/fixtures/solc/model/partial", 3, 6, "1bf3bf79ce027773629813f7595c57b7416e1d73be8e8ec0dfb560d4a1361ff0", "cec36848b99703ec24d737bc3fb3fa00fa5c99e2bc1233a205a8a4f969cf4bd4"),
    ("tests/fixtures/solc/model/project", 0, 104, "a0f99e8b32d58d12fb5cfe52f457bba44118b36b75e3c19770167944031b5055", "aed5d0f76e4d4ee4f54409ca32442b034f5330f83052d85f9e5bd5cdaf4f7fa6"),
    ("tests/fixtures/solc/state_flow", 0, 563, "208e782982e11ebe338df45dd055a970c0f6ebf80b1c0e1b7f5dfc1142793002", "8d6977759a3dc532a464ccc33912f678c4f39d7dd4b5005a6fed937bd35fe2c6"),
    ("tests/fixtures/solc/statements", 0, 103, "4ba83adf508204b39855d53ced1a6665df1095019a06ce52710790ba73bdea5c", "8dd0e17a6f0bb82784da827c79a8f71fcc7437e8f9e0a14bc87ad180fd1677c2"),
    ("tests/fixtures/staking", 0, 150, "88609695351a21d671ca11eb955edfbb9146d9d8ad8cd5cba388445ab5ade90e", "c33e0d83b21da75250a7dc73fb4b35ac17e230014c207662908b271d23dd3953"),
    ("tests/fixtures/uniswap_v2_pair", 0, 134, "9167b918bbd14a795ee09edd4cb8e89fb5f902106905944e6dae07a802d7f596", "413a461bc6751ebaede45e48323f6e2a1f0a60a5d1a8fc2af769772061221b6a"),
];

const V1_0_KINDS: &[&str] = &[
    "contract", "function", "state_var", "event", "error", "modifier", "calls", "reads", "writes", "emits", "reverts",
    "guards", "builtin_use", "unchecked_block", "uses_modifier", "opaque_assembly", "inherits", "exposes",
];

#[test]
fn existing_fixtures_keep_every_1_0_record() {
    let found: Vec<(&str, i32, usize, String, String)> = std::thread::scope(|scope| {
        let runs: Vec<_> = V1_0_EXPORTS
            .iter()
            .enumerate()
            .map(|(i, (path, code, ..))| {
                scope.spawn(move || {
                    let records: Vec<Value> = export(&repo().join(path), &format!("baseline-{i}"), *code)
                        .into_iter()
                        .skip(1)
                        .filter(|r| V1_0_KINDS.contains(&r["kind"].as_str().unwrap_or_default()))
                        .map(|r| as_1_0(&r))
                        .collect();
                    let mut ids: Vec<String> = records.iter().map(|r| r["id"].as_str().unwrap().to_string()).collect();
                    let mut lines: Vec<String> = records.iter().map(Value::to_string).collect();
                    ids.sort();
                    lines.sort();
                    (*path, *code, records.len(), digest(ids.join("\n").as_bytes()), digest(lines.join("\n").as_bytes()))
                })
            })
            .collect();
        runs.into_iter().map(|run| run.join().unwrap()).collect()
    });
    let changed: Vec<String> = found
        .iter()
        .zip(V1_0_EXPORTS)
        .filter(|((p, c, n, i, r), e)| (*p, *c, *n, i.as_str(), r.as_str()) != **e)
        .map(|(f, _)| format!("{f:?}"))
        .collect();
    assert!(changed.is_empty(), "exports differ from schema 1.0:\n{}", changed.join("\n"));
}

const DISPATCH: &str = "tests/fixtures/solc/dispatch/src";

fn dispatch_id(file: &str, member: &str) -> String {
    format!("evm:{DISPATCH}/{file}:{member}")
}

#[test]
fn external_calls_carry_their_candidates_and_add_up_to_contract_links() {
    let records = export(&fixture("dispatch"), "dispatch", 0);
    let run = dispatch_id("Router.sol", "Router.run(uint256)");
    let call = |text: &str| find(&records, "calls", |c| c["from"] == run && c["target_text"] == text);

    let vault = call("vault.deposit");
    assert_eq!(vault["dispatch_target"], Value::Null, "dispatch_target keeps its 1.0 meaning");
    assert_eq!(
        vault["dispatch_candidates"],
        serde_json::json!([{"function": dispatch_id("Impls.sol", "Vault.deposit(uint256)"), "contract": dispatch_id("Impls.sol", "Vault"), "basis": "inherits_interface", "tag": "INFERRED"}])
    );
    assert!(vault.get("unresolved_reason").is_none(), "{vault}");
    let assigned = &vault["target_source"]["assigned_at"];
    assert_eq!(assigned[0]["function"], dispatch_id("Router.sol", "Router.constructor(IVault,IHook,IOracle)"), "{assigned}");
    assert_eq!((assigned[0]["kind"].as_str(), assigned[0]["expr_text"].as_str()), (Some("constructor_param"), Some("v")));
    assert!(assigned[0]["span"]["start_line"].as_u64().is_some(), "{assigned}");
    let hook_sites: Vec<&str> = call("hook.onCall")["target_source"]["assigned_at"].as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap()).collect();
    assert_eq!(hook_sites, ["constructor_param", "setter"]);

    let hooks: Vec<(&str, &str)> = call("hook.onCall")["dispatch_candidates"].as_array().unwrap().iter().map(|c| (c["contract"].as_str().unwrap(), c["tag"].as_str().unwrap())).collect();
    let (a, b) = (dispatch_id("Impls.sol", "HookA"), dispatch_id("Impls.sol", "HookB"));
    assert_eq!(hooks, [(a.as_str(), "AMBIGUOUS"), (b.as_str(), "AMBIGUOUS")]);
    assert_eq!(call("registry.lookup")["dispatch_candidates"][0]["basis"], "selector_match");
    assert_eq!(call("this.selfValue")["dispatch_candidates"][0]["basis"], "self");
    for (text, reason) in [("oracle.price", "out_of_scope_interface"), ("orphan.ping", "no_implementation")] {
        assert_eq!(call(text)["dispatch_candidates"], serde_json::json!([]), "{text}");
        assert_eq!(call(text)["unresolved_reason"], reason, "{text}");
    }
    let pointer = find(&records, "calls", |c| c["from"] == dispatch_id("Router.sol", "Router.viaPointer(uint256)"));
    assert_eq!(pointer["unresolved_reason"], "dynamic_address");

    let links: std::collections::BTreeSet<(&str, &str, &str)> = of_kind(&records, "contract_link")
        .iter()
        .map(|l| (l["to_contract"].as_str().unwrap(), l["basis"].as_str().unwrap(), l["tag"].as_str().unwrap()))
        .collect();
    let (registry, router, vault_id) = (dispatch_id("Impls.sol", "Registry"), dispatch_id("Router.sol", "Router"), dispatch_id("Impls.sol", "Vault"));
    assert_eq!(
        links,
        std::collections::BTreeSet::from([
            (a.as_str(), "inherits_interface", "AMBIGUOUS"),
            (b.as_str(), "inherits_interface", "AMBIGUOUS"),
            (registry.as_str(), "selector_match", "INFERRED"),
            (router.as_str(), "self", "INFERRED"),
            (vault_id.as_str(), "inherits_interface", "INFERRED"),
        ])
    );
    let to_vault = find(&records, "contract_link", |l| l["to_contract"] == vault_id);
    assert_eq!(to_vault["from_contract"], router);
    assert_eq!(to_vault["via_state_vars"], serde_json::json!([dispatch_id("Router.sol", "Router.vault")]));
    assert_eq!(to_vault["call_ids"], serde_json::json!([vault["id"]]));
    assert_eq!(to_vault["span"], vault["span"]);
    assert!(to_vault["id"].as_str().unwrap().starts_with(&format!("contract_link:{router}#")), "{to_vault}");
}

const KEYS: &str = "tests/fixtures/solc/keys/src/Keys.sol";

fn keys_id(member: &str) -> String {
    format!("evm:{KEYS}:Keys.{member}")
}

#[test]
fn reads_and_writes_carry_the_keys_they_index_with() {
    let records = export(&fixture("keys"), "keys", 0);
    let access = |kind: &str, function: &str, path: &str| {
        find(&records, kind, |r| r["function"] == keys_id(function) && r["path"] == path && r["via"] == serde_json::json!([]))
    };
    let key = |text: &str, kind: &str, reference: Value| serde_json::json!({"text": text, "kind": kind, "ref": reference});

    let credit = access("writes", "credit(address,uint256)", "balances[]");
    let keys = serde_json::json!([key("account", "parameter", Value::from(keys_id("credit(address,uint256)")))]);
    assert_eq!(credit["keys"], keys);
    assert_eq!(credit["keys_reason"], Value::Null);
    assert_eq!(credit["key_sites"], serde_json::json!([{"keys": keys, "span": credit["span"]}]));
    assert_eq!(credit["path"], "balances[]", "path keeps its 1.0 value");

    let single = |kind: &str, function: &str, path: &str| access(kind, function, path)["keys"].clone();
    assert_eq!(single("writes", "deposit()", "balances[]"), serde_json::json!([key("msg.sender", "msg_sender", Value::Null)]));
    assert_eq!(single("writes", "fundTreasury(uint256)", "balances[]"), serde_json::json!([key("treasury", "state_var", Value::from(keys_id("treasury")))]));
    assert_eq!(single("writes", "selfCredit(uint256)", "balances[]"), serde_json::json!([key("address(this)", "this", Value::Null)]));
    assert_eq!(single("writes", "setFirst(uint256)", "slots[]"), serde_json::json!([key("0", "literal", Value::Null)]));
    assert_eq!(single("writes", "setNext(uint256)", "slots[]"), serde_json::json!([key("next", "local", Value::Null)]));
    assert_eq!(single("writes", "store(uint256,uint256)", "slots[]"), serde_json::json!([key("slotOf(seed)", "expression", Value::Null)]));
    let approve = Value::from(keys_id("approve(address,address,uint256)"));
    assert_eq!(
        single("writes", "approve(address,address,uint256)", "allowance[][]"),
        serde_json::json!([key("owner", "parameter", approve.clone()), key("spender", "parameter", approve)])
    );
    assert_eq!(single("writes", "grow(uint256,uint256)", "positions[].amount"), serde_json::json!([key("id", "parameter", Value::from(keys_id("_position(uint256)")))]));
    assert_eq!(single("writes", "creditLater(address,uint256)", "total"), serde_json::json!([]));

    let funded = find(&records, "reads", |r| r["function"] == keys_id("withdraw(address,uint256)") && r["via"] == serde_json::json!([keys_id("funded")]));
    assert_eq!(funded["keys"], serde_json::json!([key("who", "parameter", Value::from(keys_id("funded")))]), "a modifier's parameter refers to the modifier");

    let moved = access("reads", "move(address,address,uint256)", "balances[]");
    assert_eq!(moved["keys"], Value::Null);
    assert_eq!(moved["keys_reason"], "several");
    let texts: Vec<&str> = moved["key_sites"].as_array().unwrap().iter().map(|s| s["keys"][0]["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["from", "to"]);

    let later = find(&records, "writes", |r| r["function"] == keys_id("creditLater(address,uint256)") && r["path"] == "balances[]");
    assert_eq!(later["via"], serde_json::json!([keys_id("_credit(address,uint256)")]));
    assert_eq!((&later["keys"], &later["keys_reason"], &later["key_sites"]), (&Value::Null, &Value::from("transitive"), &serde_json::json!([])));

    for record in of_kind(&records, "reads").into_iter().chain(of_kind(&records, "writes")) {
        assert!(record.get("keys").is_some() && record.get("keys_reason").is_some() && record["key_sites"].is_array(), "{record}");
    }
}
