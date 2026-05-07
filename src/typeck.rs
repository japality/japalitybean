use crate::ast::*;
use crate::diag::{fix, Diagnostic, Phase};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct FunctionSig {
    inputs: Vec<StickyBinding>,
    output: StickyBinding,
}

pub fn check_program(program: &Program) -> Vec<Diagnostic> {
    let mut checker = Checker::new(program);
    checker.check_program(program);
    checker.diags
}

struct Checker {
    diags: Vec<Diagnostic>,
    funcs: HashMap<String, FunctionSig>,
    scopes: Vec<HashMap<String, Type>>,
    used_names: HashSet<String>,
}

impl Checker {
    fn new(program: &Program) -> Self {
        let mut funcs = HashMap::new();
        install_builtins(&mut funcs);
        for func in &program.functions {
            funcs.insert(
                func.kind.name.clone(),
                FunctionSig {
                    inputs: func.kind.inputs.clone(),
                    output: func.kind.output.clone(),
                },
            );
        }
        Self {
            diags: Vec::new(),
            funcs,
            scopes: Vec::new(),
            used_names: HashSet::new(),
        }
    }

    fn check_program(&mut self, program: &Program) {
        let mut seen_funcs: HashMap<&str, &Node<FunctionDecl>> = HashMap::new();
        for func in &program.functions {
            if let Some(prev) = seen_funcs.insert(&func.kind.name, func) {
                self.diags.push(Diagnostic::error(
                    "E_MOD001",
                    Phase::Resolve,
                    Some(func.id.clone()),
                    func.span,
                    "duplicate function name",
                    Some("unique function name".to_string()),
                    Some(func.kind.name.clone()),
                    fix("rename", format!("@func {}_2", func.kind.name)),
                ));
                self.diags.push(Diagnostic::warning(
                    "W_MOD001",
                    Phase::Resolve,
                    Some(prev.id.clone()),
                    prev.span,
                    "previous function definition with the same name",
                    None,
                    Some(prev.kind.name.clone()),
                    fix("rename", format!("@func {}_1", prev.kind.name)),
                ));
            }
        }

        for func in &program.functions {
            self.check_function(func);
        }
    }

    fn check_function(&mut self, func: &Node<FunctionDecl>) {
        self.scopes.clear();
        self.used_names.clear();
        self.push_scope();
        for input in &func.kind.inputs {
            self.declare(input, func.id.clone(), func.span);
        }
        self.declare(&func.kind.output, func.id.clone(), func.span);

        let mut output_assigned = false;
        self.check_statements(&func.kind.body, &func.kind.output, 1, false, &mut output_assigned);

        for input in &func.kind.inputs {
            if !self.used_names.contains(&input.name) {
                self.diags.push(Diagnostic::warning(
                    "W_INTENT002",
                    Phase::Resolve,
                    Some(func.id.clone()),
                    func.span,
                    "input declared in @in is never referenced",
                    Some("referenced input".to_string()),
                    Some(input.name.clone()),
                    fix("delete", format!("@in {}::{}", input.name, input.ty)),
                ));
            }
        }

        if !output_assigned && func.kind.output.ty != Type::Unit {
            self.diags.push(Diagnostic::error(
                "E_INTENT003",
                Phase::Resolve,
                Some(func.id.clone()),
                func.span,
                "@out binding is not assigned by set or return on the checked path",
                Some(format!("set {} = <expr> or return <expr>", func.kind.output.name)),
                None,
                fix("insert", format!("set {} = <value>", func.kind.output.name)),
            ));
        }
        self.pop_scope();
    }

    fn check_statements(
        &mut self,
        stmts: &[Node<Statement>],
        output: &StickyBinding,
        depth: usize,
        in_loop: bool,
        output_assigned: &mut bool,
    ) {
        if depth > 3 {
            if let Some(stmt) = stmts.first() {
                self.diags.push(Diagnostic::error(
                    "E_FLAT002",
                    Phase::Resolve,
                    Some(stmt.id.clone()),
                    stmt.span,
                    "static block nesting exceeds JapalityBean v1 maximum depth",
                    Some("nesting depth <= 3".to_string()),
                    Some(depth.to_string()),
                    fix("insert", "@func extracted_helper"),
                ));
            }
        }

        for stmt in stmts {
            match &stmt.kind {
                Statement::Let(let_stmt) => {
                    let actual = self.infer_expr(&let_stmt.value, Some(&let_stmt.binding.ty));
                    self.expect_type(
                        &let_stmt.binding.ty,
                        &actual,
                        &let_stmt.value.id,
                        let_stmt.value.span,
                        "sticky-type mismatch on let binding",
                    );
                    self.declare(&let_stmt.binding, stmt.id.clone(), stmt.span);
                }
                Statement::Set(set_stmt) => {
                    let expected = self.lookup(&set_stmt.target, stmt);
                    let actual = self.infer_expr(&set_stmt.value, expected.as_ref());
                    if let Some(expected) = expected {
                        self.expect_type(
                            &expected,
                            &actual,
                            &set_stmt.value.id,
                            set_stmt.value.span,
                            "assignment type does not match target binding",
                        );
                    }
                    if set_stmt.target == output.name {
                        *output_assigned = true;
                    }
                }
                Statement::Return(ret_stmt) => {
                    let actual = if let Some(value) = &ret_stmt.value {
                        self.infer_expr(value, Some(&output.ty))
                    } else {
                        Type::Unit
                    };
                    self.expect_type(
                        &output.ty,
                        &actual,
                        &stmt.id,
                        stmt.span,
                        "return type does not match @out binding",
                    );
                    *output_assigned = true;
                }
                Statement::Loop(loop_block) => {
                    let collection_ty = self.lookup(&loop_block.collection, stmt);
                    match collection_ty {
                        Some(Type::Vector(inner)) => self.expect_type(
                            &loop_block.item.ty,
                            &inner,
                            &stmt.id,
                            stmt.span,
                            "loop item type does not match collection element type",
                        ),
                        Some(other) => self.diags.push(Diagnostic::error(
                            "E_TYPE002",
                            Phase::Type,
                            Some(stmt.id.clone()),
                            stmt.span,
                            "loop collection must be Vector<T>",
                            Some("Vector<T>".to_string()),
                            Some(other.to_string()),
                            fix("replace", format!("Vector<{}>", loop_block.item.ty)),
                        )),
                        None => {}
                    }
                    self.push_scope();
                    self.declare(&loop_block.item, stmt.id.clone(), stmt.span);
                    self.check_statements(
                        &loop_block.body,
                        output,
                        depth + 1,
                        true,
                        output_assigned,
                    );
                    self.pop_scope();
                }
                Statement::Condition(cond) => {
                    let cond_ty = self.infer_expr(&cond.guard.kind.cond, Some(&Type::Bool));
                    self.expect_type(
                        &Type::Bool,
                        &cond_ty,
                        &cond.guard.kind.cond.id,
                        cond.guard.kind.cond.span,
                        "condition guard must evaluate to bool",
                    );
                    self.check_guard_action(
                        &cond.guard.kind.action,
                        output,
                        in_loop,
                        output_assigned,
                        &cond.guard.id,
                        cond.guard.span,
                    );
                }
                Statement::Expr(expr) => {
                    self.infer_expr(expr, None);
                }
            }
        }
    }

    fn check_guard_action(
        &mut self,
        action: &GuardAction,
        output: &StickyBinding,
        in_loop: bool,
        output_assigned: &mut bool,
        node_id: &str,
        span: Span,
    ) {
        match action {
            GuardAction::Continue | GuardAction::Break if !in_loop => {
                self.diags.push(Diagnostic::error(
                    "E_FLOW001",
                    Phase::Resolve,
                    Some(node_id.to_string()),
                    span,
                    "@continue and @break must be inside @loop",
                    Some("@loop ... @end loop".to_string()),
                    Some("guard action outside loop".to_string()),
                    fix("replace", "return <expr>"),
                ));
            }
            GuardAction::Continue | GuardAction::Break => {}
            GuardAction::Return(expr) => {
                let actual = self.infer_expr(expr, Some(&output.ty));
                self.expect_type(
                    &output.ty,
                    &actual,
                    &expr.id,
                    expr.span,
                    "guard return type does not match @out binding",
                );
                *output_assigned = true;
            }
            GuardAction::SetAssign { target, value } => {
                let expected = self.lookup_by_name(target, node_id.to_string(), span);
                let actual = self.infer_expr(value, expected.as_ref());
                if let Some(expected) = expected {
                    self.expect_type(
                        &expected,
                        &actual,
                        &value.id,
                        value.span,
                        "guard assignment type does not match target binding",
                    );
                }
                if target == &output.name {
                    *output_assigned = true;
                }
            }
        }
    }

    fn infer_expr(&mut self, expr: &Node<Expr>, expected: Option<&Type>) -> Type {
        match &expr.kind {
            Expr::Lit(Literal::Int(_)) => match expected {
                Some(ty) if ty.is_numeric() => ty.clone(),
                _ => Type::I64,
            },
            Expr::Lit(Literal::Float(_)) => match expected {
                Some(ty) if matches!(*ty, Type::F32 | Type::F64) => ty.clone(),
                _ => Type::F64,
            },
            Expr::Lit(Literal::Str(_)) => Type::String,
            Expr::Lit(Literal::Bool(_)) => Type::Bool,
            Expr::Ident(name) => self.lookup_by_name(name, expr.id.clone(), expr.span).unwrap_or(Type::Unknown),
            Expr::Paren(inner) => self.infer_expr(inner, expected),
            Expr::Unary { op, operand } => {
                let ty = self.infer_expr(operand, expected);
                match op {
                    UnOp::Neg if ty.is_numeric() => ty,
                    UnOp::Not if ty == Type::Bool => Type::Bool,
                    UnOp::Neg => {
                        self.diags.push(Diagnostic::error(
                            "E_TYPE002",
                            Phase::Type,
                            Some(expr.id.clone()),
                            expr.span,
                            "unary - requires numeric operand",
                            Some("numeric".to_string()),
                            Some(ty.to_string()),
                            fix("replace", "0"),
                        ));
                        Type::Unknown
                    }
                    UnOp::Not => {
                        self.diags.push(Diagnostic::error(
                            "E_TYPE002",
                            Phase::Type,
                            Some(expr.id.clone()),
                            expr.span,
                            "unary ! requires bool operand",
                            Some("bool".to_string()),
                            Some(ty.to_string()),
                            fix("replace", "true"),
                        ));
                        Type::Unknown
                    }
                }
            }
            Expr::Binary { op, lhs, rhs } => self.infer_binary(*op, lhs, rhs, expr),
            Expr::Call { callee, args } => self.infer_call(callee, args, expr),
            Expr::Cast { expr: inner, target } => {
                let source = self.infer_expr(inner, None);
                if is_valid_cast(&source, target) {
                    target.clone()
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE003",
                        Phase::Type,
                        Some(expr.id.clone()),
                        expr.span,
                        "explicit cast is not valid for these types",
                        Some(format!("numeric as::<{}>", target)),
                        Some(source.to_string()),
                        fix("replace", format!("<{} expression>", target)),
                    ));
                    Type::Unknown
                }
            }
            Expr::Ctor(ctor) => self.infer_ctor(ctor, expected, expr),
        }
    }

    fn infer_ctor(
        &mut self,
        ctor: &ConstructorExpr,
        expected: Option<&Type>,
        expr: &Node<Expr>,
    ) -> Type {
        match ctor {
            ConstructorExpr::Some(value) => match expected {
                Some(Type::Option(inner)) => {
                    let actual = self.infer_expr(value, Some(inner));
                    self.expect_type(inner, &actual, &value.id, value.span, "Some payload type mismatch");
                    Type::Option(inner.clone())
                }
                _ => Type::Option(Box::new(self.infer_expr(value, None))),
            },
            ConstructorExpr::None => match expected {
                Some(Type::Option(inner)) => Type::Option(inner.clone()),
                _ => {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE005",
                        Phase::Type,
                        Some(expr.id.clone()),
                        expr.span,
                        "None requires Option<T> context",
                        Some("Option<T>".to_string()),
                        Some("None without expected type".to_string()),
                        fix("annotate_type", "let value::Option<i32> = None"),
                    ));
                    Type::Unknown
                }
            },
            ConstructorExpr::Ok(value) => match expected {
                Some(Type::Result(ok, err)) => {
                    let actual = self.infer_expr(value, Some(ok));
                    self.expect_type(ok, &actual, &value.id, value.span, "Ok payload type mismatch");
                    Type::Result(ok.clone(), err.clone())
                }
                _ => Type::Result(Box::new(self.infer_expr(value, None)), Box::new(Type::Unknown)),
            },
            ConstructorExpr::Err(value) => match expected {
                Some(Type::Result(ok, err)) => {
                    let actual = self.infer_expr(value, Some(err));
                    self.expect_type(err, &actual, &value.id, value.span, "Err payload type mismatch");
                    Type::Result(ok.clone(), err.clone())
                }
                _ => Type::Result(Box::new(Type::Unknown), Box::new(self.infer_expr(value, None))),
            },
        }
    }

    fn infer_binary(
        &mut self,
        op: BinOp,
        lhs: &Node<Expr>,
        rhs: &Node<Expr>,
        full: &Node<Expr>,
    ) -> Type {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let left = self.infer_expr(lhs, None);
                let right = self.infer_expr(rhs, Some(&left));
                if is_wrapped_type(&left) || is_wrapped_type(&right) {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE004",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "wrapped Option/Result values cannot be used as arithmetic operands",
                        Some("unwrapped numeric value".to_string()),
                        Some(format!("{left}, {right}")),
                        fix("replace", "0"),
                    ));
                    return Type::Unknown;
                }
                if left.is_numeric() && right.is_numeric() && same_type(&left, &right) {
                    left
                } else if left.is_numeric() && right.is_numeric() {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE003",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "implicit numeric widening is not allowed",
                        Some(format!("{} as::<{}>", right, left)),
                        Some(format!("{left}, {right}")),
                        fix("replace", format!("<expr> as::<{}>", left)),
                    ));
                    Type::Unknown
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE002",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "arithmetic operands must have the same numeric type",
                        Some(left.to_string()),
                        Some(right.to_string()),
                        fix("replace", "0"),
                    ));
                    Type::Unknown
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let left = self.infer_expr(lhs, None);
                let right = self.infer_expr(rhs, Some(&left));
                if is_wrapped_type(&left) || is_wrapped_type(&right) {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE004",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "wrapped Option/Result values cannot be compared without unwrapping",
                        Some("unwrapped numeric value".to_string()),
                        Some(format!("{left}, {right}")),
                        fix("replace", "true"),
                    ));
                    return Type::Unknown;
                }
                if left.is_numeric() && right.is_numeric() && same_type(&left, &right) {
                    Type::Bool
                } else if left.is_numeric() && right.is_numeric() {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE003",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "implicit numeric widening is not allowed",
                        Some(format!("{} as::<{}>", right, left)),
                        Some(format!("{left}, {right}")),
                        fix("replace", format!("<expr> as::<{}>", left)),
                    ));
                    Type::Unknown
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE002",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "comparison operands must have the same numeric type",
                        Some(left.to_string()),
                        Some(right.to_string()),
                        fix("replace", "true"),
                    ));
                    Type::Unknown
                }
            }
            BinOp::Eq | BinOp::Neq => {
                let left = self.infer_expr(lhs, None);
                let right = self.infer_expr(rhs, Some(&left));
                if is_wrapped_type(&left) || is_wrapped_type(&right) {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE004",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "wrapped Option/Result values cannot be compared without unwrapping",
                        Some("unwrapped value".to_string()),
                        Some(format!("{left}, {right}")),
                        fix("replace", "true"),
                    ));
                    return Type::Unknown;
                }
                if same_type(&left, &right) {
                    Type::Bool
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE002",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "equality operands must have the same type",
                        Some(left.to_string()),
                        Some(right.to_string()),
                        fix("replace", "true"),
                    ));
                    Type::Unknown
                }
            }
            BinOp::And | BinOp::Or => {
                let left = self.infer_expr(lhs, Some(&Type::Bool));
                let right = self.infer_expr(rhs, Some(&Type::Bool));
                if left == Type::Bool && right == Type::Bool {
                    Type::Bool
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_TYPE002",
                        Phase::Type,
                        Some(full.id.clone()),
                        full.span,
                        "logical operands must be bool",
                        Some("bool".to_string()),
                        Some(format!("{left}, {right}")),
                        fix("replace", "true"),
                    ));
                    Type::Unknown
                }
            }
        }
    }

    fn infer_call(&mut self, callee: &str, args: &[Node<Expr>], expr: &Node<Expr>) -> Type {
        let Some(sig) = self.funcs.get(callee).cloned() else {
            self.diags.push(Diagnostic::error(
                "E_NAME001",
                Phase::Resolve,
                Some(expr.id.clone()),
                expr.span,
                "unknown function",
                Some("declared @func".to_string()),
                Some(callee.to_string()),
                fix("insert", format!("@func {callee}")),
            ));
            return Type::Unknown;
        };
        if args.len() != sig.inputs.len() {
            self.diags.push(Diagnostic::error(
                "E_CALL001",
                Phase::Type,
                Some(expr.id.clone()),
                expr.span,
                "function argument count mismatch",
                Some(sig.inputs.len().to_string()),
                Some(args.len().to_string()),
                fix("replace", format!("{callee}(...)")),
            ));
            return sig.output.ty;
        }
        for (arg, param) in args.iter().zip(sig.inputs.iter()) {
            let actual = self.infer_expr(arg, Some(&param.ty));
            self.expect_type(
                &param.ty,
                &actual,
                &arg.id,
                arg.span,
                "function argument type mismatch",
            );
        }
        sig.output.ty
    }

    fn expect_type(
        &mut self,
        expected: &Type,
        actual: &Type,
        node_id: &str,
        span: Span,
        message: &str,
    ) {
        if expected == &Type::Unknown || actual == &Type::Unknown || same_type(expected, actual) {
            return;
        }
        let (code, message, fix_strategy, patch) = if expected.is_numeric() && actual.is_numeric() {
            (
                "E_TYPE003",
                "implicit numeric widening is not allowed; use as::<T>",
                "replace",
                format!("<{} expression>", expected),
            )
        } else {
            (
                "E_TYPE002",
                message,
                "replace",
                format!("<{} expression>", expected),
            )
        };
        self.diags.push(Diagnostic::error(
            code,
            Phase::Type,
            Some(node_id.to_string()),
            span,
            message,
            Some(expected.to_string()),
            Some(actual.to_string()),
            fix(fix_strategy, patch),
        ));
    }

    fn declare(&mut self, binding: &StickyBinding, node_id: String, span: Span) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if scope.contains_key(&binding.name) {
            self.diags.push(Diagnostic::error(
                "E_NAME002",
                Phase::Resolve,
                Some(node_id),
                span,
                "identifier redeclared in the same scope",
                Some("unique binding name".to_string()),
                Some(binding.name.clone()),
                fix("rename", format!("{}2::{}", binding.name, binding.ty)),
            ));
        } else {
            scope.insert(binding.name.clone(), binding.ty.clone());
        }
    }

    fn lookup(&mut self, name: &str, stmt: &Node<Statement>) -> Option<Type> {
        self.lookup_by_name(name, stmt.id.clone(), stmt.span)
    }

    fn lookup_by_name(&mut self, name: &str, node_id: String, span: Span) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                self.used_names.insert(name.to_string());
                return Some(ty.clone());
            }
        }
        self.diags.push(Diagnostic::error(
            "E_NAME001",
            Phase::Resolve,
            Some(node_id),
            span,
            "unknown identifier",
            Some("declared binding".to_string()),
            Some(name.to_string()),
            fix("insert", format!("let {name}::i32 = 0")),
        ));
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

fn same_type(a: &Type, b: &Type) -> bool {
    a == b || a == &Type::Unknown || b == &Type::Unknown
}

fn is_valid_cast(source: &Type, target: &Type) -> bool {
    same_type(source, target) || (source.is_numeric() && target.is_numeric())
}

fn is_wrapped_type(ty: &Type) -> bool {
    matches!(ty, Type::Option(_) | Type::Result(_, _))
}

fn install_builtins(funcs: &mut HashMap<String, FunctionSig>) {
    builtin(funcs, "debug_i32", vec![binding("value", Type::I32)], binding("result", Type::Unit));
    builtin(
        funcs,
        "debug_string",
        vec![binding("value", Type::String)],
        binding("result", Type::Unit),
    );
    builtin(funcs, "abs_i32", vec![binding("value", Type::I32)], binding("result", Type::I32));
    builtin(funcs, "is_even_i32", vec![binding("value", Type::I32)], binding("result", Type::Bool));
    builtin(
        funcs,
        "max_i32",
        vec![binding("left", Type::I32), binding("right", Type::I32)],
        binding("result", Type::I32),
    );
    builtin(
        funcs,
        "vector_len_i32",
        vec![binding("items", Type::Vector(Box::new(Type::I32)))],
        binding("result", Type::I64),
    );
    builtin(
        funcs,
        "vector_i32_3",
        vec![
            binding("first", Type::I32),
            binding("second", Type::I32),
            binding("third", Type::I32),
        ],
        binding("result", Type::Vector(Box::new(Type::I32))),
    );
}

fn builtin(
    funcs: &mut HashMap<String, FunctionSig>,
    name: &str,
    inputs: Vec<StickyBinding>,
    output: StickyBinding,
) {
    funcs.insert(name.to_string(), FunctionSig { inputs, output });
}

fn binding(name: &str, ty: Type) -> StickyBinding {
    StickyBinding {
        name: name.to_string(),
        ty,
    }
}
