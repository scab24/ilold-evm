use std::fmt::Write;

use ilold_core::relations::{
    ConnectionGroup, Connections, ContractCall, ContractCallKind, Party, RouteCall, Routes, Tag, ValueFlowPairs, ValueReason,
};
use serde_json::{json, Value};

use crate::colors::*;

/// The serialized name of a fact's enum value, as the API and the export write it.
fn word(value: Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

/// `Contract::name` without the contract when it is `contract`.
fn local<'a>(id: &'a str, contract: &str) -> &'a str {
    id.strip_prefix(contract).and_then(|rest| rest.strip_prefix("::")).unwrap_or(id)
}

/// `caller → callee`, its kind, line and the state variable holding the address; one row per call, whatever its candidates.
fn call_row(call: &ContractCall, contract: &str) -> String {
    let callees: Vec<&str> = call.callees.iter().map(|d| d.function.as_str()).collect();
    let target = match callees.as_slice() {
        [] => call.target.clone().unwrap_or_else(|| "?".into()),
        [one] => one.to_string(),
        many => format!("{} possible: {}", many.len(), many.join(", ")),
    };
    let mut row = format!("{} → {target}", local(&call.function, contract));
    if call.kind != ContractCallKind::Call {
        write!(row, "  {}", c_muted(&word(json!(call.kind)))).ok();
    }
    write!(row, "  {}", c_muted(&format!("line {}", call.span.start_line))).ok();
    if let Some(via) = &call.via {
        write!(row, "  {}", c_muted(&format!("via {via}"))).ok();
    }
    if call.callees.iter().any(|d| d.tag == Tag::Ambiguous) {
        write!(row, "  {}", c_danger("AMBIGUOUS")).ok();
    }
    if let Some(reason) = call.unresolved_reason {
        write!(row, "  {}", c_warn(&word(json!(reason)))).ok();
    }
    row
}

fn groups(out: &mut String, title: String, empty: &str, groups: &[ConnectionGroup], contract: &str) {
    writeln!(out, "\n  {}", c_bright(&title)).ok();
    if groups.is_empty() {
        writeln!(out, "    {}", c_muted(empty)).ok();
    }
    for group in groups {
        let calls = if group.calls.len() == 1 { "1 call".to_string() } else { format!("{} calls", group.calls.len()) };
        writeln!(out, "    {}  {}", c_accent(&group.contract), c_muted(&calls)).ok();
        for call in &group.calls {
            writeln!(out, "      {}", call_row(call, contract)).ok();
        }
    }
}

/// The connection rows in the order the API lists them.
pub fn links(c: &Connections) -> String {
    let mut out = String::new();
    groups(&mut out, format!("{} → calls", c.contract), "calls no other contract", &c.outgoing, &c.contract);
    groups(&mut out, format!("{} ← called by", c.contract), "no contract calls it", &c.incoming, &c.contract);
    if !c.unresolved.is_empty() {
        writeln!(out, "\n  {}", c_bright("Unresolved")).ok();
        for call in &c.unresolved {
            writeln!(out, "      {}", call_row(call, &c.contract)).ok();
        }
    }
    if !c.assignments.is_empty() {
        writeln!(out, "\n  {}", c_bright("Addresses set at")).ok();
        for site in &c.assignments {
            let place = site.function.as_deref().unwrap_or("declaration");
            let line = c_muted(&format!("line {}", site.span.start_line));
            writeln!(out, "      {} = {}  {}  {place}  {line}", site.state_var, site.expr_text, c_muted(&word(json!(site.kind)))).ok();
        }
    }
    out.push('\n');
    out
}

fn route_call(call: &RouteCall) -> String {
    let mut row = format!("{} → {}", call.call.function, call.callee.function);
    if call.call.kind != ContractCallKind::Call {
        write!(row, "  {}", c_muted(&word(json!(call.call.kind)))).ok();
    }
    write!(row, "  {}", c_muted(&format!("line {}", call.call.span.start_line))).ok();
    if !call.recorded {
        write!(row, "  {}", c_warn("not recorded from the previous step")).ok();
    }
    row
}

/// The shortest routes from `from` to `to`, step by step.
pub fn route(from: &str, to: &str, reads: bool, found: &Routes) -> String {
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
pub fn value(contract: Option<&str>, flow: &ValueFlowPairs) -> String {
    let mut out = String::new();
    let title = contract.map_or("Value flow".to_string(), |c| format!("Value flow of {c}"));
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
        let mut asset = match pair.reason {
            Some(ValueReason::Currency) => "ETH or token, set by the pool".to_string(),
            _ => word(json!(pair.asset_kind)),
        };
        if let Some(token) = &pair.token_expr {
            write!(asset, " {token}").ok();
        }
        let functions = pair.functions.iter().map(|f| local(f, &pair.in_context)).collect::<Vec<_>>().join(", ");
        writeln!(out, "      {} → {}  {}  ×{}  {}", party(&pair.from), party(&pair.to), c_warn(&asset), pair.transfers.len(), c_muted(&functions)).ok();
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
    use ilold_core::parse::solc_frontend::SolcFrontend;
    use ilold_core::relations::routes;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    fn analysis() -> &'static ProjectAnalysis {
        static ANALYSIS: OnceLock<ProjectAnalysis> = OnceLock::new();
        ANALYSIS.get_or_init(|| {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ilold-web/tests/fixtures/connections");
            ProjectAnalysis::build(&SolcFrontend.parse_project(&root).expect("parse connections fixture"), &AnalysisParams::new(1))
        })
    }

    /// The REPL attached to a server reads the same facts from their JSON.
    macro_rules! attached {
        ($facts:expr) => {
            serde_json::from_value(serde_json::to_value(&$facts).unwrap()).unwrap()
        };
    }

    #[test]
    fn links_list_every_call_under_the_contracts_that_may_answer_it() {
        let curve = analysis().relations.connections("Curve");
        let text = links(&curve);
        assert_eq!(text, links(&attached!(curve)));
        let calls = text.find("Curve → calls").unwrap();
        let called = text.find("Curve ← called by").unwrap();
        let den_a = text.find("DenA").unwrap();
        assert!(calls < den_a && den_a < called, "{text}");
        assert!(text.contains("buy → 2 possible: DenA::deliver, DenB::deliver"), "{text}");
        assert!(text.contains("AMBIGUOUS"), "{text}");
        assert!(text.contains("CurveBase::_graduate → Hook::fee  read"), "inherited code runs in Curve: {text}");
        assert!(text.contains("Hook  2 calls") && text.contains("Router  2 calls"), "{text}");
        assert!(text.contains("CurveBase::hook = hook_  constructor_param  CurveBase::constructor"), "{text}");

        let hook = links(&analysis().relations.connections("Hook"));
        assert!(hook.contains("Unresolved") && hook.contains("migrate → IPoolManager::unlock") && hook.contains("out_of_scope_interface"), "{hook}");
        assert!(hook.contains("calls no other contract"), "{hook}");
    }

    #[test]
    fn a_route_prints_each_step_and_the_calls_making_it() {
        let a = analysis();
        let found = routes(&a.relations, &a.call_graphs, "Router", "Hook", false);
        let text = route("Router", "Hook", false, &found);
        assert_eq!(text, route("Router", "Hook", false, &attached!(found)));
        assert!(text.contains("route 1 Router → Curve → Hook"), "{text}");
        assert!(text.contains("Router::buyWith → Curve::buy  line"), "{text}");
        assert!(text.contains("CurveBase::_graduate → Hook::migrate  line"), "{text}");
        assert!(!text.contains("Hook::fee"), "reads only when asked: {text}");
        let reads = route("Router", "Hook", true, &routes(&a.relations, &a.call_graphs, "Router", "Hook", true));
        assert!(reads.contains("Hook::fee  read"), "{reads}");
        let none = route("Hook", "Router", false, &routes(&a.relations, &a.call_graphs, "Hook", "Router", false));
        assert!(none.contains("No route from Hook to Router following calls; --reads also follows reads"), "{none}");
    }

    #[test]
    fn value_flow_names_a_contract_only_where_the_code_resolves_one() {
        let flow = analysis().relations.value_flow(Some("Hook"));
        let text = value(Some("Hook"), &flow);
        assert_eq!(text, value(Some("Hook"), &attached!(flow)));
        assert!(text.contains("Value flow of Hook") && text.contains("1 pair(s), 1 transfer(s)"), "{text}");
        assert!(text.contains("as Curve runs") && text.contains("Curve → Hook  native  ×1  CurveBase::_graduate"), "{text}");
        let all = value(None, &analysis().relations.value_flow(None));
        assert!(all.contains("Curve → `payable(to)`  native") && all.contains("Token → `to`  erc20"), "{all}");
    }
}
