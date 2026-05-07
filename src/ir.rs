use crate::ast::*;
use std::collections::HashMap;

pub fn lower_program_to_ir(program: &Program) -> String {
    lower_typed_program_to_llvm(&TypedProgram { program })
}

pub struct TypedProgram<'a> {
    pub program: &'a Program,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    inputs: Vec<Type>,
    output: Type,
    builtin: bool,
}

#[derive(Debug, Clone)]
struct Slot {
    ptr: String,
    ty: Type,
}

#[derive(Debug, Clone)]
struct Value {
    repr: String,
    ty: Type,
}

#[derive(Debug, Clone)]
struct LoopLabels {
    latch: String,
    exit: String,
}

struct FunctionBuilder<'a> {
    out: String,
    temp_counter: usize,
    label_counter: usize,
    vars: Vec<HashMap<String, Slot>>,
    funcs: &'a HashMap<String, FunctionSig>,
    loops: Vec<LoopLabels>,
    terminated: bool,
}

pub fn lower_typed_program_to_llvm(typed: &TypedProgram<'_>) -> String {
    lower_typed_program_to_llvm_with_entry(typed, None).expect("library IR has no entrypoint errors")
}

pub fn lower_typed_program_to_llvm_with_entry(
    typed: &TypedProgram<'_>,
    entry: Option<&str>,
) -> Result<String, String> {
    let funcs = collect_sigs(typed.program);
    let mut out = String::new();
    out.push_str("; JapalityBean LLVM text IR\n");
    out.push_str("declare void @debug_i32(i32)\n");
    out.push_str("declare void @debug_string({ i8*, i64 })\n");
    out.push_str("declare i32 @abs_i32(i32)\n");
    out.push_str("declare i1 @is_even_i32(i32)\n");
    out.push_str("declare i32 @max_i32(i32, i32)\n");
    out.push_str("declare i64 @vector_len_i32({ i32*, i64, i64 })\n\n");

    for func in &typed.program.functions {
        let mut builder = FunctionBuilder::new(&funcs);
        out.push_str(&builder.lower_function(&func.kind));
        out.push('\n');
    }
    if let Some(entry) = entry {
        append_linux_main_wrapper(&mut out, &funcs, entry)?;
    }
    Ok(out)
}

impl<'a> FunctionBuilder<'a> {
    fn new(funcs: &'a HashMap<String, FunctionSig>) -> Self {
        Self {
            out: String::new(),
            temp_counter: 0,
            label_counter: 0,
            vars: Vec::new(),
            funcs,
            loops: Vec::new(),
            terminated: false,
        }
    }

    fn lower_function(&mut self, func: &FunctionDecl) -> String {
        let params = func
            .inputs
            .iter()
            .map(|input| format!("{} %{}", llvm_ty(&input.ty), input.name))
            .collect::<Vec<_>>()
            .join(", ");
        self.line(&format!(
            "define internal {} @{}({}) {{",
            llvm_ty(&func.output.ty),
            mangle_user_func(&func.name),
            params
        ));
        self.label("entry");
        self.push_scope();

        for input in &func.inputs {
            let ptr = self.alloca(&format!("{}.addr", input.name), &input.ty);
            self.line(&format!(
                "store {} %{}, {}* {}",
                llvm_ty(&input.ty),
                input.name,
                llvm_ty(&input.ty),
                ptr
            ));
            self.declare(&input.name, ptr, input.ty.clone());
        }

        if func.output.ty != Type::Unit {
            let ptr = self.alloca(&func.output.name, &func.output.ty);
            self.declare(&func.output.name, ptr, func.output.ty.clone());
        }

        for stmt in &func.body {
            if !self.terminated {
                self.lower_stmt(stmt, &func.output.ty);
            }
        }

        if !self.terminated {
            if func.output.ty == Type::Unit {
                self.line("ret void");
            } else if let Some(slot) = self.lookup(&func.output.name).cloned() {
                let ret = self.load_slot(&slot);
                self.line(&format!("ret {} {}", llvm_ty(&ret.ty), ret.repr));
            } else {
                self.line(&format!("ret {} zeroinitializer", llvm_ty(&func.output.ty)));
            }
        }

        self.pop_scope();
        self.line("}");
        std::mem::take(&mut self.out)
    }

    fn lower_stmt(&mut self, stmt: &Node<Statement>, output_ty: &Type) {
        match &stmt.kind {
            Statement::Let(stmt) => {
                let value = self.lower_expr(&stmt.value, Some(&stmt.binding.ty));
                let ptr = self.alloca(&stmt.binding.name, &stmt.binding.ty);
                let value = self.cast_value_for_store(value, &stmt.binding.ty);
                self.line(&format!(
                    "store {} {}, {}* {}",
                    llvm_ty(&stmt.binding.ty),
                    value.repr,
                    llvm_ty(&stmt.binding.ty),
                    ptr
                ));
                self.declare(&stmt.binding.name, ptr, stmt.binding.ty.clone());
            }
            Statement::Set(stmt) => {
                if let Some(slot) = self.lookup(&stmt.target).cloned() {
                    let value = self.lower_expr(&stmt.value, Some(&slot.ty));
                    let value = self.cast_value_for_store(value, &slot.ty);
                    self.line(&format!(
                        "store {} {}, {}* {}",
                        llvm_ty(&slot.ty),
                        value.repr,
                        llvm_ty(&slot.ty),
                        slot.ptr
                    ));
                }
            }
            Statement::Return(stmt) => {
                if let Some(value) = &stmt.value {
                    let value = self.lower_expr(value, Some(output_ty));
                    if output_ty == &Type::Unit {
                        self.line("ret void");
                    } else {
                        self.line(&format!("ret {} {}", llvm_ty(output_ty), value.repr));
                    }
                } else {
                    self.line("ret void");
                }
                self.terminated = true;
            }
            Statement::Loop(loop_block) => self.lower_loop(loop_block, output_ty),
            Statement::Condition(cond) => self.lower_condition(cond, output_ty),
            Statement::Expr(expr) => {
                self.lower_expr(expr, None);
            }
        }
    }

    fn lower_loop(&mut self, loop_block: &LoopBlock, output_ty: &Type) {
        let Some(collection) = self.lookup(&loop_block.collection).cloned() else {
            return;
        };
        let Type::Vector(inner) = collection.ty.clone() else {
            return;
        };

        let idx_ptr = self.alloca(&format!("{}.idx", loop_block.item.name), &Type::I64);
        self.line(&format!("store i64 0, i64* {idx_ptr}"));
        let header = self.fresh_label("loop.header");
        let body = self.fresh_label("loop.body");
        let latch = self.fresh_label("loop.latch");
        let exit = self.fresh_label("loop.exit");

        self.line(&format!("br label %{header}"));
        self.label(&header);
        let idx = self.load_ptr(&idx_ptr, &Type::I64);
        let collection_value = self.load_slot(&collection);
        let len = self.temp();
        self.line(&format!(
            "{len} = extractvalue {} {}, 1",
            llvm_ty(&collection.ty),
            collection_value.repr
        ));
        let cond = self.temp();
        self.line(&format!("{cond} = icmp slt i64 {}, {len}", idx.repr));
        self.line(&format!("br i1 {cond}, label %{body}, label %{exit}"));

        self.label(&body);
        self.push_scope();
        let data = self.temp();
        self.line(&format!(
            "{data} = extractvalue {} {}, 0",
            llvm_ty(&collection.ty),
            collection_value.repr
        ));
        let elem_ptr = self.temp();
        self.line(&format!(
            "{elem_ptr} = getelementptr {}, {}* {data}, i64 {}",
            llvm_ty(&inner),
            llvm_ty(&inner),
            idx.repr
        ));
        let elem = self.temp();
        self.line(&format!(
            "{elem} = load {}, {}* {elem_ptr}",
            llvm_ty(&inner),
            llvm_ty(&inner)
        ));
        let item_ptr = self.alloca(&loop_block.item.name, &loop_block.item.ty);
        self.line(&format!(
            "store {} {elem}, {}* {item_ptr}",
            llvm_ty(&loop_block.item.ty),
            llvm_ty(&loop_block.item.ty)
        ));
        self.declare(&loop_block.item.name, item_ptr, loop_block.item.ty.clone());
        self.loops.push(LoopLabels {
            latch: latch.clone(),
            exit: exit.clone(),
        });
        self.terminated = false;
        for child in &loop_block.body {
            if !self.terminated {
                self.lower_stmt(child, output_ty);
            }
        }
        self.loops.pop();
        self.pop_scope();
        if !self.terminated {
            self.line(&format!("br label %{latch}"));
        }
        self.terminated = false;

        self.label(&latch);
        let next = self.temp();
        self.line(&format!("{next} = add i64 {}, 1", idx.repr));
        self.line(&format!("store i64 {next}, i64* {idx_ptr}"));
        self.line(&format!("br label %{header}"));

        self.label(&exit);
    }

    fn lower_condition(&mut self, cond: &ConditionBlock, output_ty: &Type) {
        let value = self.lower_expr(&cond.guard.kind.cond, Some(&Type::Bool));
        let taken = self.fresh_label(&format!("cond.{}.taken", cond.name));
        let next = self.fresh_label(&format!("cond.{}.next", cond.name));
        self.line(&format!("br i1 {}, label %{taken}, label %{next}", value.repr));
        self.label(&taken);
        self.lower_guard_action(&cond.guard.kind.action, output_ty, &next);
        self.label(&next);
        self.terminated = false;
    }

    fn lower_guard_action(&mut self, action: &GuardAction, output_ty: &Type, next: &str) {
        match action {
            GuardAction::Continue => {
                if let Some(labels) = self.loops.last() {
                    self.line(&format!("br label %{}", labels.latch));
                } else {
                    self.line(&format!("br label %{next}"));
                }
            }
            GuardAction::Break => {
                if let Some(labels) = self.loops.last() {
                    self.line(&format!("br label %{}", labels.exit));
                } else {
                    self.line(&format!("br label %{next}"));
                }
            }
            GuardAction::Return(expr) => {
                let value = self.lower_expr(expr, Some(output_ty));
                if output_ty == &Type::Unit {
                    self.line("ret void");
                } else {
                    self.line(&format!("ret {} {}", llvm_ty(output_ty), value.repr));
                }
            }
            GuardAction::SetAssign { target, value } => {
                if let Some(slot) = self.lookup(target).cloned() {
                    let value = self.lower_expr(value, Some(&slot.ty));
                    let value = self.cast_value_for_store(value, &slot.ty);
                    self.line(&format!(
                        "store {} {}, {}* {}",
                        llvm_ty(&slot.ty),
                        value.repr,
                        llvm_ty(&slot.ty),
                        slot.ptr
                    ));
                }
                self.line(&format!("br label %{next}"));
            }
        }
    }

    fn lower_expr(&mut self, expr: &Node<Expr>, expected: Option<&Type>) -> Value {
        match &expr.kind {
            Expr::Lit(Literal::Int(value)) => Value {
                repr: value.to_string(),
                ty: expected.filter(|ty| ty.is_numeric()).cloned().unwrap_or(Type::I64),
            },
            Expr::Lit(Literal::Float(value)) => Value {
                repr: value.to_string(),
                ty: expected
                    .filter(|ty| matches!(ty, Type::F32 | Type::F64))
                    .cloned()
                    .unwrap_or(Type::F64),
            },
            Expr::Lit(Literal::Bool(value)) => Value {
                repr: if *value { "1".to_string() } else { "0".to_string() },
                ty: Type::Bool,
            },
            Expr::Lit(Literal::Str(_)) => Value {
                repr: "zeroinitializer".to_string(),
                ty: Type::String,
            },
            Expr::Ident(name) => self
                .lookup(name)
                .cloned()
                .map(|slot| self.load_slot(&slot))
                .unwrap_or(Value {
                    repr: "0".to_string(),
                    ty: Type::I64,
                }),
            Expr::Paren(inner) => self.lower_expr(inner, expected),
            Expr::Unary { op, operand } => {
                let value = self.lower_expr(operand, expected);
                match op {
                    UnOp::Neg => {
                        let zero = if matches!(value.ty, Type::F32 | Type::F64) { "0.0" } else { "0" };
                        let tmp = self.temp();
                        let instr = if matches!(value.ty, Type::F32 | Type::F64) {
                            "fsub"
                        } else {
                            "sub"
                        };
                        self.line(&format!("{tmp} = {instr} {} {zero}, {}", llvm_ty(&value.ty), value.repr));
                        Value { repr: tmp, ty: value.ty }
                    }
                    UnOp::Not => {
                        let tmp = self.temp();
                        self.line(&format!("{tmp} = xor i1 {}, 1", value.repr));
                        Value { repr: tmp, ty: Type::Bool }
                    }
                }
            }
            Expr::Binary { op, lhs, rhs } => self.lower_binary(*op, lhs, rhs, expected),
            Expr::Call { callee, args } => self.lower_call(callee, args),
            Expr::Cast { expr, target } => {
                let source = self.lower_expr(expr, None);
                self.cast_value_for_store(source, target)
            }
            Expr::Ctor(ctor) => self.lower_ctor(ctor, expected),
        }
    }

    fn lower_binary(
        &mut self,
        op: BinOp,
        lhs: &Node<Expr>,
        rhs: &Node<Expr>,
        expected: Option<&Type>,
    ) -> Value {
        let left = self.lower_expr(lhs, expected);
        let right = self.lower_expr(rhs, Some(&left.ty));
        let tmp = self.temp();
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let instr = arithmetic_instr(op, &left.ty);
                self.line(&format!("{tmp} = {instr} {} {}, {}", llvm_ty(&left.ty), left.repr, right.repr));
                Value { repr: tmp, ty: left.ty }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Neq => {
                let instr = compare_instr(op, &left.ty);
                self.line(&format!("{tmp} = {instr} {} {}, {}", llvm_ty(&left.ty), left.repr, right.repr));
                Value { repr: tmp, ty: Type::Bool }
            }
            BinOp::And | BinOp::Or => {
                let instr = if op == BinOp::And { "and" } else { "or" };
                self.line(&format!("{tmp} = {instr} i1 {}, {}", left.repr, right.repr));
                Value { repr: tmp, ty: Type::Bool }
            }
        }
    }

    fn lower_call(&mut self, callee: &str, args: &[Node<Expr>]) -> Value {
        let sig = self.funcs.get(callee).cloned().unwrap_or(FunctionSig {
            inputs: Vec::new(),
            output: Type::Unknown,
            builtin: false,
        });
        let lowered_args = args
            .iter()
            .enumerate()
            .map(|(idx, arg)| {
                let expected = sig.inputs.get(idx);
                let value = self.lower_expr(arg, expected);
                format!("{} {}", llvm_ty(&value.ty), value.repr)
            })
            .collect::<Vec<_>>()
            .join(", ");
        if sig.output == Type::Unit {
            self.line(&format!(
                "call void @{}({lowered_args})",
                llvm_func_name(callee, &sig)
            ));
            Value {
                repr: String::new(),
                ty: Type::Unit,
            }
        } else {
            let tmp = self.temp();
            self.line(&format!(
                "{tmp} = call {} @{}({lowered_args})",
                llvm_ty(&sig.output),
                llvm_func_name(callee, &sig)
            ));
            Value {
                repr: tmp,
                ty: sig.output,
            }
        }
    }

    fn lower_ctor(&mut self, ctor: &ConstructorExpr, expected: Option<&Type>) -> Value {
        let ty = match (ctor, expected) {
            (ConstructorExpr::Some(_), Some(Type::Option(inner))) => Type::Option(inner.clone()),
            (ConstructorExpr::None, Some(Type::Option(inner))) => Type::Option(inner.clone()),
            (ConstructorExpr::Ok(_), Some(Type::Result(ok, err))) => Type::Result(ok.clone(), err.clone()),
            (ConstructorExpr::Err(_), Some(Type::Result(ok, err))) => Type::Result(ok.clone(), err.clone()),
            _ => Type::Unknown,
        };
        Value {
            repr: "zeroinitializer".to_string(),
            ty,
        }
    }

    fn cast_value_for_store(&mut self, value: Value, target: &Type) -> Value {
        if value.ty == *target || value.ty == Type::Unknown || *target == Type::Unknown {
            return Value {
                repr: value.repr,
                ty: target.clone(),
            };
        }
        if llvm_ty(&value.ty) == llvm_ty(target) {
            return Value {
                repr: value.repr,
                ty: target.clone(),
            };
        }
        if !(value.ty.is_numeric() && target.is_numeric()) {
            return value;
        }
        let tmp = self.temp();
        let instr = cast_instr(&value.ty, target);
        self.line(&format!(
            "{tmp} = {instr} {} {} to {}",
            llvm_ty(&value.ty),
            value.repr,
            llvm_ty(target)
        ));
        Value {
            repr: tmp,
            ty: target.clone(),
        }
    }

    fn load_slot(&mut self, slot: &Slot) -> Value {
        self.load_ptr(&slot.ptr, &slot.ty)
    }

    fn load_ptr(&mut self, ptr: &str, ty: &Type) -> Value {
        let tmp = self.temp();
        self.line(&format!("{tmp} = load {}, {}* {ptr}", llvm_ty(ty), llvm_ty(ty)));
        Value {
            repr: tmp,
            ty: ty.clone(),
        }
    }

    fn alloca(&mut self, name: &str, ty: &Type) -> String {
        let ptr = format!("%{}", sanitize(name));
        self.line(&format!("{ptr} = alloca {}", llvm_ty(ty)));
        ptr
    }

    fn declare(&mut self, name: &str, ptr: String, ty: Type) {
        if let Some(scope) = self.vars.last_mut() {
            scope.insert(name.to_string(), Slot { ptr, ty });
        }
    }

    fn lookup(&self, name: &str) -> Option<&Slot> {
        self.vars.iter().rev().find_map(|scope| scope.get(name))
    }

    fn push_scope(&mut self) {
        self.vars.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.vars.pop();
    }

    fn temp(&mut self) -> String {
        self.temp_counter += 1;
        format!("%t{}", self.temp_counter)
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!("{}.{}", sanitize(prefix), self.label_counter)
    }

    fn label(&mut self, name: &str) {
        self.out.push_str(&format!("{name}:\n"));
    }

    fn line(&mut self, line: &str) {
        self.out.push_str("  ");
        self.out.push_str(line);
        self.out.push('\n');
    }
}

fn llvm_ty(ty: &Type) -> String {
    match ty {
        Type::I8 | Type::U8 => "i8".to_string(),
        Type::I16 | Type::U16 => "i16".to_string(),
        Type::I32 | Type::U32 => "i32".to_string(),
        Type::I64 | Type::U64 => "i64".to_string(),
        Type::F32 => "float".to_string(),
        Type::F64 => "double".to_string(),
        Type::Bool => "i1".to_string(),
        Type::String => "{ i8*, i64 }".to_string(),
        Type::Unit => "void".to_string(),
        Type::Vector(inner) => format!("{{ {}*, i64, i64 }}", llvm_ty(inner)),
        Type::Option(inner) => format!("{{ i1, {} }}", llvm_ty(inner)),
        Type::Result(ok, err) => format!("{{ i1, {{ {}, {} }} }}", llvm_ty(ok), llvm_ty(err)),
        Type::Boxed(inner) => format!("{}*", llvm_ty(inner)),
        Type::Unknown => "opaque".to_string(),
    }
}

fn collect_sigs(program: &Program) -> HashMap<String, FunctionSig> {
    let mut funcs = HashMap::new();
    funcs.insert(
        "debug_i32".to_string(),
        FunctionSig {
            inputs: vec![Type::I32],
            output: Type::Unit,
            builtin: true,
        },
    );
    funcs.insert(
        "debug_string".to_string(),
        FunctionSig {
            inputs: vec![Type::String],
            output: Type::Unit,
            builtin: true,
        },
    );
    funcs.insert(
        "abs_i32".to_string(),
        FunctionSig {
            inputs: vec![Type::I32],
            output: Type::I32,
            builtin: true,
        },
    );
    funcs.insert(
        "is_even_i32".to_string(),
        FunctionSig {
            inputs: vec![Type::I32],
            output: Type::Bool,
            builtin: true,
        },
    );
    funcs.insert(
        "max_i32".to_string(),
        FunctionSig {
            inputs: vec![Type::I32, Type::I32],
            output: Type::I32,
            builtin: true,
        },
    );
    funcs.insert(
        "vector_len_i32".to_string(),
        FunctionSig {
            inputs: vec![Type::Vector(Box::new(Type::I32))],
            output: Type::I64,
            builtin: true,
        },
    );
    for func in &program.functions {
        funcs.insert(
            func.kind.name.clone(),
            FunctionSig {
                inputs: func.kind.inputs.iter().map(|input| input.ty.clone()).collect(),
                output: func.kind.output.ty.clone(),
                builtin: false,
            },
        );
    }
    funcs
}

fn append_linux_main_wrapper(
    out: &mut String,
    funcs: &HashMap<String, FunctionSig>,
    entry: &str,
) -> Result<(), String> {
    let sig = funcs
        .get(entry)
        .filter(|sig| !sig.builtin)
        .ok_or_else(|| format!("executable entrypoint function not found: {entry}"))?;
    let args = sig
        .inputs
        .iter()
        .map(|ty| format!("{} {}", llvm_ty(ty), zero_value(ty)))
        .collect::<Vec<_>>()
        .join(", ");

    out.push_str("define i32 @main() {\n");
    out.push_str("entry:\n");
    if sig.output == Type::Unit {
        out.push_str(&format!("  call void @{}({args})\n", mangle_user_func(entry)));
        out.push_str("  ret i32 0\n");
    } else {
        out.push_str(&format!(
            "  %result = call {} @{}({args})\n",
            llvm_ty(&sig.output),
            mangle_user_func(entry)
        ));
        match sig.output {
            Type::I32 | Type::U32 => out.push_str("  ret i32 %result\n"),
            Type::I8 | Type::U8 | Type::I16 | Type::U16 | Type::Bool => {
                out.push_str(&format!(
                    "  %exit_code = zext {} %result to i32\n",
                    llvm_ty(&sig.output)
                ));
                out.push_str("  ret i32 %exit_code\n");
            }
            Type::I64 | Type::U64 => {
                out.push_str("  %exit_code = trunc i64 %result to i32\n");
                out.push_str("  ret i32 %exit_code\n");
            }
            _ => out.push_str("  ret i32 0\n"),
        }
    }
    out.push_str("}\n");
    Ok(())
}

fn llvm_func_name(callee: &str, sig: &FunctionSig) -> String {
    if sig.builtin {
        callee.to_string()
    } else {
        mangle_user_func(callee)
    }
}

fn mangle_user_func(name: &str) -> String {
    format!("jb_{}", sanitize(name))
}

fn zero_value(ty: &Type) -> String {
    match ty {
        Type::Bool => "0".to_string(),
        Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64 => "0".to_string(),
        Type::F32 | Type::F64 => "0.0".to_string(),
        _ => "zeroinitializer".to_string(),
    }
}

fn arithmetic_instr(op: BinOp, ty: &Type) -> &'static str {
    if matches!(ty, Type::F32 | Type::F64) {
        return match op {
            BinOp::Add => "fadd",
            BinOp::Sub => "fsub",
            BinOp::Mul => "fmul",
            BinOp::Div => "fdiv",
            BinOp::Mod => "frem",
            _ => unreachable!(),
        };
    }
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "sdiv",
        BinOp::Mod => "srem",
        _ => unreachable!(),
    }
}

fn compare_instr(op: BinOp, ty: &Type) -> &'static str {
    if matches!(ty, Type::F32 | Type::F64) {
        return match op {
            BinOp::Lt => "fcmp olt",
            BinOp::Le => "fcmp ole",
            BinOp::Gt => "fcmp ogt",
            BinOp::Ge => "fcmp oge",
            BinOp::Eq => "fcmp oeq",
            BinOp::Neq => "fcmp one",
            _ => unreachable!(),
        };
    }
    match op {
        BinOp::Lt => "icmp slt",
        BinOp::Le => "icmp sle",
        BinOp::Gt => "icmp sgt",
        BinOp::Ge => "icmp sge",
        BinOp::Eq => "icmp eq",
        BinOp::Neq => "icmp ne",
        _ => unreachable!(),
    }
}

fn cast_instr(source: &Type, target: &Type) -> &'static str {
    match (source, target) {
        (Type::F32 | Type::F64, Type::F32 | Type::F64) => {
            if bit_width(source) < bit_width(target) {
                "fpext"
            } else {
                "fptrunc"
            }
        }
        (Type::F32 | Type::F64, _) => "fptosi",
        (_, Type::F32 | Type::F64) => "sitofp",
        _ => {
            if bit_width(source) < bit_width(target) {
                "sext"
            } else {
                "trunc"
            }
        }
    }
}

fn bit_width(ty: &Type) -> u16 {
    match ty {
        Type::I8 | Type::U8 => 8,
        Type::I16 | Type::U16 => 16,
        Type::I32 | Type::U32 | Type::F32 => 32,
        Type::I64 | Type::U64 | Type::F64 => 64,
        Type::Bool => 1,
        _ => 64,
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' { ch } else { '_' })
        .collect()
}
