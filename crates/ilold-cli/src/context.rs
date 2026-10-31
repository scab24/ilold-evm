use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::cfg::builder::CfgBuilder;
use ilold_core::cfg::types::CfgGraph;
use ilold_core::classify::entry_points::{classify_contract, AccessLevel};
use ilold_core::model::contract::ContractDef;
use ilold_core::model::project::Project;
use ilold_core::narrative::function::build_function_narrative;
use ilold_core::narrative::sequence::build_sequence_narrative;
use ilold_core::narrative::types::*;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::pathtree::config::PruningConfig;
use ilold_core::pathtree::types::{PathTree, TerminalKind};
use ilold_core::pathtree::walker::build_path_tree;
use ilold_core::sequence::analysis::analyze_sequences;

use crate::colors::*;

const SHARED_SEQ_DEPTH: usize = 3;

pub fn run(
    path: &PathBuf,
    contract_filter: Option<&str>,
    function_filter: Option<&str>,
    sequence_filter: Option<&str>,
    list: bool,
) -> Result<()> {
    let paths = vec![crate::foundry_root(path)?];

    let parser = SolcFrontend;
    let (mut project, key) = parser.parse_keyed(&paths).context("Failed to parse")?;
    project.rebuild_index();

    let contract = find_contract(&project, contract_filter)?;
    let classifications = classify_contract(&project, contract);

    if list {
        print_list(contract, &classifications);
        return Ok(());
    }

    let config = PruningConfig::default();
    let mut cfgs: HashMap<(String, String), CfgGraph> = HashMap::new();
    let mut pt_map: HashMap<(String, String), PathTree> = HashMap::new();
    let combined_state_vars = project.inherited_state_vars(contract);
    for func in &contract.functions {
        let cfg = CfgBuilder::build_with_project(func, contract, Some(&project));
        let pt = build_path_tree(&cfg, contract.key(), &func.key, &combined_state_vars, &config);
        let key = (contract.key().to_string(), func.key.clone());
        cfgs.insert(key.clone(), cfg);
        pt_map.insert(key, pt);
    }
    // Sequence depth does not affect what context prints; the CLI default shares the cached analysis.
    let params = AnalysisParams::new(SHARED_SEQ_DEPTH);
    let all_sequence_analyses =
        ProjectAnalysis::load_or_build(&project, key.as_ref(), &params).sequence_analyses;
    let analysis = all_sequence_analyses
        .get(contract.key())
        .cloned()
        .unwrap_or_else(|| analyze_sequences(&pt_map, contract.key()));

    if let Some(seq_str) = sequence_filter {
        let names: Vec<&str> = seq_str.split(',').map(|s| s.trim()).collect();
        let narrative = build_sequence_narrative(&project, contract, &names, &analysis.functions);
        print_sequence(&narrative);
        return Ok(());
    }

    if let Some(func_name) = function_filter {
        let func = contract.functions.iter()
            .find(|f| f.name == func_name || f.key == func_name || f.match_signature() == func_name)
            .ok_or_else(|| anyhow::anyhow!("Function '{}' not found. Use --list to see available functions.", func_name))?;
        let key = (contract.key().to_string(), func.key.clone());
        let cfg = cfgs.get(&key).ok_or_else(|| anyhow::anyhow!("No CFG for {}", func_name))?;
        let pt = pt_map.get(&key).ok_or_else(|| anyhow::anyhow!("No paths for {}", func_name))?;
        let narrative = build_function_narrative(contract, func, pt, cfg, &analysis.functions);
        print_function(&narrative);
        return Ok(());
    }

    let narratives: Vec<_> = contract.functions.iter().filter_map(|func| {
        let key = (contract.key().to_string(), func.key.clone());
        let cfg = cfgs.get(&key)?;
        let pt = pt_map.get(&key)?;
        Some(build_function_narrative(contract, func, pt, cfg, &analysis.functions))
    }).collect();
    print_overview(contract, &narratives);
    Ok(())
}

fn find_contract<'a>(project: &'a Project, filter: Option<&str>) -> Result<&'a ContractDef> {
    project.find_contract(filter).map_err(|e| anyhow::anyhow!(e))
}

fn print_list(contract: &ContractDef, classifications: &[(String, AccessLevel)]) {
    println!("\n  {} — {} functions\n",
        c_bright(&contract.name), contract.functions.len());

    for (name, access) in classifications {
        if name.is_empty() { continue; }
        let vis = contract.functions.iter().find(|f| f.name == *name)
            .map(|f| format!("{:?}", f.visibility).to_lowercase()).unwrap_or_default();
        println!("  {} {:<20} {}", access_colored(access), c_bright(name), c_muted(&vis));
    }

    println!("\n  {}", c_muted("Usage:"));
    println!("    ilold context <path> --function <name>");
    println!("    ilold context <path> --sequence \"fn1,fn2\"");

    let names: Vec<&str> = classifications.iter()
        .filter(|(n, _)| !n.is_empty()).map(|(n, _)| n.as_str()).take(2).collect();
    if names.len() >= 2 {
        println!("\n  {}", c_muted("Example:"));
        println!("    ilold context <path> --function {}", names[0]);
        println!("    ilold context <path> --sequence \"{},{}\"", names[0], names[1]);
    }
    println!();
}

fn print_function(n: &FunctionNarrative) {
    println!("\n  {} {}\n", c_bright(&n.name), c_muted(&format!("({})", n.access)));

    if !n.modifiers.is_empty() {
        println!("  {} {}", c_muted("Modifiers:"), n.modifiers.join(", "));
    }
    println!("  {} {} ({} return, {} revert)",
        c_muted("Paths:"), n.total_paths, c_ok(&n.happy_paths.to_string()), c_danger(&n.revert_paths.to_string()));
    if !n.state_writes.is_empty() {
        println!("  {} {}", c_muted("Writes:"), c_warn(&n.state_writes.join(", ")));
    }
    if !n.external_calls.is_empty() {
        println!("  {} {}", c_muted("Calls:"), c_danger(&n.external_calls.join(", ")));
    }

    for path in &n.paths {
        println!();
        let terminal = match path.terminal {
            TerminalKind::Return => c_ok("Return"),
            TerminalKind::Revert => c_danger("Revert"),
            TerminalKind::DepthCutoff => c_muted("DepthCutoff"),
            TerminalKind::LoopCutoff => c_muted("LoopCutoff"),
        };
        println!("  {} #{} → {}", c_muted("Path"), path.id, terminal);

        for (i, step) in path.steps.iter().enumerate() {
            let is_last = i == path.steps.len() - 1;
            let connector = if is_last { "  └── " } else { "  ├── " };

            let branch_str = match step.branch {
                Some(BranchDirection::True) => format!(" {}", c_ok("✓")),
                Some(BranchDirection::False) => format!(" {}", c_danger("✗")),
                None => String::new(),
            };

            let desc = match step.step_type {
                StepType::Entry => c_bright(&step.description).to_string(),
                StepType::Condition => c_warn(&step.description).to_string(),
                StepType::ExternalCall => c_danger(&step.description).to_string(),
                StepType::InternalCall => c_muted(&step.description).to_string(),
                StepType::StateWrite => c_accent(&step.description).to_string(),
                StepType::StateRead => c_muted(&step.description).to_string(),
                StepType::EthTransfer => c_danger(&step.description).to_string(),
                StepType::Event => c_muted(&step.description).to_string(),
                StepType::Return => c_ok(&step.description).to_string(),
                StepType::Revert => c_danger(&step.description).to_string(),
                StepType::Assembly => c_muted(&step.description).to_string(),
            };

            println!("{}{} {}{}",
                c_muted(connector), step.step_type.icon(), desc, branch_str);
        }
    }

    if !n.observations.is_empty() {
        println!("\n  {}", c_muted("Observations:"));
        for obs in &n.observations {
            println!("  {} {}: {}",
                c_warn("⚠"), c_muted(&obs.kind.to_string()), obs.description);
        }
    }
    println!();
}

fn print_sequence(n: &SequenceNarrative) {
    let names: Vec<&str> = n.steps.iter().map(|s| s.function.as_str()).collect();
    println!("\n  {} {}\n",
        c_muted("Sequence:"), c_bright(&names.join(" → ")));

    for (i, step) in n.steps.iter().enumerate() {
        let is_last_step = i == n.steps.len() - 1;
        let step_conn = if is_last_step { "└─" } else { "├─" };
        let pipe = if is_last_step { "  " } else { "│ " };

        println!("  {} {} {}",
            c_muted(step_conn),
            c_bright(&format!("Step {}: {}", i + 1, step.function)),
            c_muted(&format!("({})", step.access)));

        let mut lines: Vec<(String, String)> = Vec::new();

        for req in &step.requires {
            lines.push(("require".into(), c_warn(req).to_string()));
        }
        for eff in &step.effects {
            lines.push(("writes".into(), c_accent(eff).to_string()));
        }
        for call in &step.external_calls {
            lines.push(("calls".into(), c_danger(call).to_string()));
        }
        for ev in &step.events {
            lines.push(("emits".into(), c_muted(ev).to_string()));
        }
        for dep in &step.dependencies {
            lines.push(("depends".into(), c_warn(&dep.relationship).to_string()));
        }

        if lines.is_empty() {
            println!("  {}  {} {}", c_muted(pipe), c_muted("·"), c_muted("read-only"));
        } else {
            for (j, (label, value)) in lines.iter().enumerate() {
                let is_last = j == lines.len() - 1;
                let conn = if is_last { "└─" } else { "├─" };
                println!("  {}  {} {} {}",
                    c_muted(pipe), c_muted(conn), c_muted(&format!("{:<7}", label)), value);
            }
        }

        if !is_last_step {
            println!("  {}", c_muted("│"));
        }
    }

    if !n.observations.is_empty() {
        println!("\n  {}", c_muted("Observations:"));
        for obs in &n.observations {
            println!("  {} {}: {}",
                c_warn("⚠"), c_muted(&obs.kind.to_string()), obs.description);
        }
    }
    println!();
}

fn print_overview(contract: &ContractDef, narratives: &[FunctionNarrative]) {
    let total_paths: usize = narratives.iter().map(|n| n.total_paths).sum();
    println!("\n  {} — {} functions, {} paths\n",
        c_bright(&contract.name), narratives.len(), total_paths);

    for n in narratives {
        if n.name.is_empty() { continue; }
        let access = access_colored(&n.access);
        let paths = format!("{} paths", n.total_paths);
        let mut extras = Vec::new();
        if !n.state_writes.is_empty() {
            extras.push(format!("writes: {}", n.state_writes.join(", ")));
        }
        if !n.external_calls.is_empty() {
            extras.push(format!("calls: {}", n.external_calls.join(", ")));
        }
        let extra_str = if extras.is_empty() { String::new() }
            else { format!("  {}", c_muted(&extras.join(" | "))) };

        println!("  {} {:<20} {}{}",
            access, c_bright(&n.name), c_muted(&paths), extra_str);
    }

    let obs: Vec<&Observation> = narratives.iter().flat_map(|n| n.observations.iter()).collect();
    if !obs.is_empty() {
        println!("\n  {}", c_muted("Observations:"));
        let mut seen = std::collections::HashSet::new();
        for o in &obs {
            if seen.insert(&o.description) {
                println!("  {} {}: {}",
                    c_warn("⚠"), c_muted(&o.kind.to_string()), o.description);
            }
        }
    }
    println!();
}
