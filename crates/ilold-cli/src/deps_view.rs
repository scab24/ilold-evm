use ilold_core::depgraph::ContractDeps;
use ilold_core::relations::{ContractLink, Tag};
use serde_json::Value;

use crate::colors::*;
use crate::fmt;

pub struct DepRow {
    pub name: String,
    pub inherits: bool,
    pub calls: bool,
    pub call_count: usize,
    pub holds: bool,
    pub link: Option<ContractLink>,
}

pub fn dep_rows(deps: &ContractDeps, name: &str, reverse: bool) -> Option<Vec<DepRow>> {
    deps.node(name)?;
    let refs = if reverse {
        deps.dependents(name)
    } else {
        deps.dependencies(name)
    };
    Some(
        refs.iter()
            .map(|r| DepRow {
                name: r.node.name.clone(),
                inherits: r.edge.inherits,
                calls: r.edge.calls,
                call_count: r.edge.call_count,
                holds: r.edge.holds,
                link: r.edge.link.clone(),
            })
            .collect(),
    )
}

pub fn rows_from_depgraph_json(json: &Value, name: &str, reverse: bool) -> Vec<DepRow> {
    let mut rows: Vec<DepRow> = json["edges"]
        .as_array()
        .map(|edges| {
            edges
                .iter()
                .filter_map(|e| {
                    let d = &e["data"];
                    let source = d["source"].as_str()?;
                    let target = d["target"].as_str()?;
                    let (matches, other) = if reverse {
                        (target == name, source)
                    } else {
                        (source == name, target)
                    };
                    if !matches {
                        return None;
                    }
                    let kinds = d["kinds"].as_array();
                    let has = |k: &str| {
                        kinds
                            .map(|a| a.iter().any(|v| v.as_str() == Some(k)))
                            .unwrap_or(false)
                    };
                    Some(DepRow {
                        name: other.to_string(),
                        inherits: has("inherits"),
                        calls: has("calls"),
                        call_count: d["call_count"].as_u64().unwrap_or(0) as usize,
                        holds: has("holds"),
                        link: serde_json::from_value(d["link"].clone()).ok(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

pub fn print_direction(name: &str, reverse: bool, rows: Option<Vec<DepRow>>) {
    let Some(rows) = rows else {
        println!("  {}", c_danger(&format!("Contract '{name}' not found")));
        return;
    };

    let header = if reverse {
        format!("{name} ← used by (blast radius)")
    } else {
        format!("{name} → depends on")
    };
    println!("\n  {}", c_bright(&header));

    if rows.is_empty() {
        let msg = if reverse {
            "nothing depends on it"
        } else {
            "no in-project dependencies"
        };
        println!("    {}\n", c_muted(msg));
        return;
    }

    let max = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0);
    for row in &rows {
        println!("    {}  {}", c_accent(&fmt::pad_right(&row.name, max)), row_tags(row).join(" "));
    }
    println!();
}

pub fn row_tags(row: &DepRow) -> Vec<String> {
    let mut tags = Vec::new();
    if row.inherits {
        tags.push(c_accent("inherits").to_string());
    }
    if row.calls {
        tags.push(c_warn(&format!("calls×{}", row.call_count)).to_string());
    }
    if row.holds {
        tags.push(c_muted("holds").to_string());
    }
    if let Some(link) = &row.link {
        let basis = serde_json::to_value(link.basis).ok().and_then(|b| b.as_str().map(str::to_string)).unwrap_or_default();
        let via = if link.via_state_vars.is_empty() { String::new() } else { format!(" via {}", link.via_state_vars.join(", ")) };
        let text = format!("dispatches {basis}×{}{via}", link.calls.len());
        tags.push(if link.tag == Tag::Ambiguous { c_danger(&format!("{text} AMBIGUOUS")) } else { c_ok(&text) }.to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilold_core::parse::solc_frontend::SolcFrontend;
    use std::path::PathBuf;

    fn cross_deps() -> ContractDeps {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests/fixtures/solc/cross");
        let project = SolcFrontend.parse_project(&p).expect("parse cross fixture");
        ContractDeps::build(&project)
    }

    #[test]
    fn local_rows_carry_edge_kinds() {
        let d = cross_deps();

        let vault = dep_rows(&d, "Vault", false).expect("Vault exists");
        let ipool = vault.iter().find(|r| r.name == "IPool").expect("Vault → IPool");
        assert!(ipool.calls && ipool.holds, "pool.supply() + IPool pool => calls + holds");
        let safe = vault.iter().find(|r| r.name == "SafeMath").expect("Vault → SafeMath");
        assert!(safe.calls && !safe.holds && !safe.inherits);

        let blast = dep_rows(&d, "IPool", true).expect("IPool exists");
        assert!(blast.iter().any(|r| r.name == "Vault"), "Vault is in IPool blast radius");

        assert!(dep_rows(&d, "Nope", false).is_none(), "missing contract => None");
    }

    #[test]
    fn rows_show_the_contracts_that_may_answer_interface_calls() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/dispatch");
        let project = SolcFrontend.parse_project(&root).expect("parse dispatch fixture");
        let analysis = ilold_core::analysis::ProjectAnalysis::build(&project, &ilold_core::analysis::AnalysisParams::new(1));
        let rows = dep_rows(&analysis.dep_graph, "Router", false).unwrap();
        let tags = |name: &str| row_tags(rows.iter().find(|r| r.name == name).unwrap()).join(" ");
        assert!(tags("Vault").contains("dispatches inherits_interface") && tags("Vault").contains("via vault"), "{}", tags("Vault"));
        assert!(tags("HookA").contains("AMBIGUOUS"), "{}", tags("HookA"));
        assert!(tags("Registry").contains("selector_match"), "{}", tags("Registry"));
        let blast = dep_rows(&analysis.dep_graph, "Vault", true).unwrap();
        assert!(row_tags(&blast[0]).join(" ").contains("dispatches inherits_interface"), "used by Router through IVault");

        let json = serde_json::json!({"edges": [{"data": {"source": "Router", "target": "Vault", "kinds": ["dispatches"], "call_count": 0,
            "link": serde_json::to_value(rows.iter().find(|r| r.name == "Vault").unwrap().link.as_ref().unwrap()).unwrap()}}]});
        let from_json = rows_from_depgraph_json(&json, "Router", false);
        assert_eq!(row_tags(&from_json[0]), row_tags(rows.iter().find(|r| r.name == "Vault").unwrap()));
    }

    #[test]
    fn json_rows_filter_by_direction() {
        let json = serde_json::json!({
            "nodes": [],
            "edges": [
                {"data":{"source":"Vault","target":"IPool","kinds":["calls","holds"],"call_count":2}},
                {"data":{"source":"LendingPool","target":"IPool","kinds":["inherits"],"call_count":0}}
            ]
        });

        let deps = rows_from_depgraph_json(&json, "Vault", false);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "IPool");
        assert!(deps[0].calls && deps[0].holds && deps[0].call_count == 2);

        let blast = rows_from_depgraph_json(&json, "IPool", true);
        let names: Vec<&str> = blast.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["LendingPool", "Vault"]);
    }
}
