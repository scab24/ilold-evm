use colored::Colorize;

use ilold_core::exploration::commands::ScenarioInfo;
use ilold_core::exploration::session::MutationScope;
use ilold_core::exploration::timeline::{TimelineEntry, VariableTimeline};
use ilold_core::narrative::trace::{FlowKind, FlowNode, FlowTree};
use ilold_core::slicing::{SliceDirection, SliceEntry, SliceResult, StatementOrigin};

use crate::colors::{c_accent, c_bright, c_danger, c_muted, c_ok, c_warn};

pub fn term_width() -> usize {
    terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), _)| w as usize)
        .unwrap_or(80)
}

pub fn separator(label: &str) -> String {
    let width = term_width().min(100);
    let inner = format!("[ {} ]", label);
    let remaining = width.saturating_sub(inner.len() + 2);
    let left = remaining / 2;
    let right = remaining - left;
    format!(
        "  {}{}{}",
        "═".repeat(left).truecolor(60, 70, 90),
        inner.truecolor(190, 200, 215),
        "═".repeat(right).truecolor(60, 70, 90),
    )
}

pub fn header_box(lines: &[&str]) -> String {
    let max_content = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let inner_width = max_content + 2;

    let mut out = String::new();
    out.push_str(&format!(
        "  {}{}{}\n",
        "╭".truecolor(60, 70, 90),
        "─".repeat(inner_width).truecolor(60, 70, 90),
        "╮".truecolor(60, 70, 90),
    ));
    for line in lines {
        let visible_len = line.chars().count();
        let pad = max_content.saturating_sub(visible_len);
        out.push_str(&format!(
            "  {} {}{} {}\n",
            "│".truecolor(60, 70, 90),
            line,
            " ".repeat(pad),
            "│".truecolor(60, 70, 90),
        ));
    }
    out.push_str(&format!(
        "  {}{}{}",
        "╰".truecolor(60, 70, 90),
        "─".repeat(inner_width).truecolor(60, 70, 90),
        "╯".truecolor(60, 70, 90),
    ));
    out
}

pub fn pad_right(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - len))
    }
}


pub fn render_flow_tree(tree: &FlowTree) -> String {
    let mut out = String::new();

    let title = format!("{}::{}", tree.contract, tree.signature);
    let mods = if tree.modifiers.is_empty() {
        "modifiers: (none)".to_string()
    } else {
        format!("modifiers: {}", tree.modifiers.join(", "))
    };
    let depth = format!("max inlining depth: {}", tree.max_depth);
    let mut lines: Vec<&str> = vec![&title, &mods, &depth];
    let note = tree.path_stats.as_ref().and_then(|s| s.truncation_note()).map(|n| format!("paths {n}"));
    lines.extend(note.as_deref());
    out.push_str(&header_box(&lines));
    out.push('\n');
    out.push('\n');

    let mut step: usize = 1;
    render_flow_root(&tree.root, &mut step, &mut out);

    append_expand_hint(tree, &mut out);

    let mut raw_vars: Vec<&str> = Vec::new();
    collect_written_vars(&tree.root, &mut raw_vars);
    let mut base_vars: Vec<String> = raw_vars.iter()
        .map(|v| v.split('[').next().unwrap_or(v).to_string())
        .collect();
    base_vars.sort_unstable();
    base_vars.dedup();
    if !base_vars.is_empty() {
        let hints = base_vars.iter()
            .take(5)
            .map(|v| format!("sl {} {}", tree.function, v))
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if base_vars.len() > 5 {
            format!(" (+{} more)", base_vars.len() - 5)
        } else {
            String::new()
        };
        out.push_str(&format!("  {}{}\n", c_muted(&format!("→ {}", hints)), c_muted(&suffix)));
    }

    out
}

fn collect_written_vars<'a>(node: &'a FlowNode, out: &mut Vec<&'a str>) {
    match &node.kind {
        FlowKind::Write { target, .. } | FlowKind::StateWrite { variable: target } => {
            out.push(target.as_str());
        }
        _ => {}
    }
    for child in &node.children {
        collect_written_vars(child, out);
    }
}

fn append_expand_hint(tree: &FlowTree, out: &mut String) {
    let mut candidates: Vec<usize> = Vec::new();
    collect_depth_limited(&tree.root, &mut candidates);
    if candidates.is_empty() {
        return;
    }

    let total = candidates.len();
    candidates.truncate(10);
    let csv = candidates.iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if total > 10 {
        format!("  (… {} more)", total - 10)
    } else {
        String::new()
    };

    out.push('\n');
    out.push_str(&format!(
        "  {} {}{}\n",
        c_muted("tip: expand with `tr <func> +N` — candidates:"),
        c_accent(&csv),
        c_muted(&suffix),
    ));
}

fn collect_depth_limited(node: &FlowNode, out: &mut Vec<usize>) {
    if let FlowKind::InternalCall { depth_limited: true, .. } = &node.kind {
        out.push(node.step_id);
    }
    for child in &node.children {
        collect_depth_limited(child, out);
    }
}

fn render_flow_root(root: &FlowNode, step: &mut usize, out: &mut String) {
    let (icon, text) = format_flow_label(&root.kind);
    let gutter = format_gutter(*step);
    *step += 1;
    out.push_str(&format!(
        "{} {} {}\n",
        gutter,
        c_accent(icon),
        c_bright(&text),
    ));

    let n = root.children.len();
    for (i, child) in root.children.iter().enumerate() {
        render_flow_node(child, "", i == n - 1, step, out);
    }
}

fn render_flow_node(
    node: &FlowNode,
    parent_prefix: &str,
    is_last: bool,
    step: &mut usize,
    out: &mut String,
) {
    let connector = if is_last { "└─ " } else { "├─ " };
    let (icon, text) = format_flow_label(&node.kind);
    let gutter = format_gutter(*step);
    *step += 1;

    let suffix = match &node.from_modifier {
        Some(name) => format!("  {}", c_warn(&format!("[from: {}]", name))),
        None => String::new(),
    };

    let colored_icon = color_for_kind(&node.kind, icon);
    let colored_text = color_for_kind_text(&node.kind, &text);

    out.push_str(&format!(
        "{} {}{}{} {}{}\n",
        gutter,
        c_muted(parent_prefix),
        c_muted(connector),
        colored_icon,
        colored_text,
        suffix,
    ));

    let extension = if is_last { "   " } else { "│  " };
    let new_prefix = format!("{}{}", parent_prefix, extension);
    let n = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        render_flow_node(child, &new_prefix, i == n - 1, step, out);
    }
}

fn format_gutter(step: usize) -> String {
    format!("  {} {}", c_accent(&format!("{:03}", step)), c_muted("│"))
}

pub(crate) fn format_flow_label(kind: &FlowKind) -> (&'static str, String) {
    match kind {
        FlowKind::Entry { signature } => ("▶", signature.clone()),
        FlowKind::Require { condition, message } => {
            let text = match message {
                Some(m) => format!("require({}, {})", condition, m),
                None => format!("require({})", condition),
            };
            ("◇", text)
        }
        FlowKind::Assert { condition } => ("◇", format!("assert({})", condition)),
        FlowKind::Write { target, value, op } => {
            ("✏", format!("{} {} {}", target, op.as_str(), value))
        }
        FlowKind::StateWrite { variable } => ("✏", format!("write {}", variable)),
        FlowKind::StateRead { variable } => ("▸", format!("read {}", variable)),
        FlowKind::InternalCall {
            function,
            origin,
            depth_limited,
            ops_count,
        } => {
            let mut text = function.clone();
            if !origin.is_empty() {
                text.push_str(&format!("  [from: {}]", origin));
            }
            if *depth_limited {
                text.push_str(&format!("  [+{} ops, depth limited]", ops_count));
            }
            ("○", text)
        }
        FlowKind::ExternalCall { target, function } => {
            ("→", format!("{}.{}", target, function))
        }
        FlowKind::EmitEvent { name } => ("◆", format!("emit {}", name)),
        FlowKind::BranchTrue { condition } => ("?", format!("if ({}) == true", condition)),
        FlowKind::BranchFalse { condition } => ("?", format!("if ({}) == false", condition)),
        FlowKind::LoopHeader { kind } => ("↻", format!("loop ({})", kind)),
        FlowKind::Return => ("✓", "return".to_string()),
        FlowKind::Revert { reason } => {
            let text = match reason {
                Some(r) => format!("revert({})", r),
                None => "revert".to_string(),
            };
            ("✗", text)
        }
        FlowKind::EthTransfer { to } => ("$", format!("transfer ETH → {}", to)),
        FlowKind::AssemblyBlock => ("⚙", "assembly { … }".to_string()),
        FlowKind::DepthLimit => ("…", "depth limit".to_string()),
    }
}

fn color_for_kind(kind: &FlowKind, icon: &str) -> colored::ColoredString {
    match kind {
        FlowKind::Entry { .. } => c_accent(icon),
        FlowKind::Require { .. } | FlowKind::Assert { .. } => c_muted(icon),
        FlowKind::Write { .. } | FlowKind::StateWrite { .. } => c_danger(icon),
        FlowKind::StateRead { .. } => c_muted(icon),
        FlowKind::InternalCall { .. } => c_warn(icon),
        FlowKind::ExternalCall { .. } | FlowKind::EthTransfer { .. } => c_danger(icon),
        FlowKind::EmitEvent { .. } => c_accent(icon),
        FlowKind::BranchTrue { .. } | FlowKind::BranchFalse { .. } => c_warn(icon),
        FlowKind::LoopHeader { .. } => c_warn(icon),
        FlowKind::Return => c_ok(icon),
        FlowKind::Revert { .. } => c_danger(icon),
        FlowKind::AssemblyBlock => c_warn(icon),
        FlowKind::DepthLimit => c_muted(icon),
    }
}

fn color_for_kind_text(kind: &FlowKind, text: &str) -> colored::ColoredString {
    match kind {
        FlowKind::Entry { .. } => c_bright(text),
        FlowKind::Write { .. } | FlowKind::StateWrite { .. } => c_danger(text),
        FlowKind::ExternalCall { .. } | FlowKind::EthTransfer { .. } => c_danger(text),
        FlowKind::InternalCall { .. } => c_warn(text),
        FlowKind::BranchTrue { .. } | FlowKind::BranchFalse { .. } => c_warn(text),
        FlowKind::Revert { .. } => c_danger(text),
        FlowKind::Return => c_ok(text),
        _ => c_muted(text),
    }
}


pub fn render_variable_timeline(tl: &VariableTimeline) -> String {
    let mut out = String::new();
    out.push('\n');

    let header = format!("{} {}", c_bright(&tl.variable), c_muted("— mutation timeline"));
    out.push_str(&format!("  {}\n", header));
    out.push_str(&format!(
        "  {}\n",
        "═".repeat(60).truecolor(60, 70, 90)
    ));

    if tl.state_entries.is_empty() && tl.local_entries.is_empty() {
        out.push_str(&format!("  {}\n", c_muted(&format!("no mutations of '{}' in current session — add steps with 'c <func>' first", tl.variable))));
        return out;
    }

    if !tl.state_entries.is_empty() {
        out.push_str(&format!("  {}\n", c_warn("[state]")));
        render_entries(&tl.state_entries, &mut out);
    }

    if !tl.local_entries.is_empty() {
        out.push_str(&format!("  {}\n", c_warn("[local]")));
        render_entries(&tl.local_entries, &mut out);
    }

    let all_entries = tl.state_entries.iter().chain(tl.local_entries.iter());
    let mut seen = std::collections::HashSet::new();
    let hints: Vec<String> = all_entries
        .filter(|e| seen.insert(e.function.clone()))
        .map(|e| format!("sl {} {}", e.function, tl.variable))
        .collect();
    if !hints.is_empty() {
        out.push_str(&format!("  {}\n", c_muted(&format!("→ {}", hints.join(", ")))));
    }

    out.push('\n');
    out
}

fn render_entries(entries: &[TimelineEntry], out: &mut String) {
    let mut prev_step: Option<usize> = None;
    for entry in entries {
        if Some(entry.session_step_index) != prev_step {
            out.push_str(&format!(
                "    {} {}\n",
                c_accent(&format!("session step {}", entry.session_step_index)),
                c_bright(&entry.function),
            ));
            prev_step = Some(entry.session_step_index);
        }

        let change = if entry.value_expr.is_empty() {
            "written".to_string()
        } else {
            format!("{} {}", entry.operator.as_str(), entry.value_expr)
        };
        let flow_ref = entry.flow_step_id
            .map(|id| format!(" [trace step {}]", id))
            .unwrap_or_default();
        let via = entry.via.as_deref()
            .map(|c| format!(" via {}", c))
            .unwrap_or_default();
        let scope_tag = match entry.scope {
            MutationScope::State => "",
            MutationScope::Local => " (local)",
        };

        out.push_str(&format!(
            "      {} {} {}{}{}{}\n",
            c_danger("✏"),
            c_muted(&entry.target),
            c_muted(&change),
            c_muted(&flow_ref),
            c_muted(&via),
            c_muted(scope_tag),
        ));

        if !entry.reached_when.is_empty() {
            out.push_str(&format!("        {}\n", c_muted("reached when:")));
            for cond in &entry.reached_when {
                out.push_str(&format!("          {} {}\n", c_muted("•"), c_muted(cond)));
            }
        }
    }
}


pub fn render_slice_result(res: &SliceResult) -> String {
    let mut out = String::new();
    out.push('\n');

    let header = format!(
        "{} {} {} {}",
        c_bright(&res.function),
        c_muted("·"),
        c_bright(&res.variable),
        c_muted("— dataflow slice"),
    );
    out.push_str(&format!("  {}\n", header));
    out.push_str(&format!("  {}\n", "═".repeat(60).truecolor(60, 70, 90)));

    let show_backward = matches!(
        res.direction,
        SliceDirection::Backward | SliceDirection::Both
    );
    let show_forward = matches!(
        res.direction,
        SliceDirection::Forward | SliceDirection::Both
    );

    if show_backward {
        render_slice_side("backward", &res.backward, &res.variable, &mut out);
    }
    if show_forward {
        render_slice_side("forward", &res.forward, &res.variable, &mut out);
    }

    out.push_str(&format!(
        "  {}\n",
        c_muted(&format!("→ tr {} | tl {}", res.function, res.variable)),
    ));

    out.push('\n');
    out
}

fn render_slice_side(label: &str, entries: &[SliceEntry], var: &str, out: &mut String) {
    out.push_str(&format!("  {}\n", c_warn(&format!("[{}]", label))));
    if entries.is_empty() {
        let reason = if label == "backward" {
            format!("no definitions of '{}' found — may be a parameter or set only in constructor/modifier", var)
        } else {
            format!("no statements depend on '{}' after its definition in this function", var)
        };
        out.push_str(&format!("    {}\n", c_muted(&reason)));
        return;
    }
    for entry in entries {
        let line_tag = entry.span
            .map(|s| format!("L{:<4}", s.start_line))
            .unwrap_or_else(|| "L?   ".into());
        let origin_tag = match &entry.origin {
            StatementOrigin::FunctionBody => String::new(),
            StatementOrigin::Modifier(name) => format!("{} ", c_accent(&format!("[mod {}]", name))),
        };
        out.push_str(&format!(
            "    {} {}{}\n",
            c_muted(&line_tag),
            origin_tag,
            entry.text,
        ));
    }
}


pub fn render_scenario_list(items: &[ScenarioInfo]) -> String {
    if items.is_empty() {
        return format!("  {}\n", c_muted("(no scenarios)"));
    }

    let name_width = items
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);

    let title = format!(
        "scenarios — {} total, active: {}",
        items.len(),
        items
            .iter()
            .find(|s| s.active)
            .map(|s| s.name.as_str())
            .unwrap_or("?"),
    );
    let header = format!(
        "  {}   {}   {}",
        pad_right("", 1),
        pad_right("name", name_width),
        "steps",
    );
    let mut lines: Vec<String> = vec![title, header];
    for s in items {
        let marker = if s.active { "→" } else { " " };
        lines.push(format!(
            "  {}   {}   {}",
            marker,
            pad_right(&s.name, name_width),
            s.step_count,
        ));
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let mut out = header_box(&refs);
    out.push('\n');
    out
}

pub fn render_scenario_forked(from: &str, to: &str, at_step: usize) -> String {
    format!(
        "  {} Forked '{}' → '{}' at step {}",
        c_ok("✓"),
        c_accent(from),
        c_accent(to),
        at_step,
    )
}

pub fn render_scenario_created(name: &str) -> String {
    format!("  {} Created scenario '{}'", c_ok("✓"), c_accent(name))
}

pub fn render_scenario_switched(from: &str, to: &str) -> String {
    if from == to {
        format!("  {} Already on scenario '{}'", c_muted("·"), c_accent(to))
    } else {
        format!(
            "  {} Switched: '{}' → '{}'",
            c_ok("✓"),
            c_accent(from),
            c_accent(to),
        )
    }
}

pub fn render_scenario_deleted(name: &str) -> String {
    format!("  {} Deleted scenario '{}'", c_ok("✓"), c_accent(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_esc = false;
        for c in s.chars() {
            if in_esc {
                if c == 'm' {
                    in_esc = false;
                }
                continue;
            }
            if c == '\u{1b}' {
                in_esc = true;
                continue;
            }
            out.push(c);
        }
        out
    }

    fn make_require_tree(message: Option<&str>) -> FlowTree {
        FlowTree {
            contract: "C".into(),
            function: "f".into(),
            signature: "f()".into(),
            modifiers: vec![],
            max_depth: 2,
            path_stats: None,
            root: FlowNode {
                step_id: 0,
                depth: 0,
                kind: FlowKind::Entry { signature: "f()".into() },
                from_modifier: None,
                children: vec![FlowNode {
                    step_id: 1,
                    depth: 1,
                    kind: FlowKind::Require {
                        condition: "x == 1".into(),
                        message: message.map(|s| s.into()),
                    },
                    from_modifier: None,
                    children: vec![],
                }],
            },
        }
    }

    #[test]
    fn render_require_message_not_double_escaped() {
        let tree = make_require_tree(Some("\"LOCKED\""));
        let rendered = strip_ansi(&render_flow_tree(&tree));

        assert!(
            rendered.contains("require(x == 1, \"LOCKED\")"),
            "expected unescaped message in output.\nrendered:\n{}",
            rendered,
        );
        assert!(
            !rendered.contains("\\\""),
            "rendered output must not contain backslash-escaped quotes.\nrendered:\n{}",
            rendered,
        );
    }

    #[test]
    fn render_require_without_message() {
        let tree = make_require_tree(None);
        let rendered = strip_ansi(&render_flow_tree(&tree));
        assert!(
            rendered.contains("require(x == 1)"),
            "rendered:\n{}",
            rendered,
        );
    }
}
