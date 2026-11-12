use std::fmt::Write;

use ilold_core::relations::{
    AssetKind, ConnectionGroup, Connections, ContractCall, ContractCallKind, Party, RouteCall, Routes, Tag, ValueFlowPairs, ValueReason,
    ValueTransfer,
};

use crate::colors::*;
use crate::fmt::api_name;

/// `Contract::name` without the contract when it is `contract`.
fn local<'a>(id: &'a str, contract: &str) -> &'a str {
    id.strip_prefix(contract).and_then(|rest| rest.strip_prefix("::")).unwrap_or(id)
}

/// The asset as the value views name it, with its token.
fn asset(kind: AssetKind, reason: Option<ValueReason>, token: Option<&str>) -> String {
    let mut asset = match reason {
        Some(ValueReason::Currency) => "ETH or token, set by the pool".to_string(),
        _ => api_name(serde_json::to_value(kind)),
    };
    if let Some(token) = token {
        write!(asset, " {token}").ok();
    }
    asset
}

/// `caller → callee`, its kind, line, the state variable holding the address and the value it moves; one row per call, whatever its candidates.
fn call_row(call: &ContractCall, contract: &str, transfers: &[ValueTransfer]) -> String {
    let callees: Vec<&str> = call.callees.iter().map(|d| d.function.as_str()).collect();
    let target = match callees.as_slice() {
        [] => call.target.clone().unwrap_or_else(|| "?".into()),
        [one] => one.to_string(),
        many => format!("{} possible: {}", many.len(), many.join(", ")),
    };
    let mut row = format!("{} → {target}", local(&call.function, contract));
    if call.kind != ContractCallKind::Call {
        write!(row, "  {}", c_muted(&api_name(serde_json::to_value(call.kind)))).ok();
    }
    write!(row, "  {}", c_muted(&format!("line {}", call.span.start_line))).ok();
    if let Some(via) = &call.via {
        write!(row, "  {}", c_muted(&format!("via {via}"))).ok();
    }
    let moved = transfers.iter().filter(|t| t.in_context == call.in_context && call.transfers.contains(&t.id));
    for t in moved {
        let amount = t.amount_expr.as_deref().map(|a| format!(" {a}")).unwrap_or_default();
        write!(row, "  {}", c_warn(&format!("moves {}{amount}", asset(t.asset_kind, t.reason, t.token_expr.as_deref())))).ok();
    }
    if call.in_try {
        write!(row, "  {}", c_muted("in try")).ok();
    }
    if call.callees.iter().any(|d| d.tag == Tag::Ambiguous) {
        write!(row, "  {}", c_danger("AMBIGUOUS")).ok();
    }
    if let Some(reason) = call.unresolved_reason {
        write!(row, "  {}", c_warn(&api_name(serde_json::to_value(reason)))).ok();
    }
    row
}

fn groups(out: &mut String, title: String, empty: &str, groups: &[ConnectionGroup], c: &Connections) {
    writeln!(out, "\n  {}", c_bright(&title)).ok();
    if groups.is_empty() {
        writeln!(out, "    {}", c_muted(empty)).ok();
    }
    for group in groups {
        let calls = if group.calls.len() == 1 { "1 call".to_string() } else { format!("{} calls", group.calls.len()) };
        writeln!(out, "    {}  {}", c_accent(&group.contract), c_muted(&calls)).ok();
        for call in &group.calls {
            writeln!(out, "      {}", call_row(call, &c.contract, &c.transfers)).ok();
        }
    }
}

/// The connection rows in the order the API lists them.
pub fn links(c: &Connections) -> String {
    let mut out = String::new();
    groups(&mut out, format!("{} → calls", c.contract), "calls no other contract", &c.outgoing, c);
    groups(&mut out, format!("{} ← called by", c.contract), "no contract calls it", &c.incoming, c);
    if !c.unresolved.is_empty() {
        writeln!(out, "\n  {}", c_bright("Unresolved")).ok();
        for call in &c.unresolved {
            writeln!(out, "      {}", call_row(call, &c.contract, &c.transfers)).ok();
        }
    }
    if !c.assignments.is_empty() {
        writeln!(out, "\n  {}", c_bright("Addresses set at")).ok();
        for site in &c.assignments {
            let place = site.function.as_deref().unwrap_or("declaration");
            let line = c_muted(&format!("line {}", site.span.start_line));
            writeln!(out, "      {} = {}  {}  {place}  {line}", site.state_var, site.expr_text, c_muted(&api_name(serde_json::to_value(site.kind)))).ok();
        }
    }
    out.push('\n');
    out
}

fn route_call(call: &RouteCall) -> String {
    let mut row = format!("{} → {}", call.call.function, call.callee.function);
    if call.call.kind != ContractCallKind::Call {
        write!(row, "  {}", c_muted(&api_name(serde_json::to_value(call.call.kind)))).ok();
    }
    write!(row, "  {}", c_muted(&format!("line {}", call.call.span.start_line))).ok();
    if !call.call.transfers.is_empty() {
        write!(row, "  {}", c_warn("moves value")).ok();
    }
    if call.call.in_try {
        write!(row, "  {}", c_muted("in try")).ok();
    }
    if !call.recorded {
        write!(row, "  {}", c_warn("not recorded from the previous step")).ok();
    }
    row
}

/// The shortest routes between the two contracts, step by step.
pub fn route(reads: bool, found: &Routes) -> String {
    let (from, to) = (&found.from, &found.to);
    let mut out = String::new();
    let follows = if reads { "calls and reads" } else { "calls" };
    if found.routes.is_empty() {
        let hint = if reads { "" } else { "; --reads also follows reads" };
        writeln!(out, "\n  {}\n", c_muted(&format!("No route from {from} to {to} following {follows}{hint}"))).ok();
        return out;
    }
    let more = if found.truncated { ", more equally short ones left out" } else { "" };
    writeln!(out, "\n  {}  {}", c_bright(&format!("{from} → {to}")), c_muted(&format!("{} shortest route(s) following {follows}{more}", found.routes.len()))).ok();
    for (i, r) in found.routes.iter().enumerate() {
        let path: Vec<&str> = r.steps.iter().map(|s| s.from.as_str()).chain(r.steps.last().map(|s| s.to.as_str())).collect();
        writeln!(out, "\n  {} {}", c_muted(&format!("route {}", i + 1)), path.join(" → ")).ok();
        for step in &r.steps {
            writeln!(out, "    {}", c_accent(&format!("{} → {}", step.from, step.to))).ok();
            for call in &step.calls {
                writeln!(out, "      {}", route_call(call)).ok();
            }
        }
    }
    out.push('\n');
    out
}

fn party(p: &Party) -> String {
    match p {
        Party::Contract(keys) if keys.len() == 1 => keys[0].clone(),
        Party::Contract(keys) => format!("{} (AMBIGUOUS)", keys.join(" | ")),
        Party::Declared(key) => format!("{key} (outside the project)"),
        Party::Expression(Some(text)) => format!("`{text}`"),
        Party::Expression(None) => "(no account)".into(),
    }
}

/// Who pays whom, per running contract, as the value pairs group the transfers.
pub fn value(flow: &ValueFlowPairs) -> String {
    let mut out = String::new();
    let title = flow.contract.as_ref().map_or("Value flow".to_string(), |c| format!("Value flow of {c}"));
    let transfers = flow.pairs.iter().map(|p| p.transfers.len()).sum::<usize>();
    writeln!(out, "\n  {}  {}", c_bright(&title), c_muted(&format!("{} pair(s), {transfers} transfer(s)", flow.pairs.len()))).ok();
    if flow.pairs.is_empty() {
        writeln!(out, "    {}", c_muted("no value moves")).ok();
    }
    let mut context = None;
    for pair in &flow.pairs {
        if context != Some(&pair.in_context) {
            writeln!(out, "    {}", c_accent(&format!("as {} runs", pair.in_context))).ok();
            context = Some(&pair.in_context);
        }
        let asset = asset(pair.asset_kind, pair.reason, pair.token_expr.as_deref());
        let functions = pair.functions.iter().map(|f| local(f, &pair.in_context)).collect::<Vec<_>>().join(", ");
        writeln!(out, "      {} → {}  {}  ×{}  {}", party(&pair.from), party(&pair.to), c_warn(&asset), pair.transfers.len(), c_muted(&functions)).ok();
    }
    out.push('\n');
    out
}
