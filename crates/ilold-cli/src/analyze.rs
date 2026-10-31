use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::callgraph::builder::build_call_graph;
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::classify::entry_points::classify_in;
use ilold_core::depgraph::ContractDeps;
use ilold_core::model::contract::{ContractDef, ContractKind};
use ilold_core::model::function::Visibility;
use ilold_core::model::project::{ContractLookupError, Project};
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::parse::ProjectParser;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::PathTree;
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::analysis::{analyze_sequences, SequenceAnalysis};
use ilold_core::sequence::builder::build_sequence_tree;

use crate::colors::*;

pub(crate) fn select_contracts<'a>(
    project: &'a Project,
    filter: Option<&str>,
    include_dependencies: bool,
) -> Result<Vec<&'a ContractDef>> {
    if let Some(name) = filter {
        return Ok(vec![resolve_contract(project, name)?]);
    }
    let mut selected: Vec<&ContractDef> =
        project.contracts.iter().filter(|c| include_dependencies || c.is_listed()).collect();
    selected.sort_by(|a, b| {
        project
            .contract_folder(a)
            .cmp(&project.contract_folder(b))
            .then(a.key().cmp(b.key()))
    });
    Ok(selected)
}

/// Unknown names suggest close contract keys; an ambiguous name lists the qualified ids.
fn resolve_contract<'a>(project: &'a Project, name: &str) -> Result<&'a ContractDef> {
    match project.lookup_contract(name) {
        Ok(contract) => Ok(contract),
        Err(ContractLookupError::NotFound) => {
            Err(contract_not_found(name, project.contracts.iter().map(|c| c.key())))
        }
        Err(ContractLookupError::Ambiguous(_)) => {
            Err(anyhow::anyhow!(project.find_contract(Some(name)).unwrap_err()))
        }
    }
}

pub fn run(
    path: &PathBuf,
    contract_filter: Option<&str>,
    max_seq_depth: usize,
    verbose: bool,
    include_dependencies: bool,
) -> Result<()> {
    let paths = vec![crate::foundry_root(path)?];

    let parser = SolcFrontend;
    let (mut project, key) = parser
        .parse_keyed(&paths)
        .context(format!("Failed to parse {}", path.display()))?;
    project.rebuild_index();

    for warning in &project.warnings {
        eprintln!("{} {warning}", c_warn("warning:"));
    }
    let hidden = project.hidden_contracts();
    println!("Parsed {} file(s), {} contract(s) ({} dependency or free-function contract(s){})\n",
        project.source_files.len(), project.contracts.len() - hidden, hidden,
        if include_dependencies { "" } else { " hidden; --include-dependencies lists them" });

    let ordered = select_contracts(&project, contract_filter, include_dependencies)?;

    let params = AnalysisParams::new(max_seq_depth);
    let all_analyses = ProjectAnalysis::load_or_build(&project, key.as_ref(), &params).sequence_analyses;

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

pub fn run_deps(path: &Path, contract: Option<&str>, include_dependencies: bool) -> Result<()> {
    let paths = vec![crate::foundry_root(path)?];
    let parser = SolcFrontend;
    let mut project = parser
        .parse(&paths)
        .context(format!("Failed to parse {}", path.display()))?;
    project.rebuild_index();

    let all = ContractDeps::build(&project);

    if let Some(name) = contract {
        let name = resolve_contract(&project, name)?.key();
        let deps = &all;
        crate::deps_view::print_direction(name, false, crate::deps_view::dep_rows(&deps, name, false));
        crate::deps_view::print_direction(name, true, crate::deps_view::dep_rows(&deps, name, true));
        return Ok(());
    }

    let deps = if include_dependencies { all } else { all.without_dependencies() };
    let layers = deps.layers();
    let name_w = deps.graph.node_weights().map(|n| n.name.len()).max().unwrap_or(0);

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

pub fn contract_not_found<'a>(name: &str, available: impl Iterator<Item = &'a str>) -> anyhow::Error {
    let mut names: Vec<&str> = available.filter(|n| !n.is_empty()).collect();
    names.sort_unstable();
    names.dedup();
    let close = close_matches(name, &names);
    if close.is_empty() {
        anyhow::anyhow!("Contract '{name}' not found. Available: {}", names.join(", "))
    } else {
        anyhow::anyhow!("Contract '{name}' not found. Did you mean: {}?", close.join(", "))
    }
}

/// Equal ignoring case, containing each other, or within a small edit distance.
pub fn close_matches<'a>(name: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let needle = name.to_lowercase();
    let limit = (needle.chars().count() / 3).max(1);
    candidates
        .iter()
        .copied()
        .filter(|c| {
            let cand = c.to_lowercase();
            cand == needle
                || (needle.len() >= 3 && (cand.contains(&needle) || needle.contains(&cand)))
                || edit_distance(&cand, &needle) <= limit
        })
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
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

    let dependency = if contract.is_dependency { " (dependency)" } else { "" };
    println!("{} {} ({} functions, {} state vars){}",
        kind_str.dimmed(), contract.key().bold(), contract.functions.len(), contract.state_vars.len(),
        dependency.dimmed());

    if !contract.inherits.is_empty() {
        println!("  {} {}", "inherits:".dimmed(), contract.inherits.join(", "));
    }

    let config = PruningConfig::default();
    let mut path_trees: Vec<PathTree> = Vec::new();

    for func in &contract.functions {
        let display_name = if func.name.is_empty() {
            format!("{:?}", func.kind).to_lowercase()
        } else {
            func.name.clone()
        };

        let access = classify_in(project, contract, contract, func).0;
        let vis = match func.visibility {
            Visibility::Public => "public",
            Visibility::External => "external",
            Visibility::Internal => "internal",
            Visibility::Private => "private",
        };

        let cfg = CfgBuilder::build_with_project(func, contract, Some(project));
        let pt = build_path_tree(
            &cfg, contract.key(), &func.key, &config,
        );

        println!("  {} {} {} — {} blocks, {} edges, {} paths ({} happy, {} revert)",
            access_colored(&access), c_muted(vis), c_bright(&display_name),
            cfg.node_count(), cfg.edge_count(),
            pt.stats.total_paths,
            c_ok(&pt.stats.happy_paths.to_string()),
            c_danger(&pt.stats.revert_paths.to_string()));
        if let Some(note) = pt.stats.truncation_note() {
            println!("    {}", c_warn(&note));
        }

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

    if verbose {
        let cg = build_call_graph(project, contract);
        let edges: Vec<_> = cg.edge_indices().collect();
        if !edges.is_empty() {
            println!("  {}", c_muted("Call graph:"));
            for edge_idx in edges {
                let (src, dst) = cg.edge_endpoints(edge_idx).unwrap();
                let edge = &cg[edge_idx];
                let kind = edge.kind.label();
                let kind_color = if edge.kind.is_internal() { c_muted(kind) } else { c_danger(kind) };
                println!("    {} {} {} {}",
                    c_bright(&cg[src].id()), c_muted("→"),
                    c_accent(&cg[dst].id()),
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
                st.sequence_count(max_seq_depth),
                st.functions.len(), state_changing, read_only);

            if verbose {
                for d in 1..=max_seq_depth {
                    let count = st.sequence_count(d) - st.sequence_count(d - 1);
                    println!("    {} {} sequences", c_muted(&format!("depth {}:", d)), count);
                }
                println!();
            }
        }

        let default_analysis;
        let analysis = match all_analyses.get(contract.key()) {
            Some(a) => a,
            None => {
                default_analysis = analyze_sequences(&HashMap::new(), contract.key());
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
                    .filter(|t| t.from == func.name && (t.from_owner.is_empty() || t.from_owner == func.owner)
                        && (!t.shared_state.is_empty() || !t.conditions_affected.is_empty()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model_fixture() -> Project {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/model/project");
        SolcFrontend.parse_project(&root).expect("parse model fixture")
    }

    #[test]
    fn listing_hides_dependencies_unless_asked_and_ambiguous_names_fail() {
        let project = model_fixture();
        let keys = |include: bool| -> Vec<String> {
            select_contracts(&project, None, include).unwrap().iter().map(|c| c.key().to_string()).collect()
        };
        let default = keys(false);
        assert!(default.contains(&"Vault".to_string()) && !default.contains(&"Multicall".to_string()), "{default:?}");
        let all = keys(true);
        assert_eq!(all.len(), default.len() + 4, "{all:?}");

        let err = select_contracts(&project, Some("IToken"), false).unwrap_err().to_string();
        assert!(err.contains("ambiguous") && err.contains("src/ext/IToken.sol:IToken"), "{err}");
        let one = select_contracts(&project, Some("src/ext/IToken.sol:IToken"), false).unwrap();
        assert_eq!(one[0].functions.len(), 1);
        assert!(select_contracts(&project, Some("Nope"), false).is_err());
    }

    #[test]
    fn unknown_names_suggest_contract_keys() {
        let project = model_fixture();
        let err = select_contracts(&project, Some("vault"), false).unwrap_err().to_string();
        assert!(err.contains("Contract 'vault' not found") && err.contains("Did you mean:") && err.contains(" Vault,"), "{err}");
        let err = select_contracts(&project, Some("Nope"), false).unwrap_err().to_string();
        assert!(err.contains("Available:") && err.contains("Vault"), "{err}");
    }

    #[test]
    fn close_matches_cover_case_typos_and_substrings() {
        let names = ["Vault", "LendingPool", "IPool", "BasePool"];
        assert_eq!(close_matches("vault", &names), vec!["Vault"]);
        assert_eq!(close_matches("LendingPol", &names), vec!["LendingPool"]);
        assert_eq!(close_matches("Pool", &names), vec!["LendingPool", "IPool", "BasePool"]);
        assert!(close_matches("Nope", &names).is_empty());
    }
}
