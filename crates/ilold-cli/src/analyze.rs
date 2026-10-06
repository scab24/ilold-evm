use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::callgraph::types::CallKind;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::classify::entry_points::classify_function;
use ilold_core::depgraph::ContractDeps;
use ilold_core::model::contract::{ContractDef, ContractKind};
use ilold_core::model::function::Visibility;
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::PathTree;
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::analysis::{analyze_sequences, SequenceAnalysis};
use ilold_core::sequence::builder::build_sequence_tree;

use crate::colors::*;

pub fn run(
    path: &PathBuf,
    contract_filter: Option<&str>,
    max_seq_depth: usize,
    verbose: bool,
) -> Result<()> {
    let paths = vec![crate::foundry_root(path)?];

    let parser = SolcFrontend;
    let (mut project, key) = parser
        .parse_keyed(&paths)
        .context(format!("Failed to parse {}", path.display()))?;
    project.rebuild_index();

    println!("Parsed {} file(s), {} contract(s)\n",
        project.source_files.len(), project.contracts.len());

    let params = AnalysisParams::new(max_seq_depth);
    let all_analyses = ProjectAnalysis::load_or_build(&project, key.as_ref(), &params).sequence_analyses;

    let mut ordered: Vec<&ContractDef> = project
        .contracts
        .iter()
        .filter(|c| contract_filter.map_or(true, |f| c.name == f))
        .collect();
    ordered.sort_by(|a, b| {
        project
            .contract_folder(a)
            .cmp(&project.contract_folder(b))
            .then(a.name.cmp(&b.name))
    });

    let mut current_folder: Option<String> = None;
    for contract in ordered {
        let folder = project.contract_folder(contract);
        if current_folder.as_deref() != Some(&folder) {
            let label = if folder.is_empty() { "(root)" } else { &folder };
            println!("\n{}", format!("── {label} ──").bold());
            current_folder = Some(folder);
        }
        print_contract(&project, contract, max_seq_depth, verbose, &all_analyses);
    }

    Ok(())
}

pub fn run_deps(path: &PathBuf, contract: Option<&str>) -> Result<()> {
    let paths = vec![crate::foundry_root(path)?];
    let parser = SolcFrontend;
    let mut project = parser
        .parse(&paths)
        .context(format!("Failed to parse {}", path.display()))?;
    project.rebuild_index();

    let deps = ContractDeps::build(&project);

    if let Some(name) = contract {
        if deps.node(name).is_none() {
            anyhow::bail!("Contract '{name}' not found");
        }
        crate::deps_view::print_direction(name, false, crate::deps_view::dep_rows(&deps, name, false));
        crate::deps_view::print_direction(name, true, crate::deps_view::dep_rows(&deps, name, true));
        return Ok(());
    }

    let layers = deps.layers();
    let name_w = project.contracts.iter().map(|c| c.name.len()).max().unwrap_or(0);

    println!(
        "Dependency map · reading order ({} = read first)\n",
        c_muted("layer 0")
    );
    if layers.is_empty() {
        println!("  {}", c_muted("no contracts"));
        return Ok(());
    }

    for layer in &layers {
        println!("{}", format!("── layer {} ──", layer.index).bold());

        let mut groups: Vec<_> = layer.groups.iter().collect();
        groups.sort_by_key(|g| {
            g.members
                .iter()
                .map(|&m| deps.graph[m].name.clone())
                .min()
                .unwrap_or_default()
        });

        for g in groups {
            if g.is_cycle() {
                let mut names: Vec<&str> =
                    g.members.iter().map(|&m| deps.graph[m].name.as_str()).collect();
                names.sort();
                println!(
                    "  {} {}  {}",
                    c_warn("⟲ cycle"),
                    names.join(" ⇄ "),
                    c_muted("(mutually dependent — read together)")
                );
            } else {
                let m = g.members[0];
                print_dep_line(&deps, &deps.graph[m].name, deps.graph[m].kind, name_w);
            }
        }
    }
    println!(
        "\n{} {}  {}  {}",
        c_muted("legend:"),
        c_accent("inherits→"),
        c_warn("calls→"),
        c_muted("holds→")
    );
    Ok(())
}

fn print_dep_line(deps: &ContractDeps, name: &str, kind: ContractKind, name_w: usize) {
    let outs = deps.dependencies(name);
    let mut inherits = Vec::new();
    let mut calls = Vec::new();
    let mut holds = Vec::new();
    for d in &outs {
        if d.edge.inherits {
            inherits.push(d.node.name.as_str());
        }
        if d.edge.calls {
            calls.push(d.node.name.as_str());
        }
        if d.edge.holds {
            holds.push(d.node.name.as_str());
        }
    }

    let mut parts = Vec::new();
    if !inherits.is_empty() {
        parts.push(format!("{} {}", c_accent("inherits→"), inherits.join(", ")));
    }
    if !calls.is_empty() {
        parts.push(format!("{} {}", c_warn("calls→"), calls.join(", ")));
    }
    if !holds.is_empty() {
        parts.push(format!("{} {}", c_muted("holds→"), holds.join(", ")));
    }

    let kind_str = format!("{:<9}", kind_word(kind));
    if parts.is_empty() {
        println!("  {} {}", c_muted(&kind_str), c_bright(name));
    } else {
        let name_str = format!("{name:<name_w$}");
        println!(
            "  {} {}   {}",
            c_muted(&kind_str),
            c_bright(&name_str),
            parts.join("  ·  ")
        );
    }
}

fn kind_word(kind: ContractKind) -> &'static str {
    match kind {
        ContractKind::Contract => "contract",
        ContractKind::Interface => "interface",
        ContractKind::Library => "library",
        ContractKind::Abstract => "abstract",
    }
}


fn print_contract(
    project: &Project,
    contract: &ContractDef,
    max_seq_depth: usize,
    verbose: bool,
    all_analyses: &HashMap<String, SequenceAnalysis>,
) {
    let kind_str = match contract.kind {
        ContractKind::Contract => "contract",
        ContractKind::Interface => "interface",
        ContractKind::Library => "library",
        ContractKind::Abstract => "abstract",
    };

    println!("{} {} ({} functions, {} state vars)",
        kind_str.dimmed(), contract.name.bold(), contract.functions.len(), contract.state_vars.len());

    if !contract.inherits.is_empty() {
        println!("  {} {}", "inherits:".dimmed(), contract.inherits.join(", "));
    }

    let config = PruningConfig::default();
    let mut path_trees: Vec<PathTree> = Vec::new();
    let combined_state_vars = project.inherited_state_vars(contract);

    for func in &contract.functions {
        let display_name = if func.name.is_empty() {
            format!("{:?}", func.kind).to_lowercase()
        } else {
            func.name.clone()
        };

        let access = classify_function(func, contract);
        let vis = match func.visibility {
            Visibility::Public => "public",
            Visibility::External => "external",
            Visibility::Internal => "internal",
            Visibility::Private => "private",
        };

        match CfgBuilder::build(func, contract) {
            Ok(cfg) => {
                let pt = build_path_tree(
                    &cfg, &contract.name, &func.name, &combined_state_vars, &config,
                );

                println!("  {} {} {} — {} blocks, {} edges, {} paths ({} happy, {} revert)",
                    access_colored(&access), c_muted(vis), c_bright(&display_name),
                    cfg.node_count(), cfg.edge_count(),
                    pt.stats.total_paths,
                    c_ok(&pt.stats.happy_paths.to_string()),
                    c_danger(&pt.stats.revert_paths.to_string()));

                if verbose {
                    for node in cfg.node_indices() {
                        let block = &cfg[node];
                        println!("    {} {:?} {}",
                            c_muted(&format!("[{}]", block.id)),
                            block.kind,
                            c_muted(&format!("({} stmts)", block.statements.len())));
                    }
                    for edge in cfg.edge_indices() {
                        let (src, dst) = cfg.edge_endpoints(edge).unwrap();
                        println!("    {} {} {} {}",
                            cfg[src].id, c_muted("→"), cfg[dst].id, c_muted(&format!("{:?}", cfg[edge])));
                    }
                    println!();
                }

                path_trees.push(pt);
            }
            Err(e) => {
                println!("  {} {} {} — {}",
                    access_colored(&access), c_muted(vis), display_name, c_danger(&format!("CFG error: {e}")));
            }
        }
    }

    if verbose {
        let cg = build_call_graph(project, contract);
        let edges: Vec<_> = cg.edge_indices().collect();
        if !edges.is_empty() {
            println!("  {}", c_muted("Call graph:"));
            for edge_idx in edges {
                let (src, dst) = cg.edge_endpoints(edge_idx).unwrap();
                let edge = &cg[edge_idx];
                let kind_color = match edge.kind {
                    CallKind::Internal => c_muted("internal"),
                    CallKind::External => c_danger("external"),
                    CallKind::Inherited => c_muted("inherited"),
                };
                println!("    {} {} {} {}",
                    c_bright(&cg[src].function), c_muted("→"),
                    c_accent(&format!("{}.{}", cg[dst].contract, cg[dst].function)),
                    kind_color);
            }
            println!();
        }
    }

    if contract.kind != ContractKind::Interface {
        let st = build_sequence_tree(contract, &path_trees, max_seq_depth);
        if !st.functions.is_empty() {
            let state_changing = st.functions.iter().filter(|f| !f.read_only).count();
            let read_only = st.functions.iter().filter(|f| f.read_only).count();

            println!("  {} {} total ({} functions: {} state-changing, {} read-only)",
                c_muted(&format!("Sequences (depth {}):", max_seq_depth)),
                st.sequences.len(),
                st.functions.len(), state_changing, read_only);

            if verbose {
                for d in 1..=max_seq_depth {
                    let count = st.sequences.iter().filter(|s| s.depth == d).count();
                    println!("    {} {} sequences", c_muted(&format!("depth {}:", d)), count);
                }
                println!();
            }
        }

        let default_analysis;
        let analysis = match all_analyses.get(&contract.name) {
            Some(a) => a,
            None => {
                default_analysis = analyze_sequences(&HashMap::new(), &contract.name);
                &default_analysis
            }
        };

        if verbose {
            println!("  {}", c_muted("Function behaviors:"));
            let func_count = analysis.functions.len();
            for (i, func) in analysis.functions.iter().enumerate() {
                let is_last_func = i == func_count - 1;
                let branch = if is_last_func { "└── " } else { "├── " };
                let pipe = if is_last_func { "    " } else { "│   " };

                let view_tag = if func.read_only { c_muted(" (view)").to_string() } else { String::new() };
                println!("  {}{}{}", c_muted(branch), c_bright(&func.name), view_tag);

                if !func.preconditions.is_empty() {
                    println!("  {}├── {} {}", c_muted(pipe), c_muted("requires:"), c_warn(&func.preconditions.join(", ")));
                }
                if !func.state_writes.is_empty() {
                    println!("  {}├── {} {}", c_muted(pipe), c_muted("writes:"), c_accent(&func.state_writes.join(", ")));
                }
                if !func.external_calls.is_empty() {
                    println!("  {}├── {} {}", c_muted(pipe), c_muted("calls:"), c_danger(&func.external_calls.join(", ")));
                }
                if !func.events.is_empty() {
                    println!("  {}├── {} {}", c_muted(pipe), c_muted("emits:"), func.events.join(", "));
                }

                let outgoing: Vec<_> = analysis.transitions.iter()
                    .filter(|t| t.from == func.name && (!t.shared_state.is_empty() || !t.conditions_affected.is_empty()))
                    .collect();

                if !outgoing.is_empty() {
                    println!("  {}└── {}", c_muted(pipe), c_muted("transitions:"));
                    let out_count = outgoing.len();
                    for (j, t) in outgoing.iter().enumerate() {
                        let is_last_t = j == out_count - 1;
                        let t_branch = if is_last_t { "└── " } else { "├── " };
                        let t_pipe = if is_last_t { "    " } else { "│   " };

                        println!("  {}    {}→ {}", c_muted(pipe), c_muted(t_branch), c_bright(&t.to));

                        if !t.shared_state.is_empty() {
                            println!("  {}    {}{} {}", c_muted(pipe), c_muted(t_pipe), c_muted("shared:"), c_accent(&t.shared_state.join(", ")));
                        }
                        for cond in &t.conditions_affected {
                            println!("  {}    {}{}", c_muted(pipe), c_muted(t_pipe), c_warn(cond));
                        }
                    }
                }
            }
            println!();
        }
    }

    println!();
}
