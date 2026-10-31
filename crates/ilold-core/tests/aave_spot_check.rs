//! Manual spot checks on Aave v3 origin. Analysing no longer writes into the project, but
//! `aave_model_completeness` verifies exactly that, so point it at a scratch copy:
//! `ILOLD_AAVE_ROOT=/tmp/copy/aave cargo test -p ilold-core --test aave_spot_check -- --ignored --nocapture`
use std::path::PathBuf;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::callgraph::types::CallGraph;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::CfgStatement;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::walker::build_path_tree;

fn edges_from(cg: &CallGraph, pred: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = cg
        .edge_indices()
        .filter_map(|e| {
            let (s, t) = cg.edge_endpoints(e).unwrap();
            pred(&cg[s].id()).then(|| {
                let dispatched = cg[e].static_target.as_deref().map(|s| format!(" (static {s})")).unwrap_or_default();
                format!("{} -> {} [{:?} x{}]{dispatched}", cg[s].id(), cg[t].id(), cg[e].kind, cg[e].call_count)
            })
        })
        .collect();
    out.sort();
    out
}

fn graph(project: &Project, name: &str) -> CallGraph {
    build_call_graph(project, project.contracts.iter().find(|c| c.name == name).unwrap())
}

#[test]
#[ignore = "needs ILOLD_AAVE_ROOT pointing at a scratch copy of aave-v3-origin"]
fn aave_call_resolution() {
    let root = PathBuf::from(std::env::var("ILOLD_AAVE_ROOT").expect("ILOLD_AAVE_ROOT"));
    let project = SolcFrontend.parse_project(&root).expect("parse aave");

    println!("== AToken::_transfer* edges");
    let atoken = graph(&project, "AToken");
    for e in edges_from(&atoken, |id| id.starts_with("AToken::_transfer")) {
        println!("  {e}");
    }

    println!("== Pool edges to _msgSender");
    let pool = graph(&project, "Pool");
    let msg_sender: Vec<String> = edges_from(&pool, |_| true)
        .into_iter()
        .filter(|e| e.contains("_msgSender"))
        .collect();
    println!("  {} edges, e.g. {:?}", msg_sender.len(), msg_sender.first());
    let targets: std::collections::BTreeSet<String> = msg_sender
        .iter()
        .map(|e| e.split(" -> ").nth(1).unwrap().to_string())
        .collect();
    println!("  distinct targets: {targets:?}");

    println!("== Pool::supply edges");
    for e in edges_from(&pool, |id| id == "Pool::supply") {
        println!("  {e}");
    }

    println!("== SupplyLogic::executeSupply edges");
    let supply_logic = graph(&project, "SupplyLogic");
    for e in edges_from(&supply_logic, |id| id == "SupplyLogic::executeSupply") {
        println!("  {e}");
    }

    println!("== AToken::permit edges and path calls");
    for e in edges_from(&atoken, |id| id == "AToken::permit") {
        println!("  {e}");
    }
    let atoken_def = project.contracts.iter().find(|c| c.name == "AToken").unwrap();
    let (owner, permit) = project.lookup_function(atoken_def, "permit").unwrap();
    let cfg = CfgBuilder::build_with_project(permit, owner, Some(&project)).unwrap();
    let pt = build_path_tree(&cfg, &owner.name, &permit.key, &project.inherited_state_vars(owner),
        &PruningConfig::default());
    let mut internal: Vec<&String> = pt.paths.iter().flat_map(|p| &p.annotations.internal_calls).collect();
    internal.sort();
    internal.dedup();
    let mut external: Vec<String> = pt.paths.iter().flat_map(|p| &p.annotations.external_calls)
        .map(|c| format!("{}.{} {:?}", c.target, c.function, c.kind)).collect();
    external.sort();
    external.dedup();
    println!("  internal_calls {internal:?}");
    println!("  external_calls {external:?}");

    println!("== AToken transitive effects (overloaded _transfer)");
    let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
    for name in ["transfer", "transferFrom"] {
        let atoken_seq = &analysis.sequence_analyses["AToken"];
        match atoken_seq.functions.iter().find(|b| b.name == name) {
            Some(b) => println!(
                "  {name}: internal {:?}\n    transitive_internal {:?}\n    transitive_external {:?}",
                b.internal_call_targets, b.transitive_internal_calls, b.transitive_external_calls
            ),
            None => {
                let inherited: Vec<String> = analysis.sequence_analyses.iter()
                    .filter_map(|(c, sa)| sa.functions.iter().find(|b| b.name == name)
                        .filter(|_| c == "IncentivizedERC20" || c == "ScaledBalanceTokenBase")
                        .map(|b| format!("{c}: internal {:?} transitive_external {:?}",
                            b.internal_call_targets, b.transitive_external_calls)))
                    .collect();
                println!("  {name} (inherited): {inherited:?}");
            }
        }
    }

    println!("== PoolConfigurator.setPoolPause overloads");
    let configurator = project.contracts.iter().find(|c| c.name == "PoolConfigurator").unwrap();
    println!("  bare name: {:?}", project.lookup_function(configurator, "setPoolPause").map(|(_, f)| &f.key));
    for f in configurator.functions.iter().filter(|f| f.name == "setPoolPause") {
        let (owner, found) = project.lookup_function(configurator, &f.signature).unwrap();
        let cfg = CfgBuilder::build_with_project(found, owner, Some(&project)).unwrap();
        let calls: Vec<String> = cfg
            .node_weights()
            .flat_map(|b| &b.statements)
            .filter_map(|s| match s {
                CfgStatement::InternalCall { function, .. } => Some(format!("{function}()")),
                CfgStatement::ExternalCall { target, function, .. } => Some(format!("{target}.{function}()")),
                _ => None,
            })
            .collect();
        let file = &project.source_files[found.span.file_index].content;
        let header = file.lines().nth(found.span.start_line as usize - 1).unwrap_or_default().trim();
        println!("  key={} sig={} lines {}-{} source `{header}` cfg calls {calls:?}",
            found.key, found.signature, found.span.start_line, found.span.end_line);
    }
    let cg = graph(&project, "PoolConfigurator");
    for e in edges_from(&cg, |id| id.starts_with("PoolConfigurator::setPoolPause")) {
        println!("  {e}");
    }
}

/// Path, size and modification time of every file under `dir` (empty when it is absent).
fn snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut out = std::collections::BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                out.insert(entry.path(), (meta.len(), meta.modified().unwrap()));
            }
        }
    }
    out
}

#[test]
#[ignore = "needs ILOLD_AAVE_ROOT pointing at a scratch copy of aave-v3-origin"]
fn aave_model_completeness() {
    let root = PathBuf::from(std::env::var("ILOLD_AAVE_ROOT").expect("ILOLD_AAVE_ROOT"));
    let watched = [root.join("out"), root.join("cache")];
    let before: Vec<_> = watched.iter().map(|d| snapshot(d)).collect();

    let started = std::time::Instant::now();
    let project = SolcFrontend.parse_project(&root).expect("parse aave");
    let cold = started.elapsed();
    let started = std::time::Instant::now();
    let warm_project = SolcFrontend.parse_project(&root).expect("parse aave again");
    let warm = started.elapsed();
    println!("== parse: first {cold:.2?}, second {warm:.2?}");
    assert_eq!(warm_project.contracts.len(), project.contracts.len());

    let after: Vec<_> = watched.iter().map(|d| snapshot(d)).collect();
    for ((dir, b), a) in watched.iter().zip(&before).zip(&after) {
        println!("== {}: {} files before, {} after", dir.display(), b.len(), a.len());
        assert_eq!(b, a, "{} changed while parsing", dir.display());
    }

    let listed = project.project_contracts().count();
    let dependencies = project.contracts.len() - listed;
    println!("== contracts: {listed} project, {dependencies} dependency, {} total", project.contracts.len());
    println!("== warnings: {:?}", project.warnings);
    assert_eq!(listed, 177);

    let iatoken: Vec<&ilold_core::model::contract::ContractDef> =
        project.contracts.iter().filter(|c| c.name == "IAToken").collect();
    for c in &iatoken {
        println!("== IAToken {} key={} functions={}", c.qualified_id, c.key, c.functions.len());
    }
    assert_eq!(iatoken.len(), 2);
    assert!(iatoken.iter().all(|c| c.key == c.qualified_id));
    let core = iatoken.iter().find(|c| c.functions.len() == 11).expect("core IAToken");
    let atoken = project.lookup_contract("AToken").expect("AToken unique");
    assert!(atoken.base_ids.contains(&core.id.unwrap()), "AToken inherits {}", core.qualified_id);

    for name in ["Multicall", "Context"] {
        let found: Vec<_> = project.contracts.iter().filter(|c| c.name == name).collect();
        println!("== {name}: {:?}", found.iter().map(|c| (&c.qualified_id, c.is_dependency)).collect::<Vec<_>>());
        assert!(!found.is_empty() && found.iter().all(|c| c.is_dependency), "{name}");
    }

    let pool_instance = project.lookup_contract("PoolInstance").expect("PoolInstance");
    let inherited: Vec<String> = project.accessible_functions(pool_instance).into_iter()
        .filter(|af| af.is_inherited).map(|af| af.function.name.clone()).collect();
    println!("== PoolInstance inherited functions: {}", inherited.len());
    assert!(inherited.iter().any(|f| f == "multicall"), "{inherited:?}");
    assert!(!inherited.iter().any(|f| f == "ADDRESSES_PROVIDER"), "{inherited:?}");

    let pool = project.lookup_contract("Pool").expect("Pool");
    let storage: Vec<String> = project.inherited_state_vars(pool).into_iter().map(|v| v.name).collect();
    println!("== Pool storage order: {:?}", &storage[..storage.len().min(6)]);
    assert_eq!(storage.first().map(String::as_str), Some("lastInitializedRevision"));

    let started = std::time::Instant::now();
    let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
    println!("== analysis build {:.2?}: {} cfgs, {} path trees, {} call graphs",
        started.elapsed(), analysis.cfgs.len(), analysis.path_trees.len(), analysis.call_graphs.len());
    assert!(analysis.path_trees.keys().any(|(c, f)| c.contains("Multicall") && f == "multicall"));
    let pool_cg = graph(&project, "Pool");
    let msg_sender: Vec<String> = edges_from(&pool_cg, |_| true).into_iter().filter(|e| e.contains("_msgSender")).collect();
    println!("== Pool edges to _msgSender: {} e.g. {:?}", msg_sender.len(), msg_sender.first());
}

fn files_size(dir: &std::path::Path) -> u64 {
    snapshot(dir).iter().filter(|(p, _)| p.extension().is_some_and(|e| e == "json")).map(|(_, (len, _))| len).sum()
}

/// State and revert flow through Aave's logic libraries.
/// `ILOLD_AAVE_ROOT=/tmp/copy/aave cargo test -p ilold-core --test aave_spot_check aave_state_flow -- --ignored --nocapture`
#[test]
#[ignore = "needs ILOLD_AAVE_ROOT pointing at a scratch copy of aave-v3-origin"]
fn aave_state_flow() {
    use ilold_core::sequence::analysis::{EffectKind, FunctionBehavior};

    let root = PathBuf::from(std::env::var("ILOLD_AAVE_ROOT").expect("ILOLD_AAVE_ROOT"));
    let (project, key) = SolcFrontend.parse_project_keyed(&root).expect("parse aave");
    let params = AnalysisParams::new(1);
    let started = std::time::Instant::now();
    let analysis = ProjectAnalysis::build(&project, &params);
    println!("== analysis build {:.2?}: {} cfgs, {} path trees", started.elapsed(), analysis.cfgs.len(), analysis.path_trees.len());

    // persisted snapshot size, and the part of it this model adds
    // inside the isolated cache directory of the run
    let store = PathBuf::from(std::env::var("ILOLD_PROJECT_CACHE_DIR").expect("ILOLD_PROJECT_CACHE_DIR")).join("state-flow-measure");
    let _ = ilold_core::analysis_cache::load_or_build_with(Some(&store), key.as_ref(), &params, || {
        ProjectAnalysis::build(&project, &params)
    });
    println!("== snapshot json under the store: {} bytes (key present: {})", files_size(&store), key.is_some());
    let mut added = 0usize;
    let mut dependency_functions = 0usize;
    for ((c, _), cfg) in &analysis.cfgs {
        let contract = project.lookup_contract(c).unwrap();
        let inherited = project.project_contracts().any(|p| project.linearization(p).iter().any(|l| l.key() == contract.key()));
        if contract.is_dependency && !inherited {
            dependency_functions += 1;
            added += serde_json::to_vec(&cfg.node_weights().collect::<Vec<_>>()).unwrap().len();
        }
        for block in cfg.node_weights() {
            added += serde_json::to_vec(&block.reads).unwrap().len() * usize::from(!block.reads.is_empty());
            added += block.revert_reason.as_ref().map_or(0, |r| r.len() + 18);
            for stmt in &block.statements {
                added += match stmt {
                    CfgStatement::InternalCall { storage_args, .. } | CfgStatement::ExternalCall { storage_args, .. }
                        if !storage_args.is_empty() => serde_json::to_vec(storage_args).unwrap().len(),
                    CfgStatement::Assignment { storage: Some(s), .. } => serde_json::to_vec(s).unwrap().len(),
                    _ => 0,
                };
            }
        }
    }
    for ((c, _), pt) in &analysis.path_trees {
        let contract = project.lookup_contract(c).unwrap();
        let inherited = project.project_contracts().any(|p| project.linearization(p).iter().any(|l| l.key() == contract.key()));
        if contract.is_dependency && !inherited {
            added += serde_json::to_vec(pt).unwrap().len();
        }
    }
    for sa in analysis.sequence_analyses.values() {
        for b in &sa.functions {
            added += serde_json::to_vec(&(&b.transitive_effects, &b.revert_sources, &b.storage_param_writes, &b.storage_param_reads, &b.unresolved_modifiers)).unwrap().len();
        }
    }
    println!("== bytes added by the state-flow model (estimate): {added}; dependency functions analysed because called: {dependency_functions}");

    let behavior = |contract: &str, key: &str| -> &FunctionBehavior {
        analysis.sequence_analyses[contract].functions.iter().find(|b| b.name == key)
            .unwrap_or_else(|| panic!("{contract}::{key}"))
    };
    let traces = |b: &FunctionBehavior, kind: EffectKind| -> Vec<String> {
        b.transitive_effects.iter().filter(|e| e.kind == kind)
            .map(|e| format!("{} via {}", e.item, e.via.join(" -> "))).collect()
    };
    for name in ["supply", "withdraw", "borrow", "repay", "liquidationCall", "flashLoan"] {
        let b = behavior("Pool", name);
        let writes = &b.transitive_state_write_paths;
        println!("== Pool.{name}: direct writes {:?}, can_revert {}, read_only {}", b.state_writes, b.can_revert, b.read_only);
        println!("   transitive write paths ({}): {:?}", writes.len(), writes);
        for t in traces(b, EffectKind::StateWrite).iter().take(4) {
            println!("   write {t}");
        }
        let reverts = traces(b, EffectKind::Revert);
        println!("   revert sources: {} e.g. {:?}", reverts.len(), reverts.iter().take(3).collect::<Vec<_>>());
        assert!(writes.iter().any(|w| w.starts_with("_reserves[].")), "{name}: {writes:?}");
        assert!(writes.iter().any(|w| w.starts_with("_usersConfig[]")), "{name}: {writes:?}");
        assert!(b.can_revert && !reverts.is_empty(), "{name}");
        assert!(!b.read_only, "{name}");
    }
    let supply = behavior("Pool", "supply");
    let index = supply.transitive_effects.iter()
        .find(|e| e.kind == EffectKind::StateWrite && e.item == "_reserves[].liquidityIndex").expect("liquidityIndex");
    println!("== Pool.supply _reserves[].liquidityIndex via {:?}", index.via);
    assert_eq!(index.via.first().map(String::as_str), Some("SupplyLogic::executeSupply"));
    let collateral = supply.transitive_effects.iter()
        .find(|e| e.kind == EffectKind::StateWrite && e.item == "_usersConfig[].data").expect("_usersConfig data");
    println!("== Pool.supply _usersConfig[].data via {:?}", collateral.via);
    println!("== Pool.supply events {:?} external {:?}", supply.transitive_events, supply.transitive_external_calls);

    for (name, modifier) in [("initReserve", "modifier onlyPoolConfigurator"), ("rescueTokens", "modifier onlyPoolAdmin")] {
        let b = behavior("Pool", name);
        let reverts = traces(b, EffectKind::Revert);
        println!("== Pool.{name}: can_revert {} direct {:?}\n   {:?}", b.can_revert, b.revert_sources, reverts.iter().take(4).collect::<Vec<_>>());
        assert!(b.can_revert);
        assert!(b.transitive_effects.iter().any(|e| e.kind == EffectKind::Revert && e.via.first().map(String::as_str) == Some(modifier)), "{name}");
    }
    let init = behavior("Pool", "initReserve");
    println!("== Pool.initReserve writes {:?} transitive {:?}", init.state_write_paths, init.transitive_state_write_paths);

    let transfer = behavior("AToken", "transfer");
    println!("== AToken.transfer transitive external {:?}", transfer.transitive_external_calls);
    assert!(transfer.transitive_external_calls.iter().any(|c| c == "POOL.finalizeTransfer"), "{:?}", transfer.transitive_external_calls);

    let execute = behavior("SupplyLogic", "executeSupply");
    println!("== SupplyLogic.executeSupply direct param writes {:?}", execute.storage_param_writes);
    let params_written: Vec<String> = traces(execute, EffectKind::StorageParamWrite);
    println!("   through callees ({}): {:?}", params_written.len(), params_written.iter().take(5).collect::<Vec<_>>());
    assert!(execute.state_writes.is_empty());
    assert!(params_written.iter().any(|w| w.starts_with("reservesData[].liquidityIndex")));
    assert!(params_written.iter().any(|w| w.starts_with("userConfig.data")));
}

/// Access and state guards of Aave's entry points, and the "without access control"
/// observations they leave.
/// `ILOLD_AAVE_ROOT=/tmp/copy/aave cargo test -p ilold-core --test aave_spot_check aave_access_guards -- --ignored --nocapture`
#[test]
#[ignore = "needs ILOLD_AAVE_ROOT pointing at a scratch copy of aave-v3-origin"]
fn aave_access_guards() {
    use ilold_core::classify::entry_points::AccessLevel;
    use ilold_core::classify::guards::GuardKind;
    use ilold_core::model::function::{Mutability, Visibility};
    use ilold_core::narrative::function::build_function_narrative;
    use ilold_core::narrative::types::ObservationKind;

    let root = PathBuf::from(std::env::var("ILOLD_AAVE_ROOT").expect("ILOLD_AAVE_ROOT"));
    let project = SolcFrontend.parse_project(&root).expect("parse aave");
    let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));

    let access = |c: &str, f: &str| -> AccessLevel {
        analysis.classifications[c].iter().find(|(n, _)| n == f).map(|(_, a)| a.clone()).unwrap_or_else(|| panic!("{c}::{f}"))
    };
    let guards = |c: &str, key: &str| {
        analysis.sequence_analyses[c].functions.iter().find(|b| b.name == key).map(|b| b.guards.clone()).unwrap_or_default()
    };
    for name in ["Pool", "PoolConfigurator", "AToken", "ACLManager"] {
        let contract = project.contracts.iter().find(|c| c.name == name && !c.is_dependency).unwrap();
        let c = contract.key();
        println!("== {name}");
        let mut flagged = Vec::new();
        for af in project.accessible_functions(contract) {
            let f = af.function;
            if !matches!(f.visibility, Visibility::External | Visibility::Public)
                || matches!(f.mutability, Mutability::View | Mutability::Pure)
                || f.name.is_empty()
            {
                continue;
            }
            let found = guards(c, &f.key);
            let shown: Vec<String> = found
                .iter()
                .map(|g| format!("{:?}{} `{}` via [{}]", g.kind, if g.heuristic { " (heuristic)" } else { "" }, g.condition, g.via.join(" -> ")))
                .collect();
            let from = if af.is_inherited { format!(" (from {})", af.origin) } else { String::new() };
            println!("  {}{from}: {}\n    {}", f.key, access(c, &f.name), if shown.is_empty() { "-".into() } else { shown.join("\n    ") });
            let key = (af.owner.key().to_string(), f.key.clone());
            if let (Some(pt), Some(cfg)) = (analysis.path_trees.get(&key), analysis.cfgs.get(&key)) {
                let n = build_function_narrative(af.owner, f, pt, cfg, &analysis.sequence_analyses[c].functions);
                if n.observations.iter().any(|o| o.kind == ObservationKind::NoAccessControl) {
                    flagged.push(f.key.clone());
                }
            }
        }
        println!("  -> NoAccessControl on {}: {:?}", flagged.len(), flagged);
    }

    let has = |c: &str, f: &str, kind: GuardKind, via0: &str| {
        guards(c, f).iter().any(|g| g.kind == kind && !g.heuristic && g.via.first().map(String::as_str) == Some(via0))
    };
    assert!(has("Pool", "initReserve", GuardKind::RoleCheck, "modifier onlyPoolConfigurator") || has("Pool", "initReserve", GuardKind::CallerEquals, "modifier onlyPoolConfigurator"));
    assert!(has("Pool", "rescueTokens", GuardKind::RoleCheck, "modifier onlyPoolAdmin"));
    for f in ["supply", "withdraw", "borrow", "repay", "flashLoan"] {
        assert!(!guards("Pool", f).iter().any(|g| g.kind.is_access()), "Pool.{f}: {:?}", guards("Pool", f));
        assert_eq!(access("Pool", f), AccessLevel::Public, "Pool.{f}");
    }
}
