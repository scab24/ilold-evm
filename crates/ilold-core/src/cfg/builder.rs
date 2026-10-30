use petgraph::stable_graph::NodeIndex;

use std::collections::HashSet;

use crate::model::contract::ContractDef;
use crate::model::expression::{AssignOperator, Expression, ExpressionKind, UnaryOperator};
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::statement::{Statement, StatementKind};

use super::error::CfgError;
use super::modifier::inline_modifiers;
use super::types::*;

pub struct CfgBuilder {
    graph: CfgGraph,
    next_block_id: usize,
    current_block: NodeIndex,
    // provenance is tagged on top-level statements
    current_modifier: Option<String>,
    state_vars: Vec<String>,
    storage_aliases: std::collections::HashMap<String, String>,
    // blocks left through break/continue; control never falls out of them
    closed: HashSet<NodeIndex>,
    loops: Vec<LoopFrame>,
}

// Jump targets of the innermost enclosing loop. Blocks that may stay unused
// (for increment, do-while condition and exit) are created on first use.
struct LoopFrame {
    continue_block: Option<NodeIndex>,
    continue_kind: BlockKind,
    continue_edge: BranchEdge,
    exit_block: Option<NodeIndex>,
}

impl CfgBuilder {
    pub fn build(function: &FunctionDef, contract: &ContractDef) -> Result<CfgGraph, CfgError> {
        Self::build_with_project(function, contract, None)
    }

    pub fn build_with_project(
        function: &FunctionDef,
        contract: &ContractDef,
        project: Option<&Project>,
    ) -> Result<CfgGraph, CfgError> {
        let mut builder = CfgBuilder {
            graph: CfgGraph::new(),
            next_block_id: 0,
            current_block: NodeIndex::new(0),
            current_modifier: None,
            state_vars: match project {
                Some(p) => p.inherited_state_vars(contract).into_iter().map(|sv| sv.name).collect(),
                None => contract.state_vars.iter().map(|sv| sv.name.clone()).collect(),
            },
            storage_aliases: std::collections::HashMap::new(),
            closed: HashSet::new(),
            loops: Vec::new(),
        };

        let entry = builder.add_block(BlockKind::Entry);
        builder.current_block = entry;

        let body = match &function.body {
            Some(body) => body.clone(),
            None => return Ok(builder.graph),
        };

        let tagged_body: Vec<super::modifier::TaggedStatement> = if function.modifiers.is_empty() {
            body
                .into_iter()
                .map(|s| super::modifier::TaggedStatement { stmt: s, provenance: None })
                .collect()
        } else {
            let modifier_defs: Vec<&_> = function
                .modifiers
                .iter()
                .filter_map(|mref| {
                    if let Some(proj) = project {
                        proj.resolve_modifier(contract, &mref.name)
                    } else {
                        contract.modifiers.iter().find(|m| m.name == mref.name)
                    }
                })
                .collect();
            if modifier_defs.len() == function.modifiers.len() {
                inline_modifiers(&body, &modifier_defs)?
            } else {
                body
                    .into_iter()
                    .map(|s| super::modifier::TaggedStatement { stmt: s, provenance: None })
                    .collect()
            }
        };

        for tagged in &tagged_body {
            builder.current_modifier = tagged.provenance.clone();
            builder.process_statement(&tagged.stmt);
        }
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

        Ok(builder.graph)
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
        })
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge: BranchEdge) {
        self.graph.add_edge(from, to, edge);
    }

    fn resolve_alias(&self, var: &str) -> String {
        let base = crate::util::target_base_name(var);
        if var.len() > base.len() {
            if let Some(target) = self.storage_aliases.get(base) {
                return format!("{}{}", target, &var[base.len()..]);
            }
        }
        var.to_string()
    }

    fn add_stmt_to_current(&mut self, mut stmt: CfgStatement) {
        match &mut stmt {
            CfgStatement::StateWrite { variable, .. } => {
                *variable = self.resolve_alias(variable);
                let base = crate::util::target_base_name(variable);
                if !self.state_vars.iter().any(|n| n == base) {
                    return;
                }
            }
            CfgStatement::Assignment { target, .. } => {
                *target = self.resolve_alias(target);
            }
            _ => {}
        }
        if let Some(block) = self.graph.node_weight_mut(self.current_block) {
            block.statements.push(stmt);
        }
    }

    fn is_state_target(&self, var: &str) -> bool {
        let base = crate::util::target_base_name(var);
        self.state_vars.iter().any(|n| n == base)
    }

    fn process_statement(&mut self, stmt: &Statement) {
        // code after revert/break/continue never runs
        if self.closed.contains(&self.current_block)
            || self.graph[self.current_block].kind == BlockKind::Revert
        {
            return;
        }
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
            StatementKind::Revert { arguments, .. } => {
                self.process_revert(arguments);
            }
            StatementKind::Emit { event_name, arguments } => {
                for arg in arguments {
                    self.add_effects(arg);
                }
                let from_modifier = self.current_modifier.clone();
                let args = arguments.iter().map(|a| expr_to_string(a)).collect::<Vec<_>>().join(", ");
                self.add_stmt_to_current(CfgStatement::EmitEvent {
                    event: event_name.clone(),
                    span: None,
                    from_modifier,
                    arguments: args,
                });
            }
            StatementKind::Block { statements } => {
                for s in statements {
                    self.process_statement(s);
                }
            }
            StatementKind::UncheckedBlock { statements } => {
                for s in statements {
                    self.process_statement(s);
                }
            }
            StatementKind::ExpressionStmt { expression } => {
                self.process_expression_stmt(expression);
            }
            StatementKind::VariableDeclaration { name, initial_value, is_storage_ref, .. } => {
                if let Some(val) = initial_value {
                    if *is_storage_ref {
                        let init = self.resolve_alias(&expr_to_string(val));
                        if self.state_vars.iter().any(|n| n == crate::util::target_base_name(&init)) {
                            self.storage_aliases.insert(name.clone(), init);
                        }
                    }
                    let from_modifier = self.current_modifier.clone();
                    self.add_effects(val);
                    self.add_stmt_to_current(CfgStatement::Assignment {
                        target: name.clone(),
                        value: expr_to_string(val),
                        operator: crate::model::expression::AssignOperator::Assign,
                        span: None,
                        from_modifier,
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
            StatementKind::Placeholder => {}
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

    // calls and mutations of an expression whose value is consumed elsewhere
    fn add_effects(&mut self, expr: &Expression) {
        let from_modifier = self.current_modifier.clone();
        let mut stmts = Vec::new();
        collect_calls(expr, &mut stmts, &from_modifier);
        self.collect_mutations(expr, &mut stmts, &from_modifier);
        for s in stmts {
            self.add_stmt_to_current(s);
        }
    }

    fn process_revert(&mut self, arguments: &[Expression]) {
        for arg in arguments {
            self.add_effects(arg);
        }
        let revert = self.add_block(BlockKind::Revert);
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
        self.current_block = then_block;
        for s in then_body {
            self.process_statement(s);
        }
        let then_end = self.current_block;

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
            for s in self.classify_expression(incr) {
                self.add_stmt_to_current(s);
            }
            self.add_edge(incr_block, cond_block, BranchEdge::LoopBack);
        }

        self.current_block = exit_block;
    }

    fn process_while(&mut self, condition: &Expression, body: &[Statement]) {
        let cond_str = expr_to_string(condition);

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
    }

    fn process_do_while(&mut self, body: &[Statement], condition: &Expression) {
        let cond_str = expr_to_string(condition);

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
    }

    fn process_return(&mut self, value: Option<&Expression>) {
        if let Some(expr) = value {
            for s in self.classify_expression(expr) {
                self.add_stmt_to_current(s);
            }
        }
        let ret = self.add_block(BlockKind::Return);
        if let Some(expr) = value {
            if let Some(block) = self.graph.node_weight_mut(ret) {
                block.return_value = Some(expr_to_string(expr));
            }
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
        for s in self.classify_expression(expression) {
            self.add_stmt_to_current(s);
        }

        let join = self.add_block(BlockKind::Normal);

        for (i, clause) in clauses.iter().enumerate() {
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
            }
        }

        self.current_block = join;
    }

    fn process_expression_stmt(&mut self, expr: &Expression) {
        if let ExpressionKind::FunctionCall { callee, arguments } = &expr.kind {
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
                        self.process_revert(arguments);
                        return;
                    }
                    _ => {}
                }
            }
        }

        for s in self.classify_expression(expr) {
            self.add_stmt_to_current(s);
        }
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

        let before = self.current_block;

        let next = self.add_block(BlockKind::Normal);
        self.add_edge(
            before,
            next,
            BranchEdge::ConditionalTrue { condition: cond_str.clone() },
        );

        let revert = self.add_block(BlockKind::Revert);
        self.add_edge(
            before,
            revert,
            BranchEdge::ConditionalFalse { condition: cond_str },
        );

        self.current_block = next;
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

        let before = self.current_block;

        let next = self.add_block(BlockKind::Normal);
        self.add_edge(
            before,
            next,
            BranchEdge::ConditionalTrue { condition: cond_str.clone() },
        );

        let revert = self.add_block(BlockKind::Revert);
        self.add_edge(
            before,
            revert,
            BranchEdge::ConditionalFalse { condition: cond_str },
        );

        self.current_block = next;
    }

    fn block_is_terminal(&self, idx: NodeIndex) -> bool {
        let kind = self.graph[idx].kind;
        kind == BlockKind::Return || kind == BlockKind::Revert || self.closed.contains(&idx)
    }

    fn classify_expression(&self, expr: &Expression) -> Vec<CfgStatement> {
        let from_modifier = &self.current_modifier;
        let mut stmts = Vec::new();
        collect_calls(expr, &mut stmts, from_modifier);

        if let ExpressionKind::Assignment { target, operator, value } = &expr.kind {
            stmts.push(CfgStatement::Assignment {
                target: expr_to_string(target),
                value: expr_to_string(value),
                operator: *operator,
                span: None,
                from_modifier: from_modifier.clone(),
            });
        }

        self.collect_mutations(expr, &mut stmts, from_modifier);

        stmts
    }

    // ++/--/delete: a state write on storage, a local assignment otherwise
    fn collect_mutations(&self, expr: &Expression, stmts: &mut Vec<CfgStatement>, from_modifier: &Option<String>) {
        match &expr.kind {
            ExpressionKind::UnaryOp { operator, operand } if operator.mutates_operand() => {
                let variable = expr_to_string(operand);
                if self.is_state_target(&self.resolve_alias(&variable)) {
                    stmts.push(CfgStatement::StateWrite {
                        variable,
                        span: None,
                        from_modifier: from_modifier.clone(),
                    });
                } else {
                    let (operator, value) = match operator {
                        UnaryOperator::PreIncrement | UnaryOperator::PostIncrement => {
                            (AssignOperator::AddAssign, "1".to_string())
                        }
                        UnaryOperator::PreDecrement | UnaryOperator::PostDecrement => {
                            (AssignOperator::SubAssign, "1".to_string())
                        }
                        _ => (AssignOperator::Assign, expr_to_string(expr)),
                    };
                    stmts.push(CfgStatement::Assignment {
                        target: variable,
                        value,
                        operator,
                        span: None,
                        from_modifier: from_modifier.clone(),
                    });
                }
            }
            ExpressionKind::Assignment { value, .. } => self.collect_mutations(value, stmts, from_modifier),
            ExpressionKind::BinaryOp { left, right, .. } => {
                self.collect_mutations(left, stmts, from_modifier);
                self.collect_mutations(right, stmts, from_modifier);
            }
            _ => {}
        }
    }
}

fn collect_calls(expr: &Expression, stmts: &mut Vec<CfgStatement>, from_modifier: &Option<String>) {
    match &expr.kind {
        ExpressionKind::FunctionCall { callee, arguments } => {
            // evaluation order: callee object, arguments, then the call itself
            match &callee.kind {
                ExpressionKind::Identifier { .. } | ExpressionKind::TypeMeta { .. } => {}
                ExpressionKind::MemberAccess { object, .. } => collect_calls(object, stmts, from_modifier),
                _ => collect_calls(callee, stmts, from_modifier),
            }
            for arg in arguments {
                collect_calls(arg, stmts, from_modifier);
            }
            let args = arguments.iter().map(|a| expr_to_string(a)).collect::<Vec<_>>().join(", ");
            match &callee.kind {
                ExpressionKind::TypeMeta { .. } => {}
                ExpressionKind::Identifier { name, .. } => {
                    if !crate::util::is_type_cast(name) {
                        stmts.push(CfgStatement::InternalCall {
                            function: name.clone(),
                            span: None,
                            from_modifier: from_modifier.clone(),
                            arguments: args.clone(),
                        });
                    }
                }
                ExpressionKind::MemberAccess { object, member, resolved }
                    if (member != "push" && member != "pop") || resolved.is_some() =>
                {
                    if let ExpressionKind::Identifier { name, .. } = &object.kind {
                        if name == "this" || name == "super" {
                            stmts.push(CfgStatement::InternalCall {
                                function: member.clone(),
                                span: None,
                                from_modifier: from_modifier.clone(),
                                arguments: args.clone(),
                            });
                        } else {
                            stmts.push(CfgStatement::ExternalCall {
                                target: name.clone(),
                                function: member.clone(),
                                span: None,
                                from_modifier: from_modifier.clone(),
                                resolved: *resolved,
                                arguments: args.clone(),
                            });
                        }
                    } else {
                        stmts.push(CfgStatement::ExternalCall {
                            target: expr_to_string(object),
                            function: member.clone(),
                            span: None,
                            from_modifier: from_modifier.clone(),
                            resolved: *resolved,
                            arguments: args.clone(),
                        });
                    }
                }
                ExpressionKind::MemberAccess { object, .. } => {
                    stmts.push(CfgStatement::StateWrite {
                        variable: expr_to_string(object),
                        span: None,
                        from_modifier: from_modifier.clone(),
                    });
                }
                _ => {
                    stmts.push(CfgStatement::InternalCall {
                        function: expr_to_string(callee),
                        span: None,
                        from_modifier: from_modifier.clone(),
                        arguments: args.clone(),
                    });
                }
            }
        }
        ExpressionKind::Assignment { target, value, .. } => {
            collect_calls(target, stmts, from_modifier);
            collect_calls(value, stmts, from_modifier);
        }
        ExpressionKind::BinaryOp { left, right, .. } => {
            collect_calls(left, stmts, from_modifier);
            collect_calls(right, stmts, from_modifier);
        }
        ExpressionKind::UnaryOp { operand, .. } => {
            collect_calls(operand, stmts, from_modifier);
        }
        ExpressionKind::MemberAccess { object, .. } => {
            collect_calls(object, stmts, from_modifier);
        }
        ExpressionKind::IndexAccess { base, index } => {
            collect_calls(base, stmts, from_modifier);
            if let Some(idx) = index {
                collect_calls(idx, stmts, from_modifier);
            }
        }
        ExpressionKind::Ternary { condition, true_expr, false_expr } => {
            collect_calls(condition, stmts, from_modifier);
            collect_calls(true_expr, stmts, from_modifier);
            collect_calls(false_expr, stmts, from_modifier);
        }
        ExpressionKind::Tuple { elements } => {
            for e in elements.iter().flatten() {
                collect_calls(e, stmts, from_modifier);
            }
        }
        ExpressionKind::TypeCast { expression, .. } => {
            collect_calls(expression, stmts, from_modifier);
        }
        ExpressionKind::New { arguments, .. } => {
            for arg in arguments {
                collect_calls(arg, stmts, from_modifier);
            }
        }
        ExpressionKind::IndexRange { base, start, end } => {
            collect_calls(base, stmts, from_modifier);
            if let Some(s) = start {
                collect_calls(s, stmts, from_modifier);
            }
            if let Some(e) = end {
                collect_calls(e, stmts, from_modifier);
            }
        }
        _ => {}
    }
}

fn expr_to_string(expr: &Expression) -> String {
    match &expr.kind {
        ExpressionKind::Identifier { name, .. } => name.clone(),
        ExpressionKind::Literal { value, .. } => value.clone(),
        ExpressionKind::MemberAccess { object, member, .. } => {
            format!("{}.{}", expr_to_string(object), member)
        }
        ExpressionKind::FunctionCall { callee, arguments } => {
            if let ExpressionKind::TypeMeta { type_name } = &callee.kind {
                let args: Vec<String> = arguments.iter().map(expr_to_string).collect();
                return format!("{type_name}({})", args.join(", "));
            }
            let args: Vec<String> = arguments.iter().map(expr_to_string).collect();
            format!("{}({})", expr_to_string(callee), args.join(", "))
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
        ExpressionKind::New { type_name, arguments } => {
            let args: Vec<String> = arguments.iter().map(expr_to_string).collect();
            format!("new {type_name}({})", args.join(", "))
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

