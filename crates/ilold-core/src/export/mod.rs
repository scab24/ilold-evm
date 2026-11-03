//! A deterministic JSON Lines map of a project: an envelope, then nodes, then edges, each part sorted by id.
mod facts;
mod ids;
mod inventory;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::analysis::{AnalysisParams, ProjectAnalysis};
use crate::classify::guards::MAX_DEPTH;
use crate::model::project::{CompileFailure, Project};
use crate::parse::error::ParseError;
use crate::parse::solc_frontend::{project_layout, ProjectLayout, SolcFrontend};
use crate::sequence::effects::MAX_ROUNDS;

use ids::{Ids, Paths};
use inventory::Repo;

/// Changes whenever a record kind or field changes meaning.
pub const SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Complete,
    /// Files outside the sources failed to compile; the rest is exported.
    CompleteWithWarnings,
    /// The sources failed to compile: only the envelope and its file inventory.
    NoFacts,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Complete => "complete",
            Status::CompleteWithWarnings => "complete_with_warnings",
            Status::NoFacts => "no_facts",
        }
    }
}

/// The ilold build an export comes from; unknown outside a git checkout.
#[derive(Debug, Clone, Default)]
pub struct Build {
    pub git_commit: Option<String>,
    /// The build inputs differ from `git_commit`.
    pub dirty: Option<bool>,
}

pub struct GraphExport {
    pub status: Status,
    pub jsonl: String,
}

/// `root` is the Foundry root; an error means nothing could be exported.
pub fn export_graph(root: &Path, params: &AnalysisParams, build: &Build) -> Result<GraphExport, String> {
    let layout = project_layout(root).map_err(|e| e.to_string())?;
    let repo = Repo::locate(&layout.root);
    let paths = Paths { base: repo.root.clone().unwrap_or_else(|| layout.root.clone()) };
    let (project, key) = match SolcFrontend.parse_project_keyed(&layout.root) {
        Ok(parsed) => parsed,
        Err(ParseError::SyntaxError { failures, .. }) => {
            let envelope = Envelope { build, layout: &layout, repo: &repo, paths: &paths, project: None, failures: &failures };
            let line = envelope.render(Status::NoFacts, json!({}), json!({}));
            return Ok(GraphExport { status: Status::NoFacts, jsonl: format!("{line}\n") });
        }
        Err(e) => return Err(e.to_string()),
    };
    let analysis = ProjectAnalysis::load_or_build(&project, key.as_ref(), params);
    let graph = facts::Builder::new(&project, &analysis, Ids::new(&project, &paths)).build();

    let status = if project.compile_failures.is_empty() { Status::Complete } else { Status::CompleteWithWarnings };
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for record in graph.nodes.values().chain(graph.edges.values()) {
        *counts.entry(record["kind"].as_str().unwrap_or_default()).or_default() += 1;
    }
    let path_stats: Vec<&Value> = graph.nodes.values().map(|n| &n["paths"]).filter(|p| p.is_object()).collect();
    let sum = |field: &str| path_stats.iter().filter_map(|p| p[field].as_u64()).sum::<u64>();
    let t = &graph.truncation;
    let pruning = &params.pruning;
    let truncation = json!({
        "effects_incomplete": t.effects_incomplete,
        "reads_incomplete": t.reads_incomplete,
        "reverts_incomplete": t.reverts_incomplete,
        "guards_incomplete": t.guards_incomplete,
        "paths_truncated": path_stats.iter().filter(|p| p["truncated"] == true).count(),
        "depth_cutoffs": sum("depth_cutoff"),
        "loop_cutoffs": sum("loop_cutoff"),
        "paths_dropped": sum("paths_dropped"),
        "limits": {
            "max_depth": pruning.max_depth,
            "max_loop_unroll": pruning.max_loop_unroll,
            "max_paths": pruning.max_paths,
            "guard_depth": MAX_DEPTH,
            "effect_rounds": MAX_ROUNDS,
        },
    });
    let envelope =
        Envelope { build, layout: &layout, repo: &repo, paths: &paths, project: Some(&project), failures: &project.compile_failures };
    let mut jsonl = envelope.render(status, truncation, json!(counts));
    jsonl.push('\n');
    for record in graph.nodes.values().chain(graph.edges.values()) {
        jsonl.push_str(&record.to_string());
        jsonl.push('\n');
    }
    Ok(GraphExport { status, jsonl })
}

struct Envelope<'a> {
    build: &'a Build,
    layout: &'a ProjectLayout,
    repo: &'a Repo,
    paths: &'a Paths,
    project: Option<&'a Project>,
    failures: &'a [CompileFailure],
}

impl Envelope<'_> {
    fn render(&self, status: Status, truncation: Value, counts: Value) -> String {
        let (layout, paths) = (self.layout, self.paths);
        json!({
            "kind": "envelope",
            "schema_version": SCHEMA_VERSION,
            "tool": {
                "name": "ilold",
                "version": env!("CARGO_PKG_VERSION"),
                "git_commit": self.build.git_commit,
                "dirty": self.build.dirty,
            },
            "target": {
                "git_commit": self.repo.commit,
                "dirty": self.repo.dirty,
                "sources_sha256": inventory::sources_sha256(self.project, paths),
            },
            "project_root": paths.relative(&layout.root),
            "solc_versions": self.project.map(|p| p.solc_versions.as_slice()).unwrap_or_default(),
            "profile": layout.profile,
            "remappings_sha256": inventory::remappings_sha256(layout),
            "status": status.label(),
            "files": inventory::files(layout, paths, self.project, self.failures),
            "warnings": inventory::warnings(layout, paths, self.failures, status == Status::CompleteWithWarnings),
            "truncation": truncation,
            "counts": counts,
        })
        .to_string()
    }
}
