use petgraph::stable_graph::NodeIndex;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::model::common::{Param, SourceSpan};
use crate::model::contract::ContractDef;
use crate::model::decl_id::{DeclId, DeclTable};
use crate::model::expression::{
    AssignOperator, CallKind, CallOption, CallType, Expression, ExpressionKind, UnaryOperator,
};
use crate::model::function::FunctionDef;
use crate::model::modifier::ModifierDef;
use crate::model::project::Project;
use crate::model::statement::{Statement, StatementKind};
use crate::util::push_unique;

use super::types::*;

pub struct CfgBuilder<'p> {
    graph: CfgGraph,
    project: Option<&'p Project>,
    // names the library behind `using for` calls
    decls: Option<&'p DeclTable>,
    next_block_id: usize,
    current_block: NodeIndex,
    // provenance is tagged on top-level statements
    current_modifier: Option<String>,
    // the statement being processed; what it adds without a narrower span cites it
    current_span: Option<SourceSpan>,
    scope: Scope,
    // `return` of a function returning a storage pointer does not read the storage
    returns_storage: bool,
    // blocks left through break/continue; control never falls out of them
    closed: HashSet<NodeIndex>,
    loops: Vec<LoopFrame>,
    // modifiers in application order, then the function body
    levels: Vec<Level>,
    body: Rc<Vec<Statement>>,
    level: usize,
    // where a `return` of each inlined body continues: after its placeholder, innermost last
    exits: Vec<Option<NodeIndex>>,
}

#[derive(Clone)]
enum Level {
    Modifier(Rc<ModifierDef>),
    Unresolved(String),
}

// Jump targets of the innermost enclosing loop. Blocks that may stay unused
// (for increment, do-while condition and exit) are created on first use.
struct LoopFrame {
    continue_block: Option<NodeIndex>,
    continue_kind: BlockKind,
    continue_edge: BranchEdge,
    exit_block: Option<NodeIndex>,
}

/// A variable as an identifier refers to it: by solc id, or by name in models without ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum VarKey {
    Id(DeclId),
    Name(String),
}

/// What denotes storage inside one function: state variables, `storage` parameters and
/// storage pointer locals with the locations they may point to.
#[derive(Default)]
struct Scope {
    state_ids: HashMap<DeclId, String>,
    state_names: HashSet<String>,
    params: Vec<Param>,
    // without solc ids, names are the only way to tell state from locals
    by_name: bool,
    storage_locals: HashSet<VarKey>,
    aliases: Aliases,
}

type Aliases = HashMap<VarKey, Vec<StorageRef>>;

// with no branch reaching the join, code there (a modifier after `_`) still sees the pointers
fn join_aliases(ends: Vec<Aliases>, before: Aliases) -> Aliases {
    if ends.is_empty() { before } else { merge_aliases(ends) }
}

fn merge_aliases(ends: Vec<Aliases>) -> Aliases {
    let mut merged = Aliases::new();
    for (key, places) in ends.into_iter().flatten() {
        let into = merged.entry(key).or_default();
        for place in places {
            push_unique(into, place);
        }
    }
    merged
}

impl Scope {
    fn key(name: &str, resolved: Option<DeclId>) -> VarKey {
        match resolved {
            Some(id) => VarKey::Id(id),
            None => VarKey::Name(name.to_string()),
        }
    }

    fn storage_param(&self, name: &str, resolved: Option<DeclId>) -> Option<usize> {
        self.params.iter().position(|p| {
            p.is_storage
                && match resolved {
                    Some(id) => p.id == Some(id),
                    None => self.by_name && p.name == name,
                }
        })
    }

    fn root(&self, name: &str, resolved: Option<DeclId>) -> Vec<StorageRef> {
        if resolved.is_none() && !self.by_name {
            return Vec::new();
        }
        if let Some(places) = self.aliases.get(&Self::key(name, resolved)) {
            return places.clone();
        }
        if let Some(i) = self.storage_param(name, resolved) {
            return vec![StorageRef { path: self.params[i].name.clone(), param: Some(i) }];
        }
        let state = match resolved {
            Some(id) => self.state_ids.get(&id).map(|n| StorageRef::state(n.clone())),
            None => self.state_names.contains(name).then(|| StorageRef::state(name)),
        };
        state.into_iter().collect()
    }

    fn point(&mut self, key: VarKey, places: Vec<StorageRef>) {
        if places.is_empty() {
            self.aliases.remove(&key);
        } else {
            self.aliases.insert(key, places);
        }
    }

    /// A storage pointer variable: a storage local or a `storage` parameter.
    fn is_pointer(&self, expr: &Expression) -> Option<VarKey> {
        let ExpressionKind::Identifier { name, resolved } = &expr.kind else { return None };
        let key = Self::key(name, *resolved);
        let pointer = self.storage_locals.contains(&key)
            || self.aliases.contains_key(&key)
            || self.storage_param(name, *resolved).is_some();
        pointer.then_some(key)
    }
}

impl<'p> CfgBuilder<'p> {
    pub fn build(function: &FunctionDef, contract: &ContractDef) -> CfgGraph {
        Self::build_with_project(function, contract, None)
    }

    /// `contract` is the most derived contract: modifiers resolve to the overrides it sees.
    pub fn build_with_project(
        function: &FunctionDef,
        contract: &ContractDef,
        project: Option<&'p Project>,
    ) -> CfgGraph {
        let state = match project {
            Some(p) => p.inherited_state_vars(contract),
            None => contract.state_vars.clone(),
        };
        let state: Vec<_> = state.into_iter().filter(|v| v.is_storage()).collect();
        let by_name = function.id.is_none()
            && state.iter().all(|v| v.id.is_none())
            && function.params.iter().all(|p| p.id.is_none());
        let scope = Scope {
            state_ids: state.iter().filter_map(|v| Some((v.id?, v.name.clone()))).collect(),
            state_names: state.iter().map(|v| v.name.clone()).collect(),
            params: function.params.clone(),
            by_name,
            ..Scope::default()
        };
        let levels = function
            .modifiers
            .iter()
            .map(|mref| {
                let def = match project {
                    Some(proj) => proj.resolve_modifier(contract, &mref.name),
                    None => contract.modifiers.iter().find(|m| m.name == mref.name),
                };
                // a modifier declared without a body (`virtual;`) has no code to inline
                match def {
                    Some(def) if !def.body.is_empty() => Level::Modifier(Rc::new(def.clone())),
                    _ => Level::Unresolved(mref.name.clone()),
                }
            })
            .collect();
        let mut builder = CfgBuilder {
            graph: CfgGraph::new(),
            project,
            decls: project.map(|p| &p.decl_table),
            next_block_id: 0,
            current_block: NodeIndex::new(0),
            current_modifier: None,
            current_span: None,
            scope,
            returns_storage: function.returns.len() == 1 && function.returns[0].is_storage,
            closed: HashSet::new(),
            loops: Vec::new(),
            levels,
            body: Rc::new(Vec::new()),
            level: 0,
            exits: Vec::new(),
        };

        let entry = builder.add_block(BlockKind::Entry);
        builder.current_block = entry;

        let Some(body) = &function.body else {
            return builder.graph;
        };
        builder.body = Rc::new(body.clone());
        builder.run_level(0);
        builder.current_modifier = None;

        if !builder.block_is_terminal(builder.current_block)
            && builder
                .graph
                .edges(builder.current_block)
                .next()
                .is_none()
        {
            let ret = builder.add_block(BlockKind::Return);
            builder.add_edge(builder.current_block, ret, BranchEdge::Unconditional);
        }

        for block in builder.graph.node_weights_mut() {
            if let Some(span) = block_span(&block.statements) {
                block.span = Some(span);
            }
        }
        builder.graph
    }

    // Runs modifier `level` (its `_` runs the next level), or the body after the last one.
    // Solidity inlines the same way: the body sits at the innermost placeholder.
    fn run_level(&mut self, level: usize) {
        self.level = level;
        match self.levels.get(level).cloned() {
            None => {
                self.current_modifier = None;
                let body = Rc::clone(&self.body);
                for s in body.iter() {
                    self.process_statement(s);
                }
            }
            Some(Level::Unresolved(name)) => {
                self.add_stmt_to_current(CfgStatement::UnresolvedModifier { name, span: None, from_modifier: None });
                self.run_level(level + 1);
            }
            Some(Level::Modifier(def)) => {
                self.current_modifier = Some(def.name.clone());
                for s in &def.body {
                    self.process_statement(s);
                }
            }
        }
    }

    // `_`: the inner levels run here; a `return` in them continues after the placeholder,
    // so the code after `_` runs on every exit that does not revert.
    fn process_placeholder(&mut self) {
        let level = self.level;
        if level >= self.levels.len() {
            return;
        }
        let modifier = self.current_modifier.clone();
        self.exits.push(None);
        self.run_level(level + 1);
        if let Some(Some(exit)) = self.exits.pop() {
            if !self.block_is_terminal(self.current_block) {
                self.add_edge(self.current_block, exit, BranchEdge::Unconditional);
            }
            self.current_block = exit;
        }
        self.level = level;
        self.current_modifier = modifier;
    }

    fn add_block(&mut self, kind: BlockKind) -> NodeIndex {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.graph.add_node(BasicBlock {
            id,
            kind,
            statements: Vec::new(),
            span: None,
            return_value: None,
            reads: Vec::new(),
            revert_reason: None,
        })
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge: BranchEdge) {
        self.graph.add_edge(from, to, edge);
    }

    fn add_stmt_to_current(&mut self, mut stmt: CfgStatement) {
        let span = stmt.span_mut();
        if span.is_none() {
            *span = self.current_span;
        }
        if let Some(block) = self.graph.node_weight_mut(self.current_block) {
            block.statements.push(stmt);
        }
    }

    fn process_statement(&mut self, stmt: &Statement) {
        // code after revert/break/continue never runs
        if self.closed.contains(&self.current_block)
            || self.graph[self.current_block].kind == BlockKind::Revert
        {
            return;
        }
        let outer = self.current_span.replace(stmt.span);
        self.process_statement_kind(stmt);
        self.current_span = outer;
    }

    fn process_statement_kind(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::If { condition, then_body, else_body } => {
                self.process_if(condition, then_body, else_body.as_deref());
            }
            StatementKind::For { init, condition, increment, body } => {
                self.process_for(init.as_deref(), condition.as_ref(), increment.as_ref(), body);
            }
            StatementKind::While { condition, body } => {
                self.process_while(condition, body);
            }
            StatementKind::DoWhile { body, condition } => {
                self.process_do_while(body, condition);
            }
            StatementKind::Return { value } => {
                self.process_return(value.as_ref());
            }
            StatementKind::Revert { error_name, arguments } => {
                let args = arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", ");
                let reason = match error_name.as_deref() {
                    Some(name) if !name.is_empty() => format!("revert {name}({args})"),
                    _ => format!("revert({args})"),
                };
                self.process_revert(arguments, reason);
            }
            StatementKind::Emit { event_name, arguments } => {
                for arg in arguments {
                    self.add_effects(arg);
                }
                let from_modifier = self.current_modifier.clone();
                let args = arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", ");
                self.add_stmt_to_current(CfgStatement::EmitEvent {
                    event: event_name.clone(),
                    span: None,
                    from_modifier,
                    arguments: args,
                });
            }
            StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                for s in statements {
                    self.process_statement(s);
                }
            }
            StatementKind::ExpressionStmt { expression } => {
                self.process_expression_stmt(expression);
            }
            StatementKind::VariableDeclaration { name, initial_value, is_storage_ref, id, .. } => {
                let key = Scope::key(name, *id);
                if *is_storage_ref {
                    self.scope.storage_locals.insert(key.clone());
                }
                if let Some(val) = initial_value {
                    if *is_storage_ref {
                        // a pointer: binds a location without reading it
                        let places = self.place(val);
                        self.pointer_effects(val);
                        self.scope.point(key, places);
                    } else {
                        self.add_effects(val);
                    }
                    let from_modifier = self.current_modifier.clone();
                    self.add_stmt_to_current(CfgStatement::Assignment {
                        target: name.clone(),
                        value: expr_to_string(val),
                        operator: AssignOperator::Assign,
                        span: None,
                        from_modifier,
                        storage: None,
                    });
                }
            }
            StatementKind::TryCatch { expression, clauses } => {
                self.process_try_catch(expression, clauses);
            }
            StatementKind::Assembly { .. } => {
                let from_modifier = self.current_modifier.clone();
                self.add_stmt_to_current(CfgStatement::AssemblyBlock { span: None, from_modifier });
            }
            StatementKind::Break => {
                if let Some(exit) = self.loop_exit() {
                    self.jump(exit, BranchEdge::LoopExit);
                }
            }
            StatementKind::Continue => {
                if let Some((target, edge)) = self.loop_continue() {
                    self.jump(target, edge);
                }
            }
            StatementKind::Placeholder => self.process_placeholder(),
        }
    }

    fn jump(&mut self, target: NodeIndex, edge: BranchEdge) {
        self.add_edge(self.current_block, target, edge);
        self.closed.insert(self.current_block);
    }

    fn loop_continue(&mut self) -> Option<(NodeIndex, BranchEdge)> {
        let (existing, kind, edge) = {
            let frame = self.loops.last()?;
            (frame.continue_block, frame.continue_kind, frame.continue_edge.clone())
        };
        let block = match existing {
            Some(block) => block,
            None => {
                let block = self.add_block(kind);
                self.loops.last_mut()?.continue_block = Some(block);
                block
            }
        };
        Some((block, edge))
    }

    fn loop_exit(&mut self) -> Option<NodeIndex> {
        let existing = self.loops.last()?.exit_block;
        Some(match existing {
            Some(block) => block,
            None => {
                let block = self.add_block(BlockKind::Normal);
                self.loops.last_mut()?.exit_block = Some(block);
                block
            }
        })
    }

    // falls through to the continue target unless the body already left the block
    fn close_loop_body(&mut self) -> LoopFrame {
        if !self.block_is_terminal(self.current_block) {
            if let Some((target, edge)) = self.loop_continue() {
                self.add_edge(self.current_block, target, edge);
            }
        }
        self.loops.pop().expect("loop frame")
    }


    // calls, nested writes and reads of an expression whose value is consumed elsewhere
    fn add_effects(&mut self, expr: &Expression) {
        let mut reads = Vec::new();
        self.reads(expr, &mut reads);
        self.push_reads(reads);
        self.push_calls(expr);
    }

    // a storage pointer expression: only its index expressions and calls are evaluated
    fn pointer_effects(&mut self, expr: &Expression) {
        let mut reads = Vec::new();
        self.index_reads(expr, &mut reads);
        self.push_reads(reads);
        self.push_calls(expr);
    }

    fn push_calls(&mut self, expr: &Expression) {
        let mut stmts = Vec::new();
        self.collect_calls(expr, &mut stmts);
        for s in stmts {
            self.add_stmt_to_current(s);
        }
    }

    // an expression evaluated for its effects, assignments included
    fn evaluate(&mut self, expr: &Expression) {
        let ExpressionKind::Assignment { target, operator, value } = &expr.kind else {
            self.add_effects(expr);
            return;
        };
        if *operator == AssignOperator::Assign {
            if let Some(key) = self.scope.is_pointer(target) {
                // re-pointing a storage pointer
                let places = self.place(value);
                self.pointer_effects(value);
                self.scope.point(key, places);
                self.push_assignment(target, value, *operator, Vec::new());
                return;
            }
        }
        let mut reads = Vec::new();
        self.reads(expr, &mut reads);
        self.push_reads(reads);
        // what the value and the target's indexes do comes before the write
        self.push_calls(target);
        self.push_calls(value);
        if let ExpressionKind::Tuple { elements } = &target.kind {
            self.push_assignment(target, value, *operator, Vec::new());
            for element in elements.iter().flatten() {
                for place in self.place(element) {
                    self.push_storage_write(place);
                }
            }
        } else {
            let places = self.place(target);
            self.push_assignment(target, value, *operator, places);
        }
    }

    fn push_assignment(&mut self, target: &Expression, value: &Expression, operator: AssignOperator, places: Vec<StorageRef>) {
        let from_modifier = self.current_modifier.clone();
        let mut places = places.into_iter();
        let storage = places.next();
        self.add_stmt_to_current(CfgStatement::Assignment {
            target: storage.as_ref().map(|p| p.path.clone()).unwrap_or_else(|| expr_to_string(target)),
            value: expr_to_string(value),
            operator,
            span: None,
            from_modifier,
            storage,
        });
        for place in places {
            self.push_storage_write(place);
        }
    }

    fn push_storage_write(&mut self, place: StorageRef) {
        let from_modifier = self.current_modifier.clone();
        self.add_stmt_to_current(storage_write(place, from_modifier));
    }

    fn push_reads(&mut self, reads: Vec<StorageRef>) {
        let block = &mut self.graph[self.current_block];
        for read in reads {
            if !block.reads.contains(&read) {
                block.reads.push(read);
            }
        }
    }

    // Every storage place `expr` may denote; calls returning a storage pointer are followed once.
    fn place(&self, expr: &Expression) -> Vec<StorageRef> {
        self.place_in(&self.scope, expr, true)
    }

    fn place_in(&self, scope: &Scope, expr: &Expression, follow_calls: bool) -> Vec<StorageRef> {
        let extend = |places: Vec<StorageRef>, suffix: String| places.iter().map(|p| p.extend(&suffix)).collect();
        match &expr.kind {
            ExpressionKind::Identifier { name, resolved } => scope.root(name, *resolved),
            ExpressionKind::MemberAccess { object, member, .. } => {
                extend(self.place_in(scope, object, follow_calls), format!(".{member}"))
            }
            ExpressionKind::IndexAccess { base, index } => {
                let index = index.as_ref().map(|e| expr_to_string(e)).unwrap_or_default();
                extend(self.place_in(scope, base, follow_calls), format!("[{index}]"))
            }
            ExpressionKind::Tuple { elements } if elements.len() == 1 => match &elements[0] {
                Some(e) => self.place_in(scope, e, follow_calls),
                None => Vec::new(),
            },
            ExpressionKind::Ternary { true_expr, false_expr, .. } => {
                let mut places = self.place_in(scope, true_expr, follow_calls);
                for p in self.place_in(scope, false_expr, follow_calls) {
                    push_unique(&mut places, p);
                }
                places
            }
            ExpressionKind::FunctionCall { callee, arguments, call: CallType::Call(kind), .. } if follow_calls => {
                self.returned_place(callee, arguments, *kind)
            }
            _ => Vec::new(),
        }
    }

    // The known places the `return`s of a callee returning one storage pointer may yield.
    fn returned_place(&self, callee: &Expression, arguments: &[Expression], kind: CallKind) -> Vec<StorageRef> {
        let Some(def) = self.callee_def(callee) else { return Vec::new() };
        let ([ret], Some(body)) = (def.returns.as_slice(), def.body.as_deref()) else { return Vec::new() };
        if !ret.is_storage {
            return Vec::new();
        }
        let callee_scope = Scope {
            // libraries have no storage of their own; contract functions share ours
            state_ids: if kind == CallKind::Internal { self.scope.state_ids.clone() } else { HashMap::new() },
            params: def.params.clone(),
            by_name: self.scope.by_name,
            ..Scope::default()
        };
        let mut returned = Vec::new();
        collect_returned(body, ret, &mut returned);
        let args = call_arguments(callee, arguments, kind);
        let mut out: Vec<StorageRef> = Vec::new();
        for place in returned.into_iter().flat_map(|e| self.place_in(&callee_scope, e, false)) {
            let bound = match place.param {
                None => vec![place],
                Some(i) => {
                    let suffix = &place.path[def.params[i].name.len()..];
                    args.get(i).map(|arg| self.place(arg)).unwrap_or_default().iter().map(|p| p.extend(suffix)).collect()
                }
            };
            for p in bound {
                push_unique(&mut out, p);
            }
        }
        out
    }

    fn callee_def(&self, callee: &Expression) -> Option<&'p FunctionDef> {
        let resolved = match &callee.kind {
            ExpressionKind::Identifier { resolved, .. } | ExpressionKind::MemberAccess { resolved, .. } => (*resolved)?,
            _ => return None,
        };
        let project = self.project?;
        let target = project.decl_table.function(resolved)?;
        let owner = &project.contracts[*project.contract_index.get(&target.contract)?];
        owner.functions.iter().find(|f| f.key == target.function)
    }

    // For each argument of a call (the `using for` receiver first): bound to a `storage` parameter.
    fn storage_bindings(&self, callee: &Expression, arguments: &[Expression], call: CallType) -> Vec<bool> {
        let CallType::Call(kind) = call else { return Vec::new() };
        let Some(def) = self.callee_def(callee) else { return Vec::new() };
        let count = call_arguments(callee, arguments, kind).len();
        (0..count).map(|i| def.params.get(i).is_some_and(|p| p.is_storage)).collect()
    }

    fn storage_args(&self, callee: &Expression, arguments: &[Expression], call: CallType) -> Vec<StorageArg> {
        let CallType::Call(kind) = call else { return Vec::new() };
        let bindings = self.storage_bindings(callee, arguments, call);
        call_arguments(callee, arguments, kind)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| bindings.get(*i).copied().unwrap_or(false))
            .flat_map(|(param, arg)| self.place(arg).into_iter().map(move |place| StorageArg { param, place }))
            .collect()
    }

    // Storage `expr` reads when evaluated. A storage path counts once as a whole; the
    // storage named by a pointer binding or an assignment target is not read.
    fn reads(&self, expr: &Expression, out: &mut Vec<StorageRef>) {
        match &expr.kind {
            ExpressionKind::Identifier { .. } | ExpressionKind::MemberAccess { .. } | ExpressionKind::IndexAccess { .. } => {
                let places = self.place_in(&self.scope, expr, false);
                if !places.is_empty() {
                    out.extend(places);
                    self.index_reads(expr, out);
                    return;
                }
                match &expr.kind {
                    ExpressionKind::MemberAccess { object, .. } => self.reads(object, out),
                    ExpressionKind::IndexAccess { base, index } => {
                        self.reads(base, out);
                        if let Some(index) = index {
                            self.reads(index, out);
                        }
                    }
                    _ => {}
                }
            }
            ExpressionKind::FunctionCall { callee, arguments, options, call } => {
                let bindings = self.storage_bindings(callee, arguments, *call);
                let receiver = matches!(call, CallType::Call(kind) if is_attached(callee, *kind));
                let bound = |i: usize| bindings.get(i).copied().unwrap_or(false);
                match &callee.kind {
                    ExpressionKind::MemberAccess { object, .. } if receiver && bound(0) => self.index_reads(object, out),
                    ExpressionKind::MemberAccess { object, .. } => self.reads(object, out),
                    ExpressionKind::Identifier { .. } | ExpressionKind::TypeMeta { .. } => {}
                    _ => self.reads(callee, out),
                }
                for option in options {
                    self.reads(&option.value, out);
                }
                for (i, arg) in arguments.iter().enumerate() {
                    if bound(i + receiver as usize) {
                        self.index_reads(arg, out);
                    } else {
                        self.reads(arg, out);
                    }
                }
            }
            ExpressionKind::Assignment { target, operator, value } => {
                if *operator == AssignOperator::Assign && self.scope.is_pointer(target).is_some() {
                    self.index_reads(value, out);
                    return;
                }
                if *operator == AssignOperator::Assign {
                    self.target_reads(target, out);
                } else {
                    self.reads(target, out);
                }
                self.reads(value, out);
            }
            ExpressionKind::UnaryOp { operator: UnaryOperator::Delete, operand } => self.target_reads(operand, out),
            ExpressionKind::UnaryOp { operand, .. } => self.reads(operand, out),
            ExpressionKind::BinaryOp { left, right, .. } => {
                self.reads(left, out);
                self.reads(right, out);
            }
            ExpressionKind::Ternary { condition, true_expr, false_expr } => {
                self.reads(condition, out);
                self.reads(true_expr, out);
                self.reads(false_expr, out);
            }
            ExpressionKind::Tuple { elements } => {
                for e in elements.iter().flatten() {
                    self.reads(e, out);
                }
            }
            ExpressionKind::TypeCast { expression, .. } => self.reads(expression, out),
            ExpressionKind::New { arguments, options, .. } => {
                for option in options {
                    self.reads(&option.value, out);
                }
                for arg in arguments {
                    self.reads(arg, out);
                }
            }
            ExpressionKind::IndexRange { base, start, end } => {
                self.reads(base, out);
                for e in [start, end].into_iter().flatten() {
                    self.reads(e, out);
                }
            }
            ExpressionKind::Literal { .. } | ExpressionKind::TypeMeta { .. } => {}
        }
    }

    // reads made while locating storage: index expressions and the arguments of a call
    fn index_reads(&self, expr: &Expression, out: &mut Vec<StorageRef>) {
        match &expr.kind {
            ExpressionKind::MemberAccess { object, .. } => self.index_reads(object, out),
            ExpressionKind::IndexAccess { base, index } => {
                self.index_reads(base, out);
                if let Some(index) = index {
                    self.reads(index, out);
                }
            }
            ExpressionKind::Tuple { elements } if elements.len() == 1 => {
                if let Some(e) = &elements[0] {
                    self.index_reads(e, out);
                }
            }
            ExpressionKind::FunctionCall { .. } => self.reads(expr, out),
            _ => {}
        }
    }

    // an assignment target: only what locates it is read
    fn target_reads(&self, target: &Expression, out: &mut Vec<StorageRef>) {
        match &target.kind {
            ExpressionKind::Tuple { elements } => {
                for e in elements.iter().flatten() {
                    self.target_reads(e, out);
                }
            }
            _ if !self.place(target).is_empty() => self.index_reads(target, out),
            _ => self.reads(target, out),
        }
    }

    fn process_revert(&mut self, arguments: &[Expression], reason: String) {
        for arg in arguments {
            self.add_effects(arg);
        }
        let revert = self.add_block(BlockKind::Revert);
        self.graph[revert].revert_reason = Some(reason);
        self.graph[revert].span = self.current_span;
        self.add_edge(self.current_block, revert, BranchEdge::Unconditional);
        self.current_block = revert;
    }

    fn process_if(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: Option<&[Statement]>,
    ) {
        self.add_effects(condition);
        let cond_str = expr_to_string(condition);
        let before = self.current_block;

        let then_block = self.add_block(BlockKind::Normal);
        self.add_edge(
            before,
            then_block,
            BranchEdge::ConditionalTrue { condition: cond_str.clone() },
        );
        let before_aliases = self.scope.aliases.clone();
        self.current_block = then_block;
        for s in then_body {
            self.process_statement(s);
        }
        let then_end = self.current_block;
        let then_aliases = std::mem::replace(&mut self.scope.aliases, before_aliases.clone());

        let else_end = if let Some(else_stmts) = else_body {
            let else_block = self.add_block(BlockKind::Normal);
            self.add_edge(
                before,
                else_block,
                BranchEdge::ConditionalFalse { condition: cond_str },
            );
            self.current_block = else_block;
            for s in else_stmts {
                self.process_statement(s);
            }
            self.current_block
        } else {
            let join = self.add_block(BlockKind::Normal);
            self.add_edge(
                before,
                join,
                BranchEdge::ConditionalFalse { condition: cond_str },
            );
            join
        };

        let then_needs_join = !self.block_is_terminal(then_end);
        let else_needs_join = else_body.is_some() && !self.block_is_terminal(else_end);

        let else_aliases = std::mem::take(&mut self.scope.aliases);
        let mut ends = Vec::new();
        if then_needs_join {
            ends.push(then_aliases);
        }
        if else_needs_join || else_body.is_none() {
            ends.push(else_aliases);
        }
        self.scope.aliases = join_aliases(ends, before_aliases);

        if then_needs_join || else_needs_join || else_body.is_none() {
            let join = if else_body.is_none() {
                else_end
            } else {
                self.add_block(BlockKind::Normal)
            };

            if then_needs_join {
                self.add_edge(then_end, join, BranchEdge::Unconditional);
            }
            if else_needs_join {
                self.add_edge(else_end, join, BranchEdge::Unconditional);
            }
            self.current_block = join;
        }
    }

    fn process_for(
        &mut self,
        init: Option<&Statement>,
        condition: Option<&Expression>,
        increment: Option<&Expression>,
        body: &[Statement],
    ) {
        if let Some(init_stmt) = init {
            self.process_statement(init_stmt);
        }
        let entry_aliases = self.widen_loop_aliases(body, increment);

        let cond_block = self.add_block(BlockKind::LoopCondition);
        self.add_edge(self.current_block, cond_block, BranchEdge::Unconditional);

        let exit_block = self.add_block(BlockKind::Normal);
        let body_block = self.add_block(BlockKind::Normal);

        if let Some(cond) = condition {
            self.current_block = cond_block;
            self.add_effects(cond);
            let cond_str = expr_to_string(cond);
            self.add_edge(
                cond_block,
                body_block,
                BranchEdge::ConditionalTrue { condition: cond_str.clone() },
            );
            self.add_edge(
                cond_block,
                exit_block,
                BranchEdge::ConditionalFalse { condition: cond_str },
            );
        } else {
            self.add_edge(cond_block, body_block, BranchEdge::Unconditional);
        }

        // continue runs the increment before re-checking the condition
        let (continue_block, continue_kind, continue_edge) = match increment {
            Some(_) => (None, BlockKind::Normal, BranchEdge::Unconditional),
            None => (Some(cond_block), BlockKind::LoopCondition, BranchEdge::LoopBack),
        };
        self.loops.push(LoopFrame {
            continue_block,
            continue_kind,
            continue_edge,
            exit_block: Some(exit_block),
        });

        self.current_block = body_block;
        for s in body {
            self.process_statement(s);
        }
        let frame = self.close_loop_body();

        if let (Some(incr), Some(incr_block)) = (increment, frame.continue_block) {
            self.current_block = incr_block;
            self.evaluate(incr);
            self.add_edge(incr_block, cond_block, BranchEdge::LoopBack);
        }

        self.current_block = exit_block;
        self.join_loop_aliases(entry_aliases);
    }

    fn process_while(&mut self, condition: &Expression, body: &[Statement]) {
        let cond_str = expr_to_string(condition);
        let entry_aliases = self.widen_loop_aliases(body, None);

        let cond_block = self.add_block(BlockKind::LoopCondition);
        self.add_edge(self.current_block, cond_block, BranchEdge::Unconditional);

        let body_block = self.add_block(BlockKind::Normal);
        let exit_block = self.add_block(BlockKind::Normal);

        self.current_block = cond_block;
        self.add_effects(condition);
        self.add_edge(
            cond_block,
            body_block,
            BranchEdge::ConditionalTrue { condition: cond_str.clone() },
        );
        self.add_edge(
            cond_block,
            exit_block,
            BranchEdge::ConditionalFalse { condition: cond_str },
        );

        self.loops.push(LoopFrame {
            continue_block: Some(cond_block),
            continue_kind: BlockKind::LoopCondition,
            continue_edge: BranchEdge::LoopBack,
            exit_block: Some(exit_block),
        });
        self.current_block = body_block;
        for s in body {
            self.process_statement(s);
        }
        self.close_loop_body();

        self.current_block = exit_block;
        self.join_loop_aliases(entry_aliases);
    }

    fn process_do_while(&mut self, body: &[Statement], condition: &Expression) {
        let cond_str = expr_to_string(condition);
        let entry_aliases = self.widen_loop_aliases(body, None);

        let body_block = self.add_block(BlockKind::Normal);
        self.add_edge(self.current_block, body_block, BranchEdge::Unconditional);

        self.loops.push(LoopFrame {
            continue_block: None,
            continue_kind: BlockKind::LoopCondition,
            continue_edge: BranchEdge::Unconditional,
            exit_block: None,
        });
        self.current_block = body_block;
        for s in body {
            self.process_statement(s);
        }
        let end = self.current_block;
        let frame = self.close_loop_body();

        // the condition is unreachable when the body never falls through or continues
        let Some(cond_block) = frame.continue_block else {
            self.current_block = frame.exit_block.unwrap_or(end);
            self.join_loop_aliases(entry_aliases);
            return;
        };
        self.current_block = cond_block;
        self.add_effects(condition);

        let exit_block = frame
            .exit_block
            .unwrap_or_else(|| self.add_block(BlockKind::Normal));
        self.add_edge(
            cond_block,
            body_block,
            BranchEdge::ConditionalTrue { condition: cond_str.clone() },
        );
        self.add_edge(
            cond_block,
            exit_block,
            BranchEdge::ConditionalFalse { condition: cond_str },
        );

        self.current_block = exit_block;
        self.join_loop_aliases(entry_aliases);
    }

    // Any iteration may see a place a pointer is re-pointed to later in the loop.
    fn widen_loop_aliases(&mut self, body: &[Statement], increment: Option<&Expression>) -> Aliases {
        let mut repoints = Vec::new();
        collect_repoints(body, &mut repoints);
        if let Some(incr) = increment {
            repoint(incr, &mut repoints);
        }
        // a pointer re-pointed through another one needs a few rounds
        for _ in 0..4 {
            let mut changed = false;
            for (target, value) in &repoints {
                let Some(key) = self.scope.is_pointer(target) else { continue };
                let mut places = self.place(target);
                let known = places.len();
                for p in self.place(value) {
                    push_unique(&mut places, p);
                }
                changed |= places.len() > known;
                self.scope.point(key, places);
            }
            if !changed {
                break;
            }
        }
        self.scope.aliases.clone()
    }

    // the loop may have run zero or more times
    fn join_loop_aliases(&mut self, entry: Aliases) {
        let end = std::mem::take(&mut self.scope.aliases);
        self.scope.aliases = merge_aliases(vec![entry, end]);
    }

    fn process_return(&mut self, value: Option<&Expression>) {
        if let Some(expr) = value {
            if self.returns_storage {
                self.pointer_effects(expr);
            } else {
                self.evaluate(expr);
            }
        }
        // inside a modifier-inlined body, `return` leaves that body only
        if let Some(frame) = self.exits.last().copied() {
            let exit = frame.unwrap_or_else(|| self.add_block(BlockKind::Normal));
            if let Some(slot) = self.exits.last_mut() {
                *slot = Some(exit);
            }
            self.jump(exit, BranchEdge::Unconditional);
            return;
        }
        let ret = self.add_block(BlockKind::Return);
        self.graph[ret].span = self.current_span;
        if let Some(expr) = value {
            self.graph[ret].return_value = Some(expr_to_string(expr));
        }
        self.add_edge(self.current_block, ret, BranchEdge::Unconditional);
        self.current_block = ret;
    }

    fn process_try_catch(
        &mut self,
        expression: &Expression,
        clauses: &[crate::model::statement::CatchClause],
    ) {
        let before = self.current_block;
        self.evaluate(expression);

        let join = self.add_block(BlockKind::Normal);
        let start_aliases = self.scope.aliases.clone();
        let mut ends = Vec::new();

        for (i, clause) in clauses.iter().enumerate() {
            self.scope.aliases = start_aliases.clone();
            let clause_block = self.add_block(BlockKind::Normal);
            let edge = if i == 0 {
                BranchEdge::ExternalCallSuccess
            } else {
                BranchEdge::CatchClause {
                    kind: clause.name.clone().unwrap_or("default".into()),
                }
            };
            self.add_edge(before, clause_block, edge);

            self.current_block = clause_block;
            for s in &clause.body {
                self.process_statement(s);
            }
            if !self.block_is_terminal(self.current_block) {
                self.add_edge(self.current_block, join, BranchEdge::Unconditional);
                ends.push(std::mem::take(&mut self.scope.aliases));
            }
        }

        self.scope.aliases = join_aliases(ends, start_aliases);
        self.current_block = join;
    }

    fn process_expression_stmt(&mut self, expr: &Expression) {
        if let ExpressionKind::FunctionCall { callee, arguments, .. } = &expr.kind {
            if let ExpressionKind::Identifier { name, .. } = &callee.kind {
                match name.as_str() {
                    "require" => {
                        self.process_require(arguments);
                        return;
                    }
                    "assert" => {
                        self.process_assert(arguments);
                        return;
                    }
                    "revert" => {
                        let args = arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", ");
                        self.process_revert(arguments, format!("revert({args})"));
                        return;
                    }
                    _ => {}
                }
            }
        }

        self.evaluate(expr);
    }

    fn process_require(&mut self, arguments: &[Expression]) {
        if let Some(cond) = arguments.first() {
            self.add_effects(cond);
        }
        match arguments.get(1).map(|e| &e.kind) {
            // custom error constructor: only its arguments are evaluated calls
            Some(ExpressionKind::FunctionCall { arguments: error_args, .. }) => {
                for arg in error_args {
                    self.add_effects(arg);
                }
            }
            Some(_) => self.add_effects(&arguments[1]),
            None => {}
        }
        let condition = arguments.first().map(expr_to_string).unwrap_or_default();
        let message = arguments.get(1).map(expr_to_string);

        let cond_str = condition.clone();
        let from_modifier = self.current_modifier.clone();
        self.add_stmt_to_current(CfgStatement::RequireCheck {
            condition,
            message,
            span: None,
            from_modifier,
        });
        self.branch_or_revert(cond_str, "require");
    }

    fn process_assert(&mut self, arguments: &[Expression]) {
        if let Some(cond) = arguments.first() {
            self.add_effects(cond);
        }
        let condition = arguments.first().map(expr_to_string).unwrap_or_default();
        let cond_str = condition.clone();
        let from_modifier = self.current_modifier.clone();

        self.add_stmt_to_current(CfgStatement::AssertCheck {
            condition,
            span: None,
            from_modifier,
        });
        self.branch_or_revert(cond_str, "assert");
    }

    // continues on the true edge; the false edge reverts
    fn branch_or_revert(&mut self, condition: String, check: &str) {
        let before = self.current_block;

        let next = self.add_block(BlockKind::Normal);
        self.add_edge(
            before,
            next,
            BranchEdge::ConditionalTrue { condition: condition.clone() },
        );

        let revert = self.add_block(BlockKind::Revert);
        self.graph[revert].revert_reason = Some(format!("{check}({condition})"));
        self.graph[revert].span = self.current_span;
        self.add_edge(before, revert, BranchEdge::ConditionalFalse { condition });

        self.current_block = next;
    }

    fn block_is_terminal(&self, idx: NodeIndex) -> bool {
        let kind = self.graph[idx].kind;
        kind == BlockKind::Return || kind == BlockKind::Revert || self.closed.contains(&idx)
    }

    fn push_mutation(&self, expr: &Expression, operator: UnaryOperator, operand: &Expression, stmts: &mut Vec<CfgStatement>) {
        let places = self.place(operand);
        if !places.is_empty() {
            for place in places {
                stmts.push(storage_write(place, self.current_modifier.clone()));
            }
            return;
        }
        let (operator, value) = match operator {
            UnaryOperator::PreIncrement | UnaryOperator::PostIncrement => (AssignOperator::AddAssign, "1".to_string()),
            UnaryOperator::PreDecrement | UnaryOperator::PostDecrement => (AssignOperator::SubAssign, "1".to_string()),
            _ => (AssignOperator::Assign, expr_to_string(expr)),
        };
        stmts.push(CfgStatement::Assignment {
            target: expr_to_string(operand),
            value,
            operator,
            span: None,
            from_modifier: self.current_modifier.clone(),
            storage: None,
        });
    }

    // statement-level assignments go through `evaluate`; a nested re-pointing is not followed
    fn push_nested_assignment(&self, target: &Expression, operator: AssignOperator, value: &Expression, stmts: &mut Vec<CfgStatement>) {
        if operator == AssignOperator::Assign && self.scope.is_pointer(target).is_some() {
            return;
        }
        let targets: Vec<&Expression> = match &target.kind {
            ExpressionKind::Tuple { elements } => elements.iter().flatten().collect(),
            _ => vec![target],
        };
        for target in targets {
            let places = self.place(target);
            if places.is_empty() {
                stmts.push(CfgStatement::Assignment {
                    target: expr_to_string(target),
                    value: expr_to_string(value),
                    operator,
                    span: None,
                    from_modifier: self.current_modifier.clone(),
                    storage: None,
                });
            }
            for place in places {
                stmts.push(storage_write(place, self.current_modifier.clone()));
            }
        }
    }

    // calls and nested writes, in evaluation order
    fn collect_calls(&self, expr: &Expression, stmts: &mut Vec<CfgStatement>) {
        let start = stmts.len();
        self.collect_calls_of(expr, stmts);
        // nested expressions already cited themselves; the rest is this expression's own
        for stmt in &mut stmts[start..] {
            stmt.span_mut().get_or_insert(expr.span);
        }
    }

    fn collect_calls_of(&self, expr: &Expression, stmts: &mut Vec<CfgStatement>) {
        match &expr.kind {
            ExpressionKind::FunctionCall { callee, arguments, options, call } => {
                // evaluation order: callee object, options, arguments, then the call itself
                match &callee.kind {
                    ExpressionKind::Identifier { .. } | ExpressionKind::TypeMeta { .. } => {}
                    ExpressionKind::MemberAccess { object, .. } => self.collect_calls(object, stmts),
                    _ => self.collect_calls(callee, stmts),
                }
                for option in options {
                    self.collect_calls(&option.value, stmts);
                }
                for arg in arguments {
                    self.collect_calls(arg, stmts);
                }
                let args = arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", ");
                match call {
                    CallType::Call(kind) => {
                        let storage_args = self.storage_args(callee, arguments, *call);
                        stmts.push(self.typed_call(callee, *kind, args, options, storage_args));
                    }
                    CallType::Builtin => {
                        if let ExpressionKind::MemberAccess { object, member, .. } = &callee.kind {
                            if member == "push" || member == "pop" {
                                for place in self.place(object) {
                                    stmts.push(storage_write(place, self.current_modifier.clone()));
                                }
                            }
                        }
                    }
                    CallType::Unknown => self.untyped_call(callee, args, stmts),
                }
            }
            ExpressionKind::Assignment { target, operator, value } => {
                self.collect_calls(target, stmts);
                self.collect_calls(value, stmts);
                self.push_nested_assignment(target, *operator, value, stmts);
            }
            ExpressionKind::BinaryOp { left, right, .. } => {
                self.collect_calls(left, stmts);
                self.collect_calls(right, stmts);
            }
            ExpressionKind::UnaryOp { operator, operand } => {
                self.collect_calls(operand, stmts);
                if operator.mutates_operand() {
                    self.push_mutation(expr, *operator, operand, stmts);
                }
            }
            ExpressionKind::MemberAccess { object, .. } => self.collect_calls(object, stmts),
            ExpressionKind::IndexAccess { base, index } => {
                self.collect_calls(base, stmts);
                if let Some(idx) = index {
                    self.collect_calls(idx, stmts);
                }
            }
            ExpressionKind::Ternary { condition, true_expr, false_expr } => {
                self.collect_calls(condition, stmts);
                self.collect_calls(true_expr, stmts);
                self.collect_calls(false_expr, stmts);
            }
            ExpressionKind::Tuple { elements } => {
                for e in elements.iter().flatten() {
                    self.collect_calls(e, stmts);
                }
            }
            ExpressionKind::TypeCast { expression, .. } => self.collect_calls(expression, stmts),
            ExpressionKind::New { type_name, arguments, options, creates_contract } => {
                for option in options {
                    self.collect_calls(&option.value, stmts);
                }
                for arg in arguments {
                    self.collect_calls(arg, stmts);
                }
                if *creates_contract {
                    stmts.push(CfgStatement::ExternalCall {
                        target: type_name.clone(),
                        function: "constructor".into(),
                        span: None,
                        from_modifier: self.current_modifier.clone(),
                        resolved: None,
                        arguments: arguments.iter().map(expr_to_string).collect::<Vec<_>>().join(", "),
                        kind: CallKind::Creation,
                        contract: Some(type_name.clone()),
                        options: option_strings(options),
                        storage_args: Vec::new(),
                    });
                }
            }
            ExpressionKind::IndexRange { base, start, end } => {
                self.collect_calls(base, stmts);
                for e in [start, end].into_iter().flatten() {
                    self.collect_calls(e, stmts);
                }
            }
            _ => {}
        }
    }

    // A call whose kind solc fixed through the callee's function type.
    fn typed_call(
        &self,
        callee: &Expression,
        kind: CallKind,
        arguments: String,
        options: &[CallOption],
        storage_args: Vec<StorageArg>,
    ) -> CfgStatement {
        let from_modifier = self.current_modifier.clone();
        let decls = self.decls;
        let declared_in = |resolved: &Option<_>| {
            resolved.and_then(|id| decls?.lookup(id)).map(|t| t.contract.clone())
        };
        let mut contract = None;
        let (target, function, resolved) = match &callee.kind {
            ExpressionKind::Identifier { name, resolved } => (None, name.clone(), *resolved),
            ExpressionKind::MemberAccess { object, member, resolved, object_type } => {
                contract = object_type.as_deref().and_then(type_contract_name);
                let target = match &object.kind {
                    ExpressionKind::Identifier { name, .. } if name == "super" || name == "this" => name.clone(),
                    // a library function, named or bound through `using for`
                    _ if matches!(kind, CallKind::InternalLibrary | CallKind::LibraryDelegatecall) => {
                        object_type
                            .as_deref()
                            .filter(|t| t.starts_with("type("))
                            .and_then(type_contract_name)
                            .or_else(|| declared_in(resolved))
                            .unwrap_or_else(|| expr_to_string(object))
                    }
                    _ => expr_to_string(object),
                };
                (Some(target), member.clone(), *resolved)
            }
            _ => (None, expr_to_string(callee), None),
        };
        if kind.is_internal() {
            let declaration = resolved
                .and_then(|id| decls?.lookup(id))
                .filter(|t| !t.function.is_empty())
                .map(|t| (t.contract.clone(), t.function.clone()));
            CfgStatement::InternalCall {
                function,
                span: None,
                from_modifier,
                arguments,
                kind,
                target,
                resolved,
                declaration,
                storage_args,
            }
        } else {
            CfgStatement::ExternalCall {
                target: target.unwrap_or_default(),
                function,
                span: None,
                from_modifier,
                resolved,
                arguments,
                kind,
                contract,
                options: option_strings(options),
                storage_args,
            }
        }
    }

    // Models without solc function types: classify by syntax and names.
    fn untyped_call(&self, callee: &Expression, args: String, stmts: &mut Vec<CfgStatement>) {
        let from_modifier = &self.current_modifier;
        let internal = |function: String, target: Option<String>| CfgStatement::InternalCall {
            function,
            span: None,
            from_modifier: from_modifier.clone(),
            arguments: args.clone(),
            kind: CallKind::Internal,
            target,
            resolved: None,
            declaration: None,
            storage_args: Vec::new(),
        };
        let external = |target: String, function: String, resolved, kind| CfgStatement::ExternalCall {
            target,
            function,
            span: None,
            from_modifier: from_modifier.clone(),
            resolved,
            arguments: args.clone(),
            kind,
            contract: None,
            options: Vec::new(),
            storage_args: Vec::new(),
        };
        match &callee.kind {
            ExpressionKind::TypeMeta { .. } => {}
            ExpressionKind::Identifier { name, .. } => {
                if !crate::util::is_type_cast(name) && !crate::util::is_builtin_function(name) {
                    stmts.push(internal(name.clone(), None));
                }
            }
            ExpressionKind::MemberAccess { object, member, resolved, .. }
                if (member != "push" && member != "pop") || resolved.is_some() =>
            {
                match &object.kind {
                    ExpressionKind::Identifier { name, .. } if name == "super" => {
                        stmts.push(internal(member.clone(), Some("super".into())));
                    }
                    ExpressionKind::Identifier { name, .. } if name == "this" => {
                        stmts.push(external(name.clone(), member.clone(), *resolved, CallKind::ExternalSelf));
                    }
                    ExpressionKind::Identifier { name, .. }
                        if resolved.is_none() && crate::util::is_builtin_namespace(name) => {}
                    _ => stmts.push(external(expr_to_string(object), member.clone(), *resolved, CallKind::External)),
                }
            }
            // `push`/`pop` without types
            ExpressionKind::MemberAccess { object, .. } => {
                for place in self.place(object) {
                    stmts.push(storage_write(place, from_modifier.clone()));
                }
            }
            _ => stmts.push(internal(expr_to_string(callee), None)),
        }
    }
}

// Lines a block covers: its own code, or the modifier code when it has no other.
fn block_span(statements: &[CfgStatement]) -> Option<SourceSpan> {
    let origins: Vec<(SourceSpan, bool)> = statements
        .iter()
        .filter_map(|s| match s.origin() {
            (Some(span), modifier) => Some((span, modifier.is_none())),
            (None, _) => None,
        })
        .collect();
    let own = origins.iter().any(|(_, own)| *own);
    let mut spans = origins.into_iter().filter(|(_, o)| *o == own).map(|(span, _)| span);
    let first = spans.next()?;
    Some(spans.filter(|s| s.file_index == first.file_index).fold(first, |a, b| {
        let (start_line, start_col) = (a.start_line, a.start_col).min((b.start_line, b.start_col));
        let (end_line, end_col) = (a.end_line, a.end_col).max((b.end_line, b.end_col));
        SourceSpan { file_index: a.file_index, start_line, start_col, end_line, end_col }
    }))
}

fn storage_write(place: StorageRef, from_modifier: Option<String>) -> CfgStatement {
    CfgStatement::StateWrite { variable: place.path, span: None, from_modifier, param: place.param }
}

// A library function bound through `using for`: the receiver is its first argument.
fn is_attached(callee: &Expression, kind: CallKind) -> bool {
    matches!(kind, CallKind::InternalLibrary | CallKind::LibraryDelegatecall)
        && matches!(&callee.kind, ExpressionKind::MemberAccess { object_type: Some(t), .. } if !t.starts_with("type("))
}

// The arguments a call passes, the `using for` receiver first.
pub(crate) fn call_arguments<'e>(callee: &'e Expression, arguments: &'e [Expression], kind: CallKind) -> Vec<&'e Expression> {
    let mut out = Vec::with_capacity(arguments.len() + 1);
    if let (true, ExpressionKind::MemberAccess { object, .. }) = (is_attached(callee, kind), &callee.kind) {
        out.push(object.as_ref());
    }
    out.extend(arguments);
    out
}

fn collect_repoints<'a>(stmts: &'a [Statement], out: &mut Vec<(&'a Expression, &'a Expression)>) {
    for stmt in stmts {
        match &stmt.kind {
            StatementKind::ExpressionStmt { expression } => repoint(expression, out),
            StatementKind::If { then_body, else_body, .. } => {
                collect_repoints(then_body, out);
                if let Some(else_body) = else_body {
                    collect_repoints(else_body, out);
                }
            }
            StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                collect_repoints(statements, out);
            }
            StatementKind::For { init, increment, body, .. } => {
                if let Some(init) = init {
                    collect_repoints(std::slice::from_ref(init.as_ref()), out);
                }
                if let Some(incr) = increment {
                    repoint(incr, out);
                }
                collect_repoints(body, out);
            }
            StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => collect_repoints(body, out),
            StatementKind::TryCatch { clauses, .. } => {
                for clause in clauses {
                    collect_repoints(&clause.body, out);
                }
            }
            _ => {}
        }
    }
}

fn repoint<'a>(expr: &'a Expression, out: &mut Vec<(&'a Expression, &'a Expression)>) {
    if let ExpressionKind::Assignment { target, operator: AssignOperator::Assign, value } = &expr.kind {
        if matches!(target.kind, ExpressionKind::Identifier { .. }) {
            out.push((target, value));
        }
    }
}

// Values a function returns through `ret`: `return e` and assignments to the named variable.
fn collect_returned<'a>(stmts: &'a [Statement], ret: &Param, out: &mut Vec<&'a Expression>) {
    for stmt in stmts {
        match &stmt.kind {
            StatementKind::Return { value: Some(value) } => out.push(value),
            StatementKind::ExpressionStmt { expression } => {
                if let ExpressionKind::Assignment { target, operator: AssignOperator::Assign, value } = &expression.kind {
                    if let ExpressionKind::Identifier { name, resolved } = &target.kind {
                        let named = match (resolved, ret.id) {
                            (Some(a), Some(b)) => *a == b,
                            _ => !ret.name.is_empty() && *name == ret.name,
                        };
                        if named {
                            out.push(value);
                        }
                    }
                }
            }
            StatementKind::If { then_body, else_body, .. } => {
                collect_returned(then_body, ret, out);
                if let Some(else_body) = else_body {
                    collect_returned(else_body, ret, out);
                }
            }
            StatementKind::Block { statements } | StatementKind::UncheckedBlock { statements } => {
                collect_returned(statements, ret, out);
            }
            StatementKind::For { body, .. } | StatementKind::While { body, .. } | StatementKind::DoWhile { body, .. } => {
                collect_returned(body, ret, out);
            }
            _ => {}
        }
    }
}

fn option_strings(options: &[CallOption]) -> Vec<(String, String)> {
    options.iter().map(|o| (o.name.clone(), expr_to_string(&o.value))).collect()
}

// `type(library L)` / `type(contract C)` / `contract C` -> `L` / `C`
fn type_contract_name(object_type: &str) -> Option<String> {
    let inner = object_type
        .strip_prefix("type(")
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or(object_type);
    inner
        .strip_prefix("library ")
        .or_else(|| inner.strip_prefix("contract "))
        .or_else(|| inner.strip_prefix("interface "))
        .map(|name| name.strip_prefix("super ").unwrap_or(name).to_string())
}

// A call whose kind solc fixed through the callee's function type.

pub(crate) fn expr_to_string(expr: &Expression) -> String {
    match &expr.kind {
        ExpressionKind::Identifier { name, .. } => name.clone(),
        ExpressionKind::Literal { value, .. } => value.clone(),
        ExpressionKind::MemberAccess { object, member, .. } => {
            format!("{}.{}", expr_to_string(object), member)
        }
        ExpressionKind::FunctionCall { callee, arguments, options, .. } => {
            let args: Vec<String> = arguments.iter().map(expr_to_string).collect();
            if let ExpressionKind::TypeMeta { type_name } = &callee.kind {
                return format!("{type_name}({})", args.join(", "));
            }
            format!("{}{}({})", expr_to_string(callee), options_to_string(options), args.join(", "))
        }
        ExpressionKind::BinaryOp { left, operator, right } => {
            format!("{} {} {}", expr_to_string(left), operator.as_str(), expr_to_string(right))
        }
        ExpressionKind::UnaryOp { operator, operand } => {
            let (sym, postfix) = operator.format_parts();
            let s = expr_to_string(operand);
            if postfix { format!("{}{}", s, sym) } else { format!("{}{}", sym, s) }
        }
        ExpressionKind::IndexAccess { base, index } => {
            let idx = index.as_ref().map(|e| expr_to_string(e)).unwrap_or_default();
            format!("{}[{}]", expr_to_string(base), idx)
        }
        ExpressionKind::Assignment { target, operator, value } => {
            format!("{} {} {}", expr_to_string(target), operator.as_str(), expr_to_string(value))
        }
        ExpressionKind::Ternary { condition, true_expr, false_expr } => {
            format!("{} ? {} : {}", expr_to_string(condition), expr_to_string(true_expr), expr_to_string(false_expr))
        }
        ExpressionKind::TypeCast { type_name, expression } => {
            format!("{type_name}({})", expr_to_string(expression))
        }
        ExpressionKind::TypeMeta { type_name } => {
            format!("type({type_name})")
        }
        ExpressionKind::New { type_name, arguments, options, .. } => {
            let args: Vec<String> = arguments.iter().map(expr_to_string).collect();
            format!("new {type_name}{}({})", options_to_string(options), args.join(", "))
        }
        ExpressionKind::Tuple { elements } => {
            let parts: Vec<String> = elements
                .iter()
                .map(|e| e.as_ref().map(expr_to_string).unwrap_or_default())
                .collect();
            format!("({})", parts.join(", "))
        }
        ExpressionKind::IndexRange { base, start, end } => {
            let s = start.as_ref().map(|e| expr_to_string(e)).unwrap_or_default();
            let e = end.as_ref().map(|e| expr_to_string(e)).unwrap_or_default();
            format!("{}[{}:{}]", expr_to_string(base), s, e)
        }
    }
}

fn options_to_string(options: &[CallOption]) -> String {
    if options.is_empty() {
        return String::new();
    }
    let parts: Vec<String> =
        options.iter().map(|o| format!("{}: {}", o.name, expr_to_string(&o.value))).collect();
    format!("{{{}}}", parts.join(", "))
}
