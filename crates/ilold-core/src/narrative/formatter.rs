use std::fmt::Write;

use crate::pathtree::types::TerminalKind;

use super::types::*;

pub fn function_to_markdown(n: &FunctionNarrative) -> String {
    let mut md = String::new();

    writeln!(md, "## {} ({})", n.name, n.access).unwrap();
    writeln!(md, "**Contract**: {} | **Paths**: {} ({} return, {} revert)",
        n.contract, n.total_paths, n.happy_paths, n.revert_paths).unwrap();

    if !n.modifiers.is_empty() {
        writeln!(md, "**Modifiers**: {}", n.modifiers.join(", ")).unwrap();
    }
    let has_writes = !n.state_writes.is_empty();
    let has_calls = !n.external_calls.is_empty();
    if has_writes && has_calls {
        writeln!(md, "**Writes**: {} | **Calls**: {}",
            n.state_writes.join(", "), n.external_calls.join(", ")).unwrap();
    } else if has_writes {
        writeln!(md, "**Writes**: {}", n.state_writes.join(", ")).unwrap();
    } else if has_calls {
        writeln!(md, "**Calls**: {}", n.external_calls.join(", ")).unwrap();
    }

    let mut happy: Vec<&PathNarrative> = n.paths.iter()
        .filter(|p| p.terminal == TerminalKind::Return)
        .collect();
    let mut revert: Vec<&PathNarrative> = n.paths.iter()
        .filter(|p| p.terminal != TerminalKind::Return)
        .collect();
    happy.sort_by_key(|p| p.id);
    revert.sort_by_key(|p| p.id);

    for path in happy.iter().chain(revert.iter()) {
        writeln!(md).unwrap();
        format_path(&mut md, path);
    }

    if !n.observations.is_empty() {
        writeln!(md).unwrap();
        writeln!(md, "### Observations").unwrap();
        for obs in &n.observations {
            writeln!(md, "- **{}**: {}", obs.kind, obs.description).unwrap();
        }
    }

    md
}

pub fn sequence_to_markdown(n: &SequenceNarrative) -> String {
    let mut md = String::new();

    let names: Vec<&str> = n.steps.iter().map(|s| s.function.as_str()).collect();
    writeln!(md, "## Sequence: {}", names.join(" → ")).unwrap();
    writeln!(md, "**Contract**: {}", n.contract).unwrap();

    for (i, step) in n.steps.iter().enumerate() {
        writeln!(md).unwrap();
        writeln!(md, "### Step {} — {} ({})", i + 1, step.function, step.access).unwrap();

        if !step.requires.is_empty() {
            writeln!(md, "**Requires**: {}", step.requires.join(", ")).unwrap();
        }
        if !step.effects.is_empty() {
            writeln!(md, "**Effects**: {}", step.effects.join(", ")).unwrap();
        }
        if !step.external_calls.is_empty() {
            writeln!(md, "**Calls**: {}", step.external_calls.join(", ")).unwrap();
        }
        if !step.events.is_empty() {
            writeln!(md, "**Events**: {}", step.events.join(", ")).unwrap();
        }

        for dep in &step.dependencies {
            writeln!(md, "- *Dependency*: {}", dep.relationship).unwrap();
        }
    }

    if !n.observations.is_empty() {
        writeln!(md).unwrap();
        writeln!(md, "### Observations").unwrap();
        for obs in &n.observations {
            writeln!(md, "- **{}**: {}", obs.kind, obs.description).unwrap();
        }
    }

    md
}

pub fn overview_to_markdown(
    contract_name: &str,
    narratives: &[FunctionNarrative],
) -> String {
    let mut md = String::new();

    let total_paths: usize = narratives.iter().map(|n| n.total_paths).sum();
    let total_ext: usize = narratives.iter().map(|n| n.external_calls.len()).sum();
    writeln!(md, "## {} — {} functions, {} paths, {} external calls",
        contract_name, narratives.len(), total_paths, total_ext).unwrap();

    writeln!(md).unwrap();
    writeln!(md, "| Function | Access | Paths | Writes | Calls |").unwrap();
    writeln!(md, "|----------|--------|-------|--------|-------|").unwrap();

    for n in narratives {
        let writes = if n.state_writes.is_empty() { "—".into() } else { n.state_writes.join(", ") };
        let calls = if n.external_calls.is_empty() { "—".into() } else { n.external_calls.join(", ") };
        writeln!(md, "| {} | {} | {} ({}/{}R) | {} | {} |",
            n.name, n.access, n.total_paths, n.happy_paths, n.revert_paths,
            writes, calls).unwrap();
    }

    let all_obs: Vec<&Observation> = narratives.iter()
        .flat_map(|n| n.observations.iter())
        .collect();

    if !all_obs.is_empty() {
        writeln!(md).unwrap();
        writeln!(md, "### Key Observations").unwrap();
        for obs in all_obs {
            writeln!(md, "- **{}**: {}", obs.kind, obs.description).unwrap();
        }
    }

    md
}

fn format_path(md: &mut String, path: &PathNarrative) {
    let terminal = match path.terminal {
        TerminalKind::Return => "Return",
        TerminalKind::Revert => "Revert",
        TerminalKind::DepthCutoff => "DepthCutoff",
        TerminalKind::LoopCutoff => "LoopCutoff",
    };
    writeln!(md, "### Path #{} → {}", path.id, terminal).unwrap();

    for (i, step) in path.steps.iter().enumerate() {
        let branch_str = match step.branch {
            Some(BranchDirection::True) => " [✓]",
            Some(BranchDirection::False) => " [✗]",
            None => "",
        };
        write!(md, "{}. {} {}{}", i + 1, step.step_type.icon(), step.description, branch_str).unwrap();
        if let Some(detail) = &step.detail {
            write!(md, " — {}", detail).unwrap();
        }
        writeln!(md).unwrap();
    }
}
