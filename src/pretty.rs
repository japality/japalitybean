use crate::ast::*;

pub fn pretty_program(program: &Program) -> String {
    let mut out = String::new();
    for (idx, func) in program.functions.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        pretty_function(&mut out, &func.kind);
    }
    out
}

pub fn program_json(program: &Program) -> String {
    let functions = program
        .functions
        .iter()
        .map(|func| {
            let f = &func.kind;
            serde_json::json!({
                "id": func.id,
                "name": f.name,
                "intent": f.intent,
                "inputs": f.inputs.iter().map(binding_json).collect::<Vec<_>>(),
                "output": binding_json(&f.output),
                "body_len": f.body.len(),
                "close_name": f.close_name,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({ "functions": functions }))
        .expect("AST JSON is serializable")
}

fn pretty_function(out: &mut String, func: &FunctionDecl) {
    out.push_str(&format!("@func {}\n", func.name));
    out.push_str(&format!("@intent: {}\n", func.intent));
    for input in &func.inputs {
        out.push_str(&format!("@in {}::{}\n", input.name, input.ty));
    }
    out.push_str(&format!("@out {}::{}\n", func.output.name, func.output.ty));
    out.push_str("---\n");
    for stmt in &func.body {
        pretty_stmt(out, stmt, 0);
    }
    out.push_str(&format!("@end func {}\n", func.close_name));
}

fn pretty_stmt(out: &mut String, stmt: &Node<Statement>, indent: usize) {
    let pad = "  ".repeat(indent);
    match &stmt.kind {
        Statement::Let(stmt) => out.push_str(&format!(
            "{pad}let {}::{} = {}\n",
            stmt.binding.name,
            stmt.binding.ty,
            expr_to_string(&stmt.value)
        )),
        Statement::Set(stmt) => out.push_str(&format!(
            "{pad}set {} = {}\n",
            stmt.target,
            expr_to_string(&stmt.value)
        )),
        Statement::Return(stmt) => {
            if let Some(value) = &stmt.value {
                out.push_str(&format!("{pad}return {}\n", expr_to_string(value)));
            } else {
                out.push_str(&format!("{pad}return\n"));
            }
        }
        Statement::Loop(loop_block) => {
            out.push_str(&format!(
                "{pad}@loop {}::{} in {}\n",
                loop_block.item.name, loop_block.item.ty, loop_block.collection
            ));
            for child in &loop_block.body {
                pretty_stmt(out, child, indent + 1);
            }
            match &loop_block.close_name {
                Some(name) => out.push_str(&format!("{pad}@end loop {name}\n")),
                None => out.push_str(&format!("{pad}@end loop\n")),
            }
        }
        Statement::Condition(cond) => {
            out.push_str(&format!("{pad}@condition {}\n", cond.name));
            out.push_str(&format!(
                "{pad}if ({}) -> {}\n",
                expr_to_string(&cond.guard.kind.cond),
                guard_action_to_string(&cond.guard.kind.action)
            ));
        }
        Statement::Expr(expr) => out.push_str(&format!("{pad}{}\n", expr_to_string(expr))),
    }
}

pub fn expr_to_string(expr: &Node<Expr>) -> String {
    match &expr.kind {
        Expr::Lit(Literal::Int(value)) => value.to_string(),
        Expr::Lit(Literal::Float(value)) => value.to_string(),
        Expr::Lit(Literal::Str(value)) => format!("\"{}\"", escape_string_literal(value)),
        Expr::Lit(Literal::Bool(value)) => value.to_string(),
        Expr::Ident(name) => name.clone(),
        Expr::Binary { op, lhs, rhs } => {
            format!("{} {} {}", expr_to_string(lhs), op, expr_to_string(rhs))
        }
        Expr::Unary { op, operand } => format!("{op}{}", expr_to_string(operand)),
        Expr::Call { callee, args } => format!(
            "{}({})",
            callee,
            args.iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Cast { expr, target } => format!("{} as::<{}>", expr_to_string(expr), target),
        Expr::Ctor(ConstructorExpr::Some(expr)) => format!("Some({})", expr_to_string(expr)),
        Expr::Ctor(ConstructorExpr::None) => "None".to_string(),
        Expr::Ctor(ConstructorExpr::Ok(expr)) => format!("Ok({})", expr_to_string(expr)),
        Expr::Ctor(ConstructorExpr::Err(expr)) => format!("Err({})", expr_to_string(expr)),
        Expr::Paren(inner) => format!("({})", expr_to_string(inner)),
    }
}

fn guard_action_to_string(action: &GuardAction) -> String {
    match action {
        GuardAction::Continue => "@continue".to_string(),
        GuardAction::Break => "@break".to_string(),
        GuardAction::Return(expr) => format!("return {}", expr_to_string(expr)),
        GuardAction::SetAssign { target, value } => {
            format!("set {target} = {}", expr_to_string(value))
        }
    }
}

fn binding_json(binding: &StickyBinding) -> serde_json::Value {
    serde_json::json!({
        "name": binding.name,
        "type": binding.ty.to_string(),
    })
}

fn escape_string_literal(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}
