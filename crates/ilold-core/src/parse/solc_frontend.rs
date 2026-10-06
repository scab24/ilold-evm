use std::path::{Path, PathBuf};

use foundry_compilers::solc::{Solc, SolcCompiler, SolcLanguage, SolcSettings};
use foundry_compilers::{ProjectBuilder, ProjectPathsConfig};
use foundry_compilers_artifacts_solc::ast::{Node, NodeType};
use foundry_compilers_artifacts_solc::{EvmVersion, Remapping, Settings};
use toml_edit::{DocumentMut, Item};

use crate::model::common::*;
use crate::model::contract::*;
use crate::model::decl_id::{DeclId, DeclTable};
use crate::model::expression::*;
use crate::model::function::*;
use crate::model::modifier::*;
use crate::model::project::*;
use crate::model::statement::*;

use super::cache;
use super::error::ParseError;
use super::span::LineIndex;
use super::{ProjectKey, ProjectParser};

pub struct SolcFrontend;

impl ProjectParser for SolcFrontend {
    fn parse(&self, paths: &[PathBuf]) -> Result<Project, ParseError> {
        let root = project_root(paths)?;
        self.parse_project(&root)
    }
}

impl SolcFrontend {
    pub fn parse_project(&self, root: &Path) -> Result<Project, ParseError> {
        self.parse_project_keyed(root).map(|(project, _)| project)
    }

    /// Like `parse`, also returning the validated project cache key when one exists.
    pub fn parse_keyed(&self, paths: &[PathBuf]) -> Result<(Project, Option<ProjectKey>), ParseError> {
        let root = project_root(paths)?;
        self.parse_project_keyed(&root)
    }

    pub fn parse_project_keyed(&self, root: &Path) -> Result<(Project, Option<ProjectKey>), ParseError> {
        let prepared = prepare(root)?;
        let store = cache::Store::from_env();
        let key = prepared.key();
        let canonical = prepared.canonical.clone();
        let (project, validated) = cache::reuse_or_compile(store.as_ref().ok(), canonical.as_deref(),
            key.as_deref(), || prepare(root).ok().and_then(|p| p.key()) == key,
            || self.compile_prepared(root, prepared))?;
        let key = match (validated, canonical, key) {
            (true, Some(root), Some(key)) => Some(ProjectKey { root, key }),
            _ => None,
        };
        Ok((project, key))
    }

    fn compile_prepared(&self, root: &Path, prepared: Prepared) -> Result<Project, ParseError> {
        let src_dir = prepared.paths.sources.clone();
        let project = ProjectBuilder::<SolcCompiler>::default()
            .paths(prepared.paths)
            .settings(prepared.settings)
            .set_cached(false)
            .build(solc_compiler(&prepared.config)?)
            .map_err(|e| ParseError::Internal {
                message: format!("solc project build: {e}"),
            })?;

        let output = project.compile().map_err(|e| ParseError::Internal {
            message: format!("solc compile: {e}"),
        })?;

        if output.has_compiler_errors() {
            let message = output
                .output()
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ParseError::SyntaxError {
                path: root.display().to_string(),
                message,
                span: None,
            });
        }

        let mut source_files = Vec::new();
        let mut contracts = Vec::new();
        let mut decl_table = DeclTable::default();

        let aggregated = output.into_output();
        for (path, source_file) in aggregated.sources.sources() {
            let Some(ast) = &source_file.ast else { continue };
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let file_index = source_files.len();
            let index = LineIndex::new(file_index, &content);
            let path_str = path.display().to_string();
            let vendored = ["/dependencies/", "/deployments/", "/config-engine/"]
                .iter()
                .any(|seg| path_str.contains(seg));
            let listed = path.starts_with(&src_dir) && !vendored;

            for node in &ast.nodes {
                if node.node_type == NodeType::ContractDefinition {
                    if let Some(contract) = map_contract(node, &index, &mut decl_table) {
                        if listed {
                            contracts.push(contract);
                        }
                    }
                }
            }

            source_files.push(SourceFile { path: path_str, content });
        }

        let mut project = Project {
            source_files,
            contracts,
            contract_index: Default::default(),
            decl_table,
        };
        project.rebuild_index();
        Ok(project)
    }
}

struct Prepared {
    config: FoundryConfig,
    paths: ProjectPathsConfig<SolcLanguage>,
    settings: SolcSettings,
    canonical: Option<PathBuf>,
    profile: String,
    installed: Option<Solc>,
}
impl Prepared {
    fn key(&self) -> Option<String> {
        cache::fingerprint(self.canonical.as_ref()?, &self.paths, &self.settings,
            &self.profile, self.installed.as_ref()?).map_err(|reason| {
                eprintln!("ilold project cache bypass: {reason}");
            }).ok()
    }
}
fn prepare(root: &Path) -> Result<Prepared, ParseError> {
    let profile = selected_profile();
    let config = read_foundry_config_with_profile(root, &profile)?;
    let paths = project_paths(root, &config)?;
    let settings = SolcSettings { settings: solc_settings(&config)?, ..Default::default() };
    let canonical = root.canonicalize().ok();
    let installed = installed_compiler(&config);
    Ok(Prepared { config, paths, settings, canonical, profile, installed })
}

fn project_root(paths: &[PathBuf]) -> Result<PathBuf, ParseError> {
    let first = paths.first().ok_or_else(|| ParseError::Internal {
        message: "no input paths".into(),
    })?;
    if !first.exists() {
        return Err(ParseError::FileNotFound {
            path: first.display().to_string(),
        });
    }
    let start = if first.is_dir() {
        first.clone()
    } else {
        first.parent().map(Path::to_path_buf).unwrap_or_default()
    };

    let mut dir = start.clone();
    loop {
        if dir.join("foundry.toml").exists() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    Ok(start)
}

#[derive(Debug, Default)]
struct FoundryConfig {
    src: Option<String>,
    out: Option<String>,
    test: Option<String>,
    script: Option<String>,
    libs: Option<Vec<String>>,
    solc: Option<String>,
    evm_version: Option<String>,
    optimizer: Option<bool>,
    optimizer_runs: Option<usize>,
    via_ir: Option<bool>,
    auto_detect_remappings: Option<bool>,
    remappings: Vec<Remapping>,
}

impl FoundryConfig {
    fn auto_detect_remappings(&self) -> bool {
        self.auto_detect_remappings.unwrap_or(true)
    }

    fn apply_profile(&mut self, root: &Path, profile: &Item) {
        if let Some(value) = string_value(profile, "src") {
            self.src = Some(value);
        }
        if let Some(value) = string_value(profile, "out") {
            self.out = Some(value);
        }
        if let Some(value) = string_value(profile, "test") {
            self.test = Some(value);
        }
        if let Some(value) = string_value(profile, "script") {
            self.script = Some(value);
        }
        if let Some(value) = string_array_value(profile, "libs") {
            self.libs = Some(value);
        }
        if let Some(value) = string_value(profile, "solc").or_else(|| string_value(profile, "solc_version")) {
            self.solc = Some(value);
        }
        if let Some(value) = string_value(profile, "evm_version") {
            self.evm_version = Some(value);
        }
        if let Some(value) = bool_value(profile, "optimizer") {
            self.optimizer = Some(value);
        }
        if let Some(value) = usize_value(profile, "optimizer_runs") {
            self.optimizer_runs = Some(value);
        }
        if let Some(value) = bool_value(profile, "via_ir") {
            self.via_ir = Some(value);
        }
        if let Some(value) = bool_value(profile, "auto_detect_remappings") {
            self.auto_detect_remappings = Some(value);
        }
        if let Some(raw_remappings) = string_array_value(profile, "remappings") {
            self.remappings = raw_remappings
                .iter()
                .filter_map(|raw| parse_remapping(root, raw))
                .collect();
        }
    }
}

fn project_paths(root: &Path, cfg: &FoundryConfig) -> Result<ProjectPathsConfig<SolcLanguage>, ParseError> {
    let mut builder = ProjectPathsConfig::builder().root(root);

    if let Some(src) = &cfg.src {
        builder = builder.sources(rooted_path(root, src));
    }
    if let Some(out) = &cfg.out {
        builder = builder.artifacts(rooted_path(root, out));
    }
    if let Some(test) = &cfg.test {
        builder = builder.tests(rooted_path(root, test));
    }
    if let Some(script) = &cfg.script {
        builder = builder.scripts(rooted_path(root, script));
    }
    if let Some(libs) = &cfg.libs {
        if libs.is_empty() {
            builder = builder.no_libs();
        } else {
            builder = builder.libs(libs.iter().map(|lib| rooted_path(root, lib)));
        }
    }

    let mut remappings = cfg.remappings.clone();
    remappings.extend(read_remappings_file(root));
    if cfg.auto_detect_remappings() {
        let lib_paths = cfg
            .libs
            .as_ref()
            .map(|libs| libs.iter().map(|lib| rooted_path(root, lib)).collect::<Vec<_>>())
            .unwrap_or_else(|| ProjectPathsConfig::find_libs(root));
        for detected in lib_paths
            .iter()
            .flat_map(|path| Remapping::find_many(path.as_path()))
        {
            if !remappings.iter().any(|existing| {
                existing.name == detected.name && existing.context == detected.context
            }) {
                remappings.push(detected);
            }
        }
    }
    builder = builder.remappings(remappings);

    builder.build().map_err(|e| ParseError::Internal {
        message: format!("solc paths config: {e}"),
    })
}

fn solc_settings(cfg: &FoundryConfig) -> Result<Settings, ParseError> {
    let mut settings = Settings::default().with_ast();
    if let Some(via_ir) = cfg.via_ir {
        settings = settings.set_via_ir(via_ir);
    }
    if let Some(optimizer) = cfg.optimizer {
        settings.optimizer.enabled = Some(optimizer);
    }
    if let Some(runs) = cfg.optimizer_runs {
        settings.optimizer.runs(runs);
    }
    if let Some(evm_version) = &cfg.evm_version {
        settings.evm_version = Some(evm_version.parse::<EvmVersion>().map_err(|e| ParseError::Internal {
            message: format!("invalid evm_version `{evm_version}` in foundry.toml: {e}"),
        })?);
    }
    Ok(settings)
}

fn solc_compiler(cfg: &FoundryConfig) -> Result<SolcCompiler, ParseError> {
    let Some(solc_version) = &cfg.solc else {
        return Ok(SolcCompiler::AutoDetect);
    };
    let version_text = solc_version.trim_start_matches('v');
    let version = version_text.parse().map_err(|e| ParseError::Internal {
        message: format!("invalid solc version `{solc_version}` in foundry.toml: {e}"),
    })?;
    let solc = Solc::find_or_install(&version).map_err(|e| ParseError::Internal {
        message: format!("install/find solc {solc_version}: {e}"),
    })?;
    Ok(SolcCompiler::Specific(solc))
}

fn selected_profile() -> String {
    std::env::var("FOUNDRY_PROFILE").ok().filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
fn read_foundry_config(root: &Path) -> Result<FoundryConfig, ParseError> {
    read_foundry_config_with_profile(root, &selected_profile())
}

fn installed_compiler(cfg: &FoundryConfig) -> Option<Solc> {
    let version = cfg.solc.as_ref()?.trim_start_matches('v').parse().ok()?;
    Solc::find_svm_installed_version(&version).ok().flatten()
}

fn read_foundry_config_with_profile(root: &Path, profile_name: &str) -> Result<FoundryConfig, ParseError> {
    let text = match std::fs::read_to_string(root.join("foundry.toml")) {
        Ok(t) => t,
        Err(_) => return Ok(FoundryConfig::default()),
    };
    let doc = text.parse::<DocumentMut>().map_err(|e| ParseError::Internal {
        message: format!("parse foundry.toml: {e}"),
    })?;

    let mut cfg = FoundryConfig::default();
    if let Some(default_profile) = profile_item(&doc, "default") {
        cfg.apply_profile(root, &default_profile);
    }
    if profile_name != "default" {
        if let Some(selected_profile) = profile_item(&doc, profile_name) {
            cfg.apply_profile(root, &selected_profile);
        }
    }

    Ok(cfg)
}

fn profile_item(doc: &DocumentMut, profile: &str) -> Option<Item> {
    doc.get("profile")?.get(profile).cloned()
}

fn string_value(item: &Item, key: &str) -> Option<String> {
    item.get(key)?.as_str().map(ToOwned::to_owned)
}

fn bool_value(item: &Item, key: &str) -> Option<bool> {
    item.get(key)?.as_bool()
}

fn usize_value(item: &Item, key: &str) -> Option<usize> {
    item.get(key)?.as_integer().and_then(|value| value.try_into().ok())
}

fn string_array_value(item: &Item, key: &str) -> Option<Vec<String>> {
    let array = item.get(key)?.as_array()?;
    Some(array.iter().filter_map(|value| value.as_str().map(ToOwned::to_owned)).collect())
}

fn read_remappings_file(root: &Path) -> Vec<Remapping> {
    let text = match std::fs::read_to_string(root.join("remappings.txt")) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| parse_remapping(root, line))
        .collect()
}

fn parse_remapping(root: &Path, text: &str) -> Option<Remapping> {
    let mut remapping = text.parse::<Remapping>().ok()?;
    if Path::new(&remapping.path).is_relative() {
        remapping.path = rooted_path(root, &remapping.path).to_string_lossy().into_owned();
    }
    Some(remapping)
}

fn rooted_path(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn map_contract(node: &Node, index: &LineIndex, decl_table: &mut DeclTable) -> Option<ContractDef> {
    let name: String = node.attribute("name")?;

    if let Some(id) = node.id {
        decl_table.insert(DeclId(id as isize), name.clone(), String::new());
    }

    let mut functions = Vec::new();
    let mut modifiers = Vec::new();
    let mut state_vars = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut events = Vec::new();
    let mut errors = Vec::new();

    for child in &node.nodes {
        match child.node_type {
            NodeType::FunctionDefinition => {
                let func = map_function(child, index);
                if let Some(id) = child.id {
                    decl_table.insert(DeclId(id as isize), name.clone(), func.name.clone());
                }
                functions.push(func);
            }
            NodeType::ModifierDefinition => modifiers.push(map_modifier(child, index)),
            NodeType::VariableDeclaration => {
                if child.attribute::<bool>("stateVariable").unwrap_or(false) {
                    state_vars.push(map_state_var(child, index));
                }
            }
            NodeType::StructDefinition => structs.push(map_struct(child, index)),
            NodeType::EnumDefinition => enums.push(map_enum(child, index)),
            NodeType::EventDefinition => events.push(map_event(child, index)),
            NodeType::ErrorDefinition => errors.push(map_error(child, index)),
            _ => {}
        }
    }

    Some(ContractDef {
        name,
        kind: contract_kind(node),
        functions,
        modifiers,
        state_vars,
        structs,
        enums,
        events,
        errors,
        inherits: base_contracts(node),
        span: span_of(node, index),
    })
}

fn map_function(node: &Node, index: &LineIndex) -> FunctionDef {
    let kind = function_kind(node);
    FunctionDef {
        name: function_name(node, kind),
        kind,
        visibility: visibility(node),
        mutability: mutability(node),
        modifiers: modifier_refs(node),
        params: param_list(node, "parameters"),
        returns: param_list(node, "returnParameters"),
        body: node.body.as_deref().map(|b| map_body(b, index)),
        is_virtual: node.attribute::<bool>("virtual").unwrap_or(false),
        is_override: node.attribute::<serde_json::Value>("overrides").is_some(),
        span: span_of(node, index),
    }
}

fn map_modifier(node: &Node, index: &LineIndex) -> ModifierDef {
    ModifierDef {
        name: node.attribute::<String>("name").unwrap_or_default(),
        params: param_list(node, "parameters"),
        body: node.body.as_deref().map(|b| map_body(b, index)).unwrap_or_default(),
        span: span_of(node, index),
    }
}

fn map_state_var(node: &Node, index: &LineIndex) -> StateVar {
    StateVar {
        name: node.attribute::<String>("name").unwrap_or_default(),
        type_name: type_string(node),
        visibility: visibility(node),
        is_constant: node.attribute::<bool>("constant").unwrap_or(false),
        is_immutable: node.attribute::<String>("mutability").as_deref() == Some("immutable"),
        initial_value: node.attribute::<Node>("value").map(|v| map_expression(&v, index)),
        resolved_types: type_decl_ids(node),
        span: span_of(node, index),
    }
}

fn type_decl_ids(node: &Node) -> Vec<DeclId> {
    let mut out = Vec::new();
    if let Some(type_name) = node.attribute::<serde_json::Value>("typeName") {
        collect_referenced_decls(&type_name, &mut out);
    }
    out
}

fn collect_referenced_decls(value: &serde_json::Value, out: &mut Vec<DeclId>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(id) = map.get("referencedDeclaration").and_then(|v| v.as_i64()) {
                if id >= 0 {
                    let id = DeclId(id as isize);
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
            for child in map.values() {
                collect_referenced_decls(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_referenced_decls(child, out);
            }
        }
        _ => {}
    }
}

fn map_struct(node: &Node, index: &LineIndex) -> StructDef {
    let fields = node
        .attribute::<Vec<Node>>("members")
        .unwrap_or_default()
        .iter()
        .map(|m| Param {
            name: m.attribute::<String>("name").unwrap_or_default(),
            type_name: type_string(m),
        })
        .collect();
    StructDef {
        name: node.attribute::<String>("name").unwrap_or_default(),
        fields,
        span: span_of(node, index),
    }
}

fn map_enum(node: &Node, index: &LineIndex) -> EnumDef {
    let variants = node
        .attribute::<Vec<Node>>("members")
        .unwrap_or_default()
        .iter()
        .map(|m| m.attribute::<String>("name").unwrap_or_default())
        .collect();
    EnumDef {
        name: node.attribute::<String>("name").unwrap_or_default(),
        variants,
        span: span_of(node, index),
    }
}

fn map_event(node: &Node, index: &LineIndex) -> EventDef {
    EventDef {
        name: node.attribute::<String>("name").unwrap_or_default(),
        params: param_list(node, "parameters"),
        span: span_of(node, index),
    }
}

fn map_error(node: &Node, index: &LineIndex) -> ErrorDef {
    ErrorDef {
        name: node.attribute::<String>("name").unwrap_or_default(),
        params: param_list(node, "parameters"),
        span: span_of(node, index),
    }
}

fn param_list(node: &Node, key: &str) -> Vec<Param> {
    node.attribute::<Node>(key)
        .map(|list| params_from_list(&list))
        .unwrap_or_default()
}

fn params_from_list(list: &Node) -> Vec<Param> {
    list.attribute::<Vec<Node>>("parameters")
        .unwrap_or_default()
        .iter()
        .map(|p| Param {
            name: p.attribute::<String>("name").unwrap_or_default(),
            type_name: type_string(p),
        })
        .collect()
}

fn map_block(node: &Node, index: &LineIndex) -> Vec<Statement> {
    node.attribute::<Vec<Node>>("statements")
        .unwrap_or_default()
        .iter()
        .map(|s| map_statement(s, index))
        .collect()
}

fn map_body(node: &Node, index: &LineIndex) -> Vec<Statement> {
    match node.node_type {
        NodeType::Block | NodeType::UncheckedBlock => map_block(node, index),
        _ => vec![map_statement(node, index)],
    }
}

fn map_statement(node: &Node, index: &LineIndex) -> Statement {
    let kind = match node.node_type {
        NodeType::Block => StatementKind::Block { statements: map_block(node, index) },
        NodeType::UncheckedBlock => StatementKind::UncheckedBlock { statements: map_block(node, index) },
        NodeType::IfStatement => StatementKind::If {
            condition: child_expr(node, "condition", index),
            then_body: node
                .attribute::<Node>("trueBody")
                .map(|b| map_body(&b, index))
                .unwrap_or_default(),
            else_body: node.attribute::<Node>("falseBody").map(|b| map_body(&b, index)),
        },
        NodeType::ForStatement => StatementKind::For {
            init: node
                .attribute::<Node>("initializationExpression")
                .map(|s| Box::new(map_statement(&s, index))),
            condition: node.attribute::<Node>("condition").map(|c| map_expression(&c, index)),
            increment: node
                .attribute::<Node>("loopExpression")
                .and_then(|s| s.attribute::<Node>("expression"))
                .map(|e| map_expression(&e, index)),
            body: node.body.as_deref().map(|b| map_body(b, index)).unwrap_or_default(),
        },
        NodeType::WhileStatement => StatementKind::While {
            condition: child_expr(node, "condition", index),
            body: node.body.as_deref().map(|b| map_body(b, index)).unwrap_or_default(),
        },
        NodeType::DoWhileStatement => StatementKind::DoWhile {
            body: node.body.as_deref().map(|b| map_body(b, index)).unwrap_or_default(),
            condition: child_expr(node, "condition", index),
        },
        NodeType::Return => StatementKind::Return {
            value: node.attribute::<Node>("expression").map(|e| map_expression(&e, index)),
        },
        NodeType::EmitStatement => {
            let (name, arguments) = call_parts(node, "eventCall", index);
            StatementKind::Emit { event_name: name, arguments }
        }
        NodeType::RevertStatement => {
            let (name, arguments) = call_parts(node, "errorCall", index);
            StatementKind::Revert { error_name: Some(name), arguments }
        }
        NodeType::ExpressionStatement => StatementKind::ExpressionStmt {
            expression: child_expr(node, "expression", index),
        },
        NodeType::VariableDeclarationStatement => map_var_decl(node, index),
        NodeType::TryStatement => StatementKind::TryCatch {
            expression: child_expr(node, "externalCall", index),
            clauses: node
                .attribute::<Vec<Node>>("clauses")
                .unwrap_or_default()
                .iter()
                .map(|c| map_catch(c, index))
                .collect(),
        },
        NodeType::InlineAssembly => StatementKind::Assembly { span: span_of(node, index) },
        NodeType::PlaceholderStatement => StatementKind::Placeholder,
        NodeType::Break => StatementKind::Break,
        NodeType::Continue => StatementKind::Continue,
        _ => StatementKind::ExpressionStmt {
            expression: placeholder_expr(node, index),
        },
    };
    Statement { kind, span: span_of(node, index) }
}

fn map_var_decl(node: &Node, index: &LineIndex) -> StatementKind {
    let declarations = node.attribute::<Vec<serde_json::Value>>("declarations").unwrap_or_default();
    let initial_value = node.attribute::<Node>("initialValue").map(|e| map_expression(&e, index));

    let decl_name = |d: &serde_json::Value| {
        d.get("name").and_then(|s| s.as_str()).unwrap_or("_").to_string()
    };

    if declarations.len() == 1 {
        let d = &declarations[0];
        let type_name = d
            .get("typeDescriptions")
            .and_then(|t| t.get("typeString"))
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        let is_storage_ref = d.get("storageLocation").and_then(|s| s.as_str()) == Some("storage");
        return StatementKind::VariableDeclaration {
            name: decl_name(d),
            type_name,
            initial_value,
            is_storage_ref,
        };
    }

    let names: Vec<String> = declarations.iter().map(decl_name).collect();
    StatementKind::VariableDeclaration {
        name: format!("({})", names.join(", ")),
        type_name: "tuple".into(),
        initial_value,
        is_storage_ref: false,
    }
}

fn map_catch(node: &Node, index: &LineIndex) -> CatchClause {
    CatchClause {
        name: node.attribute::<String>("errorName").filter(|s| !s.is_empty()),
        params: node
            .attribute::<Node>("parameters")
            .map(|p| params_from_list(&p))
            .unwrap_or_default(),
        body: node
            .attribute::<Node>("block")
            .map(|b| map_block(&b, index))
            .unwrap_or_default(),
    }
}

fn call_parts(node: &Node, key: &str, index: &LineIndex) -> (String, Vec<Expression>) {
    let Some(call) = node.attribute::<Node>(key) else {
        return (String::new(), Vec::new());
    };
    let name = call
        .attribute::<Node>("expression")
        .map(|c| callee_name(&c))
        .unwrap_or_default();
    let arguments = call
        .attribute::<Vec<Node>>("arguments")
        .unwrap_or_default()
        .iter()
        .map(|a| map_expression(a, index))
        .collect();
    (name, arguments)
}

fn callee_name(node: &Node) -> String {
    node.attribute::<String>("name")
        .or_else(|| node.attribute::<String>("memberName"))
        .unwrap_or_default()
}

fn map_expression(node: &Node, index: &LineIndex) -> Expression {
    let kind = match node.node_type {
        NodeType::FunctionCall => map_call(node, index),
        NodeType::FunctionCallOptions => child_expr(node, "expression", index).kind,
        NodeType::MemberAccess => ExpressionKind::MemberAccess {
            object: Box::new(child_expr(node, "expression", index)),
            member: node.attribute::<String>("memberName").unwrap_or_default(),
            resolved: referenced_decl(node),
        },
        NodeType::Identifier => ExpressionKind::Identifier {
            name: node.attribute::<String>("name").unwrap_or_default(),
            resolved: referenced_decl(node),
        },
        NodeType::Assignment => ExpressionKind::Assignment {
            target: Box::new(child_expr(node, "leftHandSide", index)),
            operator: assign_op(node),
            value: Box::new(child_expr(node, "rightHandSide", index)),
        },
        NodeType::BinaryOperation => ExpressionKind::BinaryOp {
            left: Box::new(child_expr(node, "leftExpression", index)),
            operator: binary_op(node),
            right: Box::new(child_expr(node, "rightExpression", index)),
        },
        NodeType::UnaryOperation => ExpressionKind::UnaryOp {
            operator: unary_op(node),
            operand: Box::new(child_expr(node, "subExpression", index)),
        },
        NodeType::IndexAccess => ExpressionKind::IndexAccess {
            base: Box::new(child_expr(node, "baseExpression", index)),
            index: node
                .attribute::<Node>("indexExpression")
                .map(|n| Box::new(map_expression(&n, index))),
        },
        NodeType::Literal => ExpressionKind::Literal {
            value: node.attribute::<String>("value").unwrap_or_default(),
            literal_type: literal_type(node),
        },
        NodeType::Conditional => ExpressionKind::Ternary {
            condition: Box::new(child_expr(node, "condition", index)),
            true_expr: Box::new(child_expr(node, "trueExpression", index)),
            false_expr: Box::new(child_expr(node, "falseExpression", index)),
        },
        NodeType::TupleExpression => ExpressionKind::Tuple {
            elements: node
                .attribute::<Vec<Option<Node>>>("components")
                .unwrap_or_default()
                .iter()
                .map(|c| c.as_ref().map(|n| map_expression(n, index)))
                .collect(),
        },
        NodeType::IndexRangeAccess => ExpressionKind::IndexRange {
            base: Box::new(child_expr(node, "baseExpression", index)),
            start: node
                .attribute::<Node>("startExpression")
                .map(|n| Box::new(map_expression(&n, index))),
            end: node
                .attribute::<Node>("endExpression")
                .map(|n| Box::new(map_expression(&n, index))),
        },
        _ => placeholder_expr(node, index).kind,
    };
    Expression { kind, span: span_of(node, index) }
}

fn map_call(node: &Node, index: &LineIndex) -> ExpressionKind {
    let arguments: Vec<Expression> = node
        .attribute::<Vec<Node>>("arguments")
        .unwrap_or_default()
        .iter()
        .map(|a| map_expression(a, index))
        .collect();
    let callee = node.attribute::<Node>("expression");

    if node.attribute::<String>("kind").as_deref() == Some("typeConversion") {
        let type_name = callee.as_ref().map(type_expr_name).unwrap_or_default();
        let expression = arguments
            .into_iter()
            .next()
            .map(Box::new)
            .unwrap_or_else(|| Box::new(empty_expr(node, index)));
        return ExpressionKind::TypeCast { type_name, expression };
    }

    if node.attribute::<String>("kind").as_deref() == Some("structConstructorCall") {
        return ExpressionKind::Tuple {
            elements: arguments.into_iter().map(Some).collect(),
        };
    }

    if let Some(callee) = &callee {
        if callee.node_type == NodeType::NewExpression {
            return ExpressionKind::New { type_name: type_expr_name(callee), arguments };
        }
    }

    ExpressionKind::FunctionCall {
        callee: Box::new(
            callee
                .map(|c| map_expression(&c, index))
                .unwrap_or_else(|| empty_expr(node, index)),
        ),
        arguments,
    }
}

fn child_expr(node: &Node, key: &str, index: &LineIndex) -> Expression {
    node.attribute::<Node>(key)
        .map(|c| map_expression(&c, index))
        .unwrap_or_else(|| empty_expr(node, index))
}

fn empty_expr(node: &Node, index: &LineIndex) -> Expression {
    Expression {
        kind: ExpressionKind::Identifier { name: String::new(), resolved: None },
        span: span_of(node, index),
    }
}

fn placeholder_expr(node: &Node, index: &LineIndex) -> Expression {
    Expression {
        kind: ExpressionKind::Identifier {
            name: format!("/* {:?} */", node.node_type),
            resolved: None,
        },
        span: span_of(node, index),
    }
}

fn type_expr_name(node: &Node) -> String {
    node.attribute::<String>("name")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| type_string(node))
}

// solc marks builtins with negative ids
fn referenced_decl(node: &Node) -> Option<DeclId> {
    node.attribute::<isize>("referencedDeclaration")
        .filter(|&id| id >= 0)
        .map(DeclId)
}

fn binary_op(node: &Node) -> BinaryOperator {
    match node.attribute::<String>("operator").as_deref().unwrap_or("") {
        "+" => BinaryOperator::Add,
        "-" => BinaryOperator::Sub,
        "*" => BinaryOperator::Mul,
        "/" => BinaryOperator::Div,
        "%" => BinaryOperator::Mod,
        "**" => BinaryOperator::Pow,
        "==" => BinaryOperator::Eq,
        "!=" => BinaryOperator::Neq,
        "<" => BinaryOperator::Lt,
        ">" => BinaryOperator::Gt,
        "<=" => BinaryOperator::Lte,
        ">=" => BinaryOperator::Gte,
        "&&" => BinaryOperator::And,
        "||" => BinaryOperator::Or,
        "&" => BinaryOperator::BitAnd,
        "|" => BinaryOperator::BitOr,
        "^" => BinaryOperator::BitXor,
        "<<" => BinaryOperator::Shl,
        ">>" => BinaryOperator::Shr,
        _ => BinaryOperator::Add,
    }
}

fn assign_op(node: &Node) -> AssignOperator {
    match node.attribute::<String>("operator").as_deref().unwrap_or("=") {
        "=" => AssignOperator::Assign,
        "+=" => AssignOperator::AddAssign,
        "-=" => AssignOperator::SubAssign,
        "*=" => AssignOperator::MulAssign,
        "/=" => AssignOperator::DivAssign,
        "%=" => AssignOperator::ModAssign,
        "&=" => AssignOperator::BitAndAssign,
        "|=" => AssignOperator::BitOrAssign,
        "^=" => AssignOperator::BitXorAssign,
        "<<=" => AssignOperator::ShlAssign,
        ">>=" => AssignOperator::ShrAssign,
        _ => AssignOperator::Assign,
    }
}

fn unary_op(node: &Node) -> UnaryOperator {
    let prefix = node.attribute::<bool>("prefix").unwrap_or(true);
    match node.attribute::<String>("operator").as_deref().unwrap_or("") {
        "!" => UnaryOperator::Not,
        "-" => UnaryOperator::Neg,
        "~" => UnaryOperator::BitNot,
        "++" if prefix => UnaryOperator::PreIncrement,
        "++" => UnaryOperator::PostIncrement,
        "--" if prefix => UnaryOperator::PreDecrement,
        "--" => UnaryOperator::PostDecrement,
        "delete" => UnaryOperator::Delete,
        _ => UnaryOperator::Not,
    }
}

fn literal_type(node: &Node) -> LiteralType {
    match node.attribute::<String>("kind").as_deref().unwrap_or("") {
        "number" => LiteralType::Number,
        "string" => LiteralType::String,
        "bool" => LiteralType::Bool,
        "hexString" => LiteralType::HexString,
        _ => LiteralType::String,
    }
}

fn modifier_refs(node: &Node) -> Vec<ModifierRef> {
    node.attribute::<Vec<serde_json::Value>>("modifiers")
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            m.get("modifierName")
                .and_then(|n| n.get("name"))
                .and_then(|s| s.as_str())
                .map(|name| ModifierRef {
                    name: name.to_string(),
                    arguments: Vec::new(),
                })
        })
        .collect()
}

fn base_contracts(node: &Node) -> Vec<String> {
    node.attribute::<Vec<serde_json::Value>>("baseContracts")
        .unwrap_or_default()
        .iter()
        .filter_map(|b| {
            b.get("baseName")
                .and_then(|n| n.get("name"))
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn type_string(node: &Node) -> String {
    node.attribute::<serde_json::Value>("typeDescriptions")
        .and_then(|v| v.get("typeString").and_then(|s| s.as_str().map(str::to_string)))
        .unwrap_or_default()
}

fn contract_kind(node: &Node) -> ContractKind {
    if node.attribute::<bool>("abstract").unwrap_or(false) {
        return ContractKind::Abstract;
    }
    match node.attribute::<String>("contractKind").as_deref() {
        Some("interface") => ContractKind::Interface,
        Some("library") => ContractKind::Library,
        _ => ContractKind::Contract,
    }
}

fn visibility(node: &Node) -> Visibility {
    match node.attribute::<String>("visibility").as_deref() {
        Some("external") => Visibility::External,
        Some("internal") => Visibility::Internal,
        Some("private") => Visibility::Private,
        _ => Visibility::Public,
    }
}

fn mutability(node: &Node) -> Mutability {
    match node.attribute::<String>("stateMutability").as_deref() {
        Some("pure") => Mutability::Pure,
        Some("view") => Mutability::View,
        Some("payable") => Mutability::Payable,
        _ => Mutability::NonPayable,
    }
}

fn function_kind(node: &Node) -> FunctionKind {
    match node.attribute::<String>("kind").as_deref() {
        Some("constructor") => FunctionKind::Constructor,
        Some("fallback") => FunctionKind::Fallback,
        Some("receive") => FunctionKind::Receive,
        _ => FunctionKind::Function,
    }
}

fn function_name(node: &Node, kind: FunctionKind) -> String {
    match node.attribute::<String>("name") {
        Some(n) if !n.is_empty() => n,
        _ => match kind {
            FunctionKind::Constructor => "constructor".to_string(),
            FunctionKind::Receive => "receive".to_string(),
            FunctionKind::Fallback => "fallback".to_string(),
            FunctionKind::Function => String::new(),
        },
    }
}

fn span_of(node: &Node, index: &LineIndex) -> SourceSpan {
    let start = node.src.start;
    let end = start + node.src.length.unwrap_or(0);
    index.span(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/cross")
    }

    #[test]
    fn parses_contracts_functions_inheritance() {
        let project = SolcFrontend.parse_project(&fixture_root()).expect("parse fixture");

        let lending = project
            .contracts
            .iter()
            .find(|c| c.name == "LendingPool")
            .expect("LendingPool present");
        assert!(lending.inherits.contains(&"BasePool".to_string()));
        assert!(lending.inherits.contains(&"IPool".to_string()));
        assert!(lending.functions.iter().any(|f| f.name == "supply"));

        let vault = project
            .contracts
            .iter()
            .find(|c| c.name == "Vault")
            .expect("Vault present");
        assert!(vault.state_vars.iter().any(|sv| sv.name == "pool"));
    }

    #[test]
    fn lowers_statements() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/statements");
        let project = SolcFrontend.parse_project(&root).expect("parse statements");
        let stmts = project
            .contracts
            .iter()
            .find(|c| c.name == "Stmts")
            .expect("Stmts contract");
        let run = stmts.functions.iter().find(|f| f.name == "run").expect("run fn");
        let body = run.body.as_ref().expect("run has a body");

        use StatementKind as K;
        assert!(body.iter().any(|s| matches!(s.kind, K::If { .. })), "if");
        assert!(body.iter().any(|s| matches!(s.kind, K::For { .. })), "for");
        assert!(body.iter().any(|s| matches!(s.kind, K::While { .. })), "while");
        assert!(body.iter().any(|s| matches!(s.kind, K::DoWhile { .. })), "do-while");
        assert!(body.iter().any(|s| matches!(s.kind, K::UncheckedBlock { .. })), "unchecked");
        assert!(body.iter().any(|s| matches!(s.kind, K::TryCatch { .. })), "try");
        assert!(body.iter().any(|s| matches!(s.kind, K::Emit { .. })), "emit");
        assert!(body.iter().any(|s| matches!(s.kind, K::Return { .. })), "return");
        assert!(body.iter().any(|s| matches!(s.kind, K::Assembly { .. })), "assembly");
        assert!(
            body.iter().any(|s| matches!(s.kind, K::VariableDeclaration { .. })),
            "var decl"
        );

        let for_body = body
            .iter()
            .find_map(|s| match &s.kind {
                K::For { body, .. } => Some(body),
                _ => None,
            })
            .expect("for present");
        assert!(
            for_body.iter().any(|s| matches!(s.kind, K::ExpressionStmt { .. })),
            "for body lost its statements"
        );
        let while_body = body
            .iter()
            .find_map(|s| match &s.kind {
                K::While { body, .. } => Some(body),
                _ => None,
            })
            .expect("while present");
        assert!(!while_body.is_empty(), "while body lost its statements");
        let do_body = body
            .iter()
            .find_map(|s| match &s.kind {
                K::DoWhile { body, .. } => Some(body),
                _ => None,
            })
            .expect("do-while present");
        assert!(!do_body.is_empty(), "do-while body lost its statements");

        let gated = stmts
            .modifiers
            .iter()
            .find(|m| m.name == "gated")
            .expect("gated modifier");
        assert!(
            gated.body.iter().any(|s| matches!(s.kind, K::Placeholder)),
            "placeholder _"
        );
    }

    fn count_expr(e: &Expression, calls: &mut usize, placeholders: &mut usize, resolved_members: &mut usize) {
        match &e.kind {
            ExpressionKind::Identifier { name, .. } if name.starts_with("/*") => *placeholders += 1,
            ExpressionKind::MemberAccess { object, resolved, .. } => {
                if resolved.is_some() {
                    *resolved_members += 1;
                }
                count_expr(object, calls, placeholders, resolved_members);
            }
            ExpressionKind::FunctionCall { callee, arguments } => {
                *calls += 1;
                count_expr(callee, calls, placeholders, resolved_members);
                for a in arguments {
                    count_expr(a, calls, placeholders, resolved_members);
                }
            }
            ExpressionKind::BinaryOp { left, right, .. } => {
                count_expr(left, calls, placeholders, resolved_members);
                count_expr(right, calls, placeholders, resolved_members);
            }
            ExpressionKind::UnaryOp { operand, .. } => count_expr(operand, calls, placeholders, resolved_members),
            ExpressionKind::Assignment { target, value, .. } => {
                count_expr(target, calls, placeholders, resolved_members);
                count_expr(value, calls, placeholders, resolved_members);
            }
            ExpressionKind::IndexAccess { base, index } => {
                count_expr(base, calls, placeholders, resolved_members);
                if let Some(i) = index {
                    count_expr(i, calls, placeholders, resolved_members);
                }
            }
            ExpressionKind::Ternary { condition, true_expr, false_expr } => {
                count_expr(condition, calls, placeholders, resolved_members);
                count_expr(true_expr, calls, placeholders, resolved_members);
                count_expr(false_expr, calls, placeholders, resolved_members);
            }
            ExpressionKind::TypeCast { expression, .. } => count_expr(expression, calls, placeholders, resolved_members),
            ExpressionKind::New { arguments, .. } => {
                for a in arguments {
                    count_expr(a, calls, placeholders, resolved_members);
                }
            }
            _ => {}
        }
    }

    fn count_stmt(s: &Statement, calls: &mut usize, placeholders: &mut usize, resolved_members: &mut usize) {
        use StatementKind as K;
        let exprs: Vec<&Expression> = match &s.kind {
            K::If { condition, .. } | K::While { condition, .. } | K::DoWhile { condition, .. } => vec![condition],
            K::Return { value: Some(v) } => vec![v],
            K::ExpressionStmt { expression } => vec![expression],
            K::VariableDeclaration { initial_value: Some(v), .. } => vec![v],
            K::TryCatch { expression, .. } => vec![expression],
            _ => vec![],
        };
        for e in exprs {
            count_expr(e, calls, placeholders, resolved_members);
        }
        for child in stmt_children(&s.kind) {
            count_stmt(child, calls, placeholders, resolved_members);
        }
        for arg in stmt_call_args(&s.kind) {
            count_expr(arg, calls, placeholders, resolved_members);
        }
    }

    fn stmt_children(k: &StatementKind) -> Vec<&Statement> {
        use StatementKind as K;
        match k {
            K::Block { statements } | K::UncheckedBlock { statements } => statements.iter().collect(),
            K::If { then_body, else_body, .. } => {
                then_body.iter().chain(else_body.iter().flatten()).collect()
            }
            K::For { body, init, .. } => body.iter().chain(init.iter().map(|b| b.as_ref())).collect(),
            K::While { body, .. } | K::DoWhile { body, .. } => body.iter().collect(),
            _ => vec![],
        }
    }

    fn stmt_call_args(k: &StatementKind) -> Vec<&Expression> {
        use StatementKind as K;
        match k {
            K::Emit { arguments, .. } | K::Revert { arguments, .. } => arguments.iter().collect(),
            _ => vec![],
        }
    }

    #[test]
    fn diagnostic_no_placeholders_and_all_cross_resolved() {
        for fixture in ["cross", "statements"] {
            let root =
                Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/solc/{fixture}"));
            let project = SolcFrontend.parse_project(&root).expect("parse");
            let (mut calls, mut placeholders, mut resolved_members) = (0, 0, 0);
            for c in &project.contracts {
                for f in &c.functions {
                    for s in f.body.iter().flatten() {
                        count_stmt(s, &mut calls, &mut placeholders, &mut resolved_members);
                    }
                }
            }
            println!(
                "[{fixture}] calls={calls} placeholders={placeholders} resolved_members={resolved_members}"
            );
            assert_eq!(placeholders, 0, "fixture {fixture} produced placeholder expressions");
        }
    }

    #[test]
    fn cross_contract_call_carries_resolved() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/cross");
        let project = SolcFrontend.parse_project(&root).expect("parse cross");
        let vault = project
            .contracts
            .iter()
            .find(|c| c.name == "Vault")
            .expect("Vault contract");

        let deposit_via = vault
            .functions
            .iter()
            .find(|f| f.name == "depositVia")
            .expect("depositVia fn");
        let body = deposit_via.body.as_ref().expect("body");
        let ret = body
            .iter()
            .find_map(|s| match &s.kind {
                StatementKind::Return { value: Some(v) } => Some(v),
                _ => None,
            })
            .expect("return statement");

        let resolved = match &ret.kind {
            ExpressionKind::FunctionCall { callee, .. } => match &callee.kind {
                ExpressionKind::MemberAccess { member, resolved, .. } => {
                    assert_eq!(member, "supply");
                    *resolved
                }
                other => panic!("expected member access, got {other:?}"),
            },
            other => panic!("expected function call, got {other:?}"),
        };
        assert!(resolved.is_some(), "pool.supply() must carry a resolved DeclId");
    }

    fn return_value<'a>(f: &'a FunctionDef) -> &'a Expression {
        f.body
            .as_ref()
            .expect("body")
            .iter()
            .find_map(|s| match &s.kind {
                StatementKind::Return { value: Some(v) } => Some(v),
                _ => None,
            })
            .expect("return statement")
    }

    #[test]
    fn foundry_config_reads_profile_settings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/foundry_config");
        let cfg = read_foundry_config(&root).expect("foundry config");

        assert_eq!(cfg.src.as_deref(), Some("src"));
        assert_eq!(cfg.out.as_deref(), Some("out"));
        assert_eq!(cfg.libs.as_deref(), Some(&["lib".to_string()][..]));
        assert_eq!(cfg.solc.as_deref(), Some("0.8.28"));
        assert_eq!(cfg.evm_version.as_deref(), Some("cancun"));
        assert_eq!(cfg.optimizer, Some(true));
        assert_eq!(cfg.optimizer_runs, Some(1));
        assert_eq!(cfg.via_ir, Some(true));
        assert_eq!(cfg.auto_detect_remappings, Some(false));
        assert_eq!(cfg.remappings.len(), 1);
        assert_eq!(cfg.remappings[0].name, "manual/");
    }

    #[test]
    fn foundry_config_selected_profile_inherits_default_settings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/foundry_config");
        let cfg = read_foundry_config_with_profile(&root, "deep").expect("foundry config");

        assert_eq!(cfg.src.as_deref(), Some("src"));
        assert_eq!(cfg.out.as_deref(), Some("out"));
        assert_eq!(cfg.libs.as_deref(), Some(&["lib".to_string()][..]));
        assert_eq!(cfg.solc.as_deref(), Some("0.8.28"));
        assert_eq!(cfg.evm_version.as_deref(), Some("cancun"));
        assert_eq!(cfg.optimizer, Some(true));
        assert_eq!(cfg.optimizer_runs, Some(777));
        assert_eq!(cfg.via_ir, Some(true));
        assert_eq!(cfg.auto_detect_remappings, Some(false));
        assert_eq!(cfg.remappings.len(), 1);
        assert_eq!(cfg.remappings[0].name, "manual/");
    }

    #[test]
    fn foundry_config_respects_explicit_remappings_only() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/foundry_config");
        let cfg = read_foundry_config(&root).expect("foundry config");
        let paths = project_paths(&root, &cfg).expect("project paths");

        assert!(paths.remappings.iter().any(|r| r.name == "manual/"));
        assert!(
            paths.remappings.iter().all(|r| !r.name.contains("auto")),
            "auto-detected remapping should not be present when auto_detect_remappings=false: {:?}",
            paths.remappings
        );

        let settings = solc_settings(&cfg).expect("solc settings");
        assert_eq!(settings.via_ir, Some(true));
        assert_eq!(settings.optimizer.enabled, Some(true));
        assert_eq!(settings.optimizer.runs, Some(1));
    }

    #[test]
    fn explicit_remappings_take_precedence_over_autodetected_remappings() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/foundry_config");
        let mut cfg = read_foundry_config(&root).expect("foundry config");
        cfg.auto_detect_remappings = Some(true);
        cfg.remappings = vec![
            parse_remapping(&root, "manual/=lib/auto/src/").expect("explicit remapping"),
            parse_remapping(&root, "src/:auto/=lib/manual/src/").expect("context remapping"),
        ];

        let paths = project_paths(&root, &cfg).expect("project paths");
        let manual = paths
            .remappings
            .iter()
            .filter(|r| r.name == "manual/" && r.context.is_none())
            .collect::<Vec<_>>();
        assert_eq!(
            manual.len(),
            1,
            "autodetection must not duplicate an explicit remapping"
        );
        assert_eq!(
            manual[0].path,
            root.join("lib/auto/src/").to_string_lossy().as_ref()
        );

        assert!(paths
            .remappings
            .iter()
            .any(|r| r.name == "auto/" && r.context.as_deref() == Some("src/")));
        assert!(paths
            .remappings
            .iter()
            .any(|r| r.name == "auto/" && r.context.is_none()));
    }

    #[test]
    fn parses_foundry_project_with_inline_remappings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/foundry_config");
        let project = SolcFrontend.parse_project(&root).expect("parse foundry config fixture");
        assert!(project.contracts.iter().any(|c| c.name == "UsesManual"));
        assert!(project.contracts.iter().all(|c| c.name != "AutoLib"));
        assert!(project.source_files.iter().any(|f| f.path.ends_with("lib/manual/src/ManualLib.sol")));
        assert!(project.source_files.iter().all(|f| !f.path.ends_with("lib/auto/src/AutoLib.sol")));
    }

    #[test]
    fn maps_tuple_and_index_range() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/statements");
        let project = SolcFrontend.parse_project(&root).expect("parse statements");
        let stmts = project.contracts.iter().find(|c| c.name == "Stmts").expect("Stmts");

        let split = stmts.functions.iter().find(|f| f.name == "split").expect("split");
        match &return_value(split).kind {
            ExpressionKind::Tuple { elements } => {
                assert_eq!(elements.len(), 2, "(x, x) has 2 elements");
                assert!(elements.iter().all(|e| e.is_some()), "no empty tuple slots");
            }
            other => panic!("expected Tuple, got {other:?}"),
        }

        let slice = stmts.functions.iter().find(|f| f.name == "slice").expect("slice");
        match &return_value(slice).kind {
            ExpressionKind::IndexRange { base, start, end } => {
                assert!(matches!(base.kind, ExpressionKind::Identifier { .. }), "base is the array");
                assert!(start.is_some(), "start present");
                assert!(end.is_some(), "end present");
            }
            other => panic!("expected IndexRange, got {other:?}"),
        }
    }
}
