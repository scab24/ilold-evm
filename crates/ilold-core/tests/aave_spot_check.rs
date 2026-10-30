//! Manual spot check of call resolution on Aave v3 origin. Analysing writes Foundry
//! artifacts into the project, so point it at a scratch copy:
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
