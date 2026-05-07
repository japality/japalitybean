use crate::ast::*;
use crate::diag::{fix, Diagnostic, Phase};
use crate::lexer::{Token, TokenKind};

#[derive(Debug, Clone)]
struct Line {
    number: usize,
    start: usize,
    end: usize,
    text: String,
    trimmed: String,
}

pub fn parse_program_from_tokens(source: &str, tokens: &[Token]) -> (Program, Vec<Diagnostic>) {
    let mut parser = Parser::new(source, tokens);
    let program = parser.program();
    (program, parser.diags)
}

struct Parser {
    lines: Vec<Line>,
    index: usize,
    diags: Vec<Diagnostic>,
    id_counter: usize,
    source_hash: u32,
}

impl Parser {
    fn new(source: &str, tokens: &[Token]) -> Self {
        Self {
            lines: split_token_lines(source, tokens),
            index: 0,
            diags: Vec::new(),
            id_counter: 0,
            source_hash: stable_hash(source),
        }
    }

    fn program(&mut self) -> Program {
        let mut functions = Vec::new();
        while let Some(line) = self.peek_significant().cloned() {
            if line.trimmed.starts_with("@func ") {
                functions.push(self.function());
            } else {
                self.diags.push(Diagnostic::error(
                    "E_PARSE001",
                    Phase::Parse,
                    None,
                    span_for_line(&line),
                    "top-level item must start with @func",
                    Some("@func <identifier>".to_string()),
                    Some(line.trimmed.clone()),
                    fix("insert", "@func main"),
                ));
                self.index += 1;
            }
        }
        Program { functions }
    }

    fn function(&mut self) -> Node<FunctionDecl> {
        let open = self.next_significant().expect("peeked line must exist");
        let open_span = span_for_line(&open);
        let name = open
            .trimmed
            .strip_prefix("@func ")
            .unwrap_or("")
            .trim()
            .to_string();

        if !is_ident(&name) {
            self.diags.push(Diagnostic::error(
                "E_PARSE002",
                Phase::Parse,
                None,
                open_span,
                "function name must be an identifier",
                Some("identifier".to_string()),
                Some(name.clone()),
                fix("replace", "@func main"),
            ));
        }

        let intent = self.intent_line();
        let mut inputs = Vec::new();
        while let Some(line) = self.peek_significant().cloned() {
            if line.trimmed.starts_with("@in ") {
                self.index += 1;
                match parse_sticky_binding(line.trimmed.trim_start_matches("@in ").trim()) {
                    Ok(binding) => inputs.push(binding),
                    Err(actual) => self.diags.push(Diagnostic::error(
                        "E_TYPE001",
                        Phase::Parse,
                        None,
                        span_for_line(&line),
                        "@in declaration requires sticky-type annotation",
                        Some("identifier::Type".to_string()),
                        Some(actual),
                        fix("annotate_type", "@in value::i32"),
                    )),
                }
            } else {
                break;
            }
        }

        if inputs.is_empty() {
            self.diags.push(Diagnostic::error(
                "E_INTENT001",
                Phase::Parse,
                None,
                open_span,
                "function preamble requires at least one @in declaration",
                Some("@in <identifier>::<Type>".to_string()),
                None,
                fix("add_intent", "@in input::i32"),
            ));
        }

        let output = self.output_line(open_span);
        self.dash_separator(open_span);
        let body = self.statements_until_func_end(&name);

        let mut close_name = String::new();
        let mut end_span = open_span;
        if let Some(line) = self.peek_significant().cloned() {
            if line.trimmed.starts_with("@end func") {
                self.index += 1;
                close_name = line
                    .trimmed
                    .strip_prefix("@end func")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                end_span = span_for_line(&line);
                if close_name != name {
                    self.diags.push(Diagnostic::error(
                        "E_CLOSE001",
                        Phase::Parse,
                        None,
                        end_span,
                        "function close name does not match opener",
                        Some(name.clone()),
                        Some(close_name.clone()),
                        fix("rename", format!("@end func {name}")),
                    ));
                }
            }
        }

        if close_name.is_empty() {
            self.diags.push(Diagnostic::error(
                "E_CLOSE002",
                Phase::Parse,
                None,
                open_span,
                "function block reached EOF without matching @end func",
                Some(format!("@end func {name}")),
                None,
                fix("close_block", format!("@end func {name}")),
            ));
            close_name = name.clone();
        }

        self.node(
            "func",
            join_span(open_span, end_span),
            FunctionDecl {
                name,
                intent,
                inputs,
                output,
                body,
                close_name,
            },
        )
    }

    fn intent_line(&mut self) -> String {
        if let Some(line) = self.next_significant() {
            if let Some(intent) = line.trimmed.strip_prefix("@intent:") {
                let intent = intent.trim().to_string();
                if intent.is_empty() || intent.chars().count() > 280 {
                    self.diags.push(Diagnostic::error(
                        "E_INTENT001",
                        Phase::Parse,
                        None,
                        span_for_line(&line),
                        "@intent must contain 1..=280 characters",
                        Some("@intent: <short purpose>".to_string()),
                        Some(line.trimmed),
                        fix("add_intent", "@intent: Describe this function"),
                    ));
                }
                return intent;
            }

            self.diags.push(Diagnostic::error(
                "E_INTENT001",
                Phase::Parse,
                None,
                span_for_line(&line),
                "function preamble must start with @intent:",
                Some("@intent: <purpose>".to_string()),
                Some(line.trimmed),
                fix("add_intent", "@intent: Describe this function"),
            ));
        }
        String::new()
    }

    fn output_line(&mut self, _fallback_span: Span) -> StickyBinding {
        if let Some(line) = self.next_significant() {
            if line.trimmed.starts_with("@out ") {
                return match parse_sticky_binding(line.trimmed.trim_start_matches("@out ").trim()) {
                    Ok(binding) => binding,
                    Err(actual) => {
                        self.diags.push(Diagnostic::error(
                            "E_TYPE001",
                            Phase::Parse,
                            None,
                            span_for_line(&line),
                            "@out declaration requires sticky-type annotation",
                            Some("identifier::Type".to_string()),
                            Some(actual),
                            fix("annotate_type", "@out result::i32"),
                        ));
                        StickyBinding {
                            name: "result".to_string(),
                            ty: Type::Unknown,
                        }
                    }
                };
            }
            self.diags.push(Diagnostic::error(
                "E_INTENT001",
                Phase::Parse,
                None,
                span_for_line(&line),
                "function preamble requires exactly one @out declaration after @in lines",
                Some("@out <identifier>::<Type>".to_string()),
                Some(line.trimmed),
                fix("add_intent", "@out result::i32"),
            ));
        }
        StickyBinding {
            name: "result".to_string(),
            ty: Type::Unknown,
        }
    }

    fn dash_separator(&mut self, fallback_span: Span) {
        if let Some(line) = self.next_significant() {
            if line.trimmed == "---" {
                return;
            }
            self.diags.push(Diagnostic::error(
                "E_INTENT001",
                Phase::Parse,
                None,
                span_for_line(&line),
                "intent preamble must be followed by --- separator",
                Some("---".to_string()),
                Some(line.trimmed),
                fix("insert", "---"),
            ));
            return;
        }
        self.diags.push(Diagnostic::error(
            "E_INTENT001",
            Phase::Parse,
            None,
            fallback_span,
            "intent preamble must be followed by --- separator",
            Some("---".to_string()),
            None,
            fix("insert", "---"),
        ));
    }

    fn statements_until_func_end(&mut self, func_name: &str) -> Vec<Node<Statement>> {
        let mut body = Vec::new();
        while let Some(line) = self.peek_significant().cloned() {
            if line.trimmed.starts_with("@end func") {
                break;
            }
            if line.trimmed.starts_with("@end loop") {
                self.diags.push(Diagnostic::error(
                    "E_CLOSE002",
                    Phase::Parse,
                    None,
                    span_for_line(&line),
                    "encountered @end loop outside a loop block",
                    Some(format!("@end func {func_name}")),
                    Some(line.trimmed),
                    fix("delete", ""),
                ));
                self.index += 1;
                continue;
            }
            body.push(self.statement());
        }
        body
    }

    fn statements_until_loop_end(&mut self, collection: &str, open_span: Span) -> Vec<Node<Statement>> {
        let mut body = Vec::new();
        while let Some(line) = self.peek_significant().cloned() {
            if line.trimmed.starts_with("@end loop") || line.trimmed.starts_with("@end func") {
                break;
            }
            body.push(self.statement());
        }
        if self.peek_significant().is_none() {
            self.diags.push(Diagnostic::error(
                "E_CLOSE002",
                Phase::Parse,
                None,
                open_span,
                "loop block reached EOF without matching @end loop",
                Some("@end loop".to_string()),
                None,
                fix("close_block", format!("@end loop {collection}")),
            ));
        }
        body
    }

    fn statement(&mut self) -> Node<Statement> {
        let line = self.next_significant().expect("caller checks line");
        let span = span_for_line(&line);
        let text = line.trimmed.as_str();

        if text.starts_with("let ") {
            return self.let_stmt(line);
        }
        if text.starts_with("set ") {
            return self.set_stmt(line);
        }
        if text == "return" || text.starts_with("return ") {
            let expr_src = text.strip_prefix("return").unwrap().trim();
            let value = if expr_src.is_empty() {
                None
            } else {
                Some(parse_expr_node(expr_src, span, &mut self.diags, &mut self.id_counter, self.source_hash))
            };
            return self.node("return", span, Statement::Return(ReturnStmt { value }));
        }
        if text.starts_with("@loop ") {
            return self.loop_stmt(line);
        }
        if text.starts_with("@condition ") {
            return self.condition_stmt(line);
        }

        let expr = parse_expr_node(text, span, &mut self.diags, &mut self.id_counter, self.source_hash);
        self.node("expr_stmt", span, Statement::Expr(expr))
    }

    fn let_stmt(&mut self, line: Line) -> Node<Statement> {
        let span = span_for_line(&line);
        let rest = line.trimmed.trim_start_matches("let ").trim();
        let (binding_src, value_src) = match rest.split_once('=') {
            Some(parts) => (parts.0.trim(), parts.1.trim()),
            None => {
                self.diags.push(Diagnostic::error(
                    "E_PARSE003",
                    Phase::Parse,
                    None,
                    span,
                    "let statement requires =",
                    Some("let name::Type = expr".to_string()),
                    Some(line.trimmed.clone()),
                    fix("insert", " = "),
                ));
                (rest, "")
            }
        };
        let binding = match parse_sticky_binding(binding_src) {
            Ok(binding) => binding,
            Err(actual) => {
                self.diags.push(Diagnostic::error(
                    "E_TYPE001",
                    Phase::Parse,
                    None,
                    span,
                    "let binding requires sticky-type annotation",
                    Some("identifier::Type".to_string()),
                    Some(actual),
                    fix("annotate_type", format!("let {binding_src}::i32 = {value_src}")),
                ));
                StickyBinding {
                    name: binding_src.to_string(),
                    ty: Type::Unknown,
                }
            }
        };
        let value = parse_expr_node(value_src, span, &mut self.diags, &mut self.id_counter, self.source_hash);
        self.node("let", span, Statement::Let(LetStmt { binding, value }))
    }

    fn set_stmt(&mut self, line: Line) -> Node<Statement> {
        let span = span_for_line(&line);
        let rest = line.trimmed.trim_start_matches("set ").trim();
        let (target, value_src) = match rest.split_once('=') {
            Some(parts) => (parts.0.trim().to_string(), parts.1.trim()),
            None => {
                self.diags.push(Diagnostic::error(
                    "E_PARSE003",
                    Phase::Parse,
                    None,
                    span,
                    "set statement requires =",
                    Some("set name = expr".to_string()),
                    Some(line.trimmed.clone()),
                    fix("insert", " = "),
                ));
                (rest.to_string(), "")
            }
        };
        let value = parse_expr_node(value_src, span, &mut self.diags, &mut self.id_counter, self.source_hash);
        self.node("set", span, Statement::Set(SetStmt { target, value }))
    }

    fn loop_stmt(&mut self, line: Line) -> Node<Statement> {
        let open_span = span_for_line(&line);
        let rest = line.trimmed.trim_start_matches("@loop ").trim();
        let (binding_src, collection) = match rest.split_once(" in ") {
            Some(parts) => (parts.0.trim(), parts.1.trim().to_string()),
            None => {
                self.diags.push(Diagnostic::error(
                    "E_PARSE004",
                    Phase::Parse,
                    None,
                    open_span,
                    "@loop requires sticky item and collection",
                    Some("@loop item::Type in collection".to_string()),
                    Some(line.trimmed.clone()),
                    fix("replace", "@loop item::i32 in items"),
                ));
                (rest, "items".to_string())
            }
        };
        let item = match parse_sticky_binding(binding_src) {
            Ok(binding) => binding,
            Err(actual) => {
                self.diags.push(Diagnostic::error(
                    "E_TYPE001",
                    Phase::Parse,
                    None,
                    open_span,
                    "loop item requires sticky-type annotation",
                    Some("identifier::Type".to_string()),
                    Some(actual),
                    fix("annotate_type", "@loop item::i32 in items"),
                ));
                StickyBinding {
                    name: "item".to_string(),
                    ty: Type::Unknown,
                }
            }
        };

        let body = self.statements_until_loop_end(&collection, open_span);
        let mut close_name = None;
        let mut end_span = open_span;
        if let Some(close) = self.peek_significant().cloned() {
            if close.trimmed.starts_with("@end loop") {
                self.index += 1;
                end_span = span_for_line(&close);
                let suffix = close.trimmed.trim_start_matches("@end loop").trim();
                if !suffix.is_empty() {
                    close_name = Some(suffix.to_string());
                    if suffix != collection {
                        self.diags.push(Diagnostic::error(
                            "E_CLOSE001",
                            Phase::Parse,
                            None,
                            end_span,
                            "loop close name does not match collection",
                            Some(collection.clone()),
                            Some(suffix.to_string()),
                            fix("rename", format!("@end loop {collection}")),
                        ));
                    }
                }
            }
        }

        self.node(
            "loop",
            join_span(open_span, end_span),
            Statement::Loop(LoopBlock {
                item,
                collection,
                body,
                close_name,
            }),
        )
    }

    fn condition_stmt(&mut self, line: Line) -> Node<Statement> {
        let span = span_for_line(&line);
        let name = line
            .trimmed
            .trim_start_matches("@condition ")
            .trim()
            .to_string();
        let guard_line = self.next_significant();
        let guard = match guard_line {
            Some(guard_line) => self.guard_stmt(guard_line),
            None => {
                self.diags.push(Diagnostic::error(
                    "E_PARSE005",
                    Phase::Parse,
                    None,
                    span,
                    "@condition must be followed by one guard statement",
                    Some("if (<expr>) -> <action>".to_string()),
                    None,
                    fix("insert", "if (true) -> @continue"),
                ));
                let cond = self.node("bool", span, Expr::Lit(Literal::Bool(true)));
                self.node(
                    "guard",
                    span,
                    GuardStmt {
                        cond,
                        action: GuardAction::Continue,
                    },
                )
            }
        };
        self.node(
            "condition",
            join_span(span, guard.span),
            Statement::Condition(ConditionBlock { name, guard }),
        )
    }

    fn guard_stmt(&mut self, line: Line) -> Node<GuardStmt> {
        let span = span_for_line(&line);
        let text = line.trimmed.as_str();
        let Some(rest) = text.strip_prefix("if (") else {
            self.diags.push(Diagnostic::error(
                "E_PARSE005",
                Phase::Parse,
                None,
                span,
                "guard statement must start with if (",
                Some("if (<expr>) -> <action>".to_string()),
                Some(text.to_string()),
                fix("replace", "if (true) -> @continue"),
            ));
            let cond = self.node("bool", span, Expr::Lit(Literal::Bool(true)));
            return self.node(
                "guard",
                span,
                GuardStmt {
                    cond,
                    action: GuardAction::Continue,
                },
            );
        };
        let Some((cond_src, action_src)) = rest.split_once(") ->") else {
            self.diags.push(Diagnostic::error(
                "E_PARSE005",
                Phase::Parse,
                None,
                span,
                "guard statement requires ) -> separator",
                Some("if (<expr>) -> <action>".to_string()),
                Some(text.to_string()),
                fix("insert", ") -> @continue"),
            ));
            let cond = self.node("bool", span, Expr::Lit(Literal::Bool(true)));
            return self.node(
                "guard",
                span,
                GuardStmt {
                    cond,
                    action: GuardAction::Continue,
                },
            );
        };
        let cond = parse_expr_node(cond_src.trim(), span, &mut self.diags, &mut self.id_counter, self.source_hash);
        let action = parse_guard_action(action_src.trim(), span, &mut self.diags, &mut self.id_counter, self.source_hash);
        self.node("guard", span, GuardStmt { cond, action })
    }

    fn peek_significant(&self) -> Option<&Line> {
        self.lines
            .iter()
            .skip(self.index)
            .find(|line| is_significant(&line.trimmed))
    }

    fn next_significant(&mut self) -> Option<Line> {
        while self.index < self.lines.len() {
            let line = self.lines[self.index].clone();
            self.index += 1;
            if is_significant(&line.trimmed) {
                return Some(line);
            }
        }
        None
    }

    fn node<T>(&mut self, kind: &str, span: Span, item: T) -> Node<T> {
        self.id_counter += 1;
        Node {
            id: format!("{kind}:{:04}:{:08x}", self.id_counter, self.source_hash),
            span,
            kind: item,
        }
    }
}

pub fn parse_sticky_binding(src: &str) -> Result<StickyBinding, String> {
    let Some((name, ty_src)) = src.split_once("::") else {
        return Err(src.to_string());
    };
    if name.contains(char::is_whitespace) || ty_src.contains(char::is_whitespace) {
        return Err(src.to_string());
    }
    if !is_ident(name) {
        return Err(name.to_string());
    }
    let ty = parse_type(ty_src)?;
    Ok(StickyBinding {
        name: name.to_string(),
        ty,
    })
}

pub fn parse_type(src: &str) -> Result<Type, String> {
    let src = src.trim();
    match src {
        "i8" => Ok(Type::I8),
        "i16" => Ok(Type::I16),
        "i32" => Ok(Type::I32),
        "i64" => Ok(Type::I64),
        "u8" => Ok(Type::U8),
        "u16" => Ok(Type::U16),
        "u32" => Ok(Type::U32),
        "u64" => Ok(Type::U64),
        "f32" => Ok(Type::F32),
        "f64" => Ok(Type::F64),
        "bool" => Ok(Type::Bool),
        "string" => Ok(Type::String),
        "unit" => Ok(Type::Unit),
        _ if src.starts_with("Vector<") && src.ends_with('>') => {
            Ok(Type::Vector(Box::new(parse_type(&src[7..src.len() - 1])?)))
        }
        _ if src.starts_with("Option<") && src.ends_with('>') => {
            Ok(Type::Option(Box::new(parse_type(&src[7..src.len() - 1])?)))
        }
        _ if src.starts_with("Box<") && src.ends_with('>') => {
            Ok(Type::Boxed(Box::new(parse_type(&src[4..src.len() - 1])?)))
        }
        _ if src.starts_with("Result<") && src.ends_with('>') => {
            let inner = &src[7..src.len() - 1];
            let parts = split_top_level_comma(inner).ok_or_else(|| src.to_string())?;
            Ok(Type::Result(
                Box::new(parse_type(parts.0)?),
                Box::new(parse_type(parts.1)?),
            ))
        }
        _ => Err(src.to_string()),
    }
}

fn parse_guard_action(
    src: &str,
    span: Span,
    diags: &mut Vec<Diagnostic>,
    id_counter: &mut usize,
    source_hash: u32,
) -> GuardAction {
    if src == "@continue" {
        return GuardAction::Continue;
    }
    if src == "@break" {
        return GuardAction::Break;
    }
    if let Some(expr_src) = src.strip_prefix("return ") {
        return GuardAction::Return(parse_expr_node(
            expr_src.trim(),
            span,
            diags,
            id_counter,
            source_hash,
        ));
    }
    if let Some(rest) = src.strip_prefix("set ") {
        if let Some((target, value_src)) = rest.split_once('=') {
            return GuardAction::SetAssign {
                target: target.trim().to_string(),
                value: parse_expr_node(value_src.trim(), span, diags, id_counter, source_hash),
            };
        }
    }

    diags.push(Diagnostic::error(
        "E_PARSE006",
        Phase::Parse,
        None,
        span,
        "unsupported guard action",
        Some("@continue | @break | return <expr> | set <id> = <expr>".to_string()),
        Some(src.to_string()),
        fix("replace", "@continue"),
    ));
    GuardAction::Continue
}

fn parse_expr_node(
    src: &str,
    span: Span,
    diags: &mut Vec<Diagnostic>,
    id_counter: &mut usize,
    source_hash: u32,
) -> Node<Expr> {
    let mut tokens = lex_expr(src);
    let mut parser = ExprParser {
        tokens: &mut tokens,
        pos: 0,
        span,
        diags,
        id_counter,
        source_hash,
    };
    parser.expr_bp(0)
}

#[derive(Debug, Clone, PartialEq)]
enum ExprToken {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    CastType(Type),
    Op(String),
    LParen,
    RParen,
    Comma,
}

struct ExprParser<'a> {
    tokens: &'a mut Vec<ExprToken>,
    pos: usize,
    span: Span,
    diags: &'a mut Vec<Diagnostic>,
    id_counter: &'a mut usize,
    source_hash: u32,
}

impl ExprParser<'_> {
    fn expr_bp(&mut self, min_bp: u8) -> Node<Expr> {
        let mut lhs = self.prefix();
        loop {
            if let Some(ExprToken::CastType(target)) = self.peek().cloned() {
                self.pos += 1;
                lhs = self.node(
                    "cast",
                    Expr::Cast {
                        expr: Box::new(lhs),
                        target,
                    },
                );
                continue;
            }
            let Some(op) = self.peek_binop() else {
                break;
            };
            let (left_bp, right_bp) = infix_binding_power(op);
            if left_bp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.expr_bp(right_bp);
            lhs = self.node(
                "binary",
                Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            );
        }
        lhs
    }

    fn prefix(&mut self) -> Node<Expr> {
        match self.next() {
            Some(ExprToken::Op(op)) if op == "-" || op == "!" => {
                let operand = self.expr_bp(80);
                self.node(
                    "unary",
                    Expr::Unary {
                        op: if op == "-" { UnOp::Neg } else { UnOp::Not },
                        operand: Box::new(operand),
                    },
                )
            }
            Some(ExprToken::Int(value)) => self.node("int", Expr::Lit(Literal::Int(value))),
            Some(ExprToken::Float(value)) => self.node("float", Expr::Lit(Literal::Float(value))),
            Some(ExprToken::Str(value)) => self.node("string", Expr::Lit(Literal::Str(value))),
            Some(ExprToken::Bool(value)) => self.node("bool", Expr::Lit(Literal::Bool(value))),
            Some(ExprToken::Ident(name)) => {
                if is_constructor_name(&name) {
                    return self.constructor_expr(name);
                }
                if self.peek() == Some(&ExprToken::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    while self.peek() != Some(&ExprToken::RParen) && self.peek().is_some() {
                        args.push(self.expr_bp(0));
                        if self.peek() == Some(&ExprToken::Comma) {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    if self.peek() == Some(&ExprToken::RParen) {
                        self.pos += 1;
                    }
                    self.node("call", Expr::Call { callee: name, args })
                } else {
                    self.node("ident", Expr::Ident(name))
                }
            }
            Some(ExprToken::LParen) => {
                let inner = self.expr_bp(0);
                if self.peek() == Some(&ExprToken::RParen) {
                    self.pos += 1;
                }
                self.node("paren", Expr::Paren(Box::new(inner)))
            }
            other => {
                self.diags.push(Diagnostic::error(
                    "E_PARSE007",
                    Phase::Parse,
                    None,
                    self.span,
                    "invalid expression",
                    Some("expression".to_string()),
                    Some(format!("{other:?}")),
                    fix("replace", "0"),
                ));
                self.node("int", Expr::Lit(Literal::Int(0)))
            }
        }
    }

    fn constructor_expr(&mut self, name: String) -> Node<Expr> {
        match name.as_str() {
            "None" => {
                if self.peek() == Some(&ExprToken::LParen) {
                    self.pos += 1;
                    if self.peek() == Some(&ExprToken::RParen) {
                        self.pos += 1;
                    }
                }
                self.node("ctor", Expr::Ctor(ConstructorExpr::None))
            }
            "Some" | "Ok" | "Err" => {
                if self.peek() == Some(&ExprToken::LParen) {
                    self.pos += 1;
                } else {
                    self.diags.push(Diagnostic::error(
                        "E_PARSE007",
                        Phase::Parse,
                        None,
                        self.span,
                        "constructor requires parentheses",
                        Some(format!("{name}(<expr>)")),
                        Some(name.clone()),
                        fix("insert", format!("{name}(...)")),
                    ));
                }
                let value = self.expr_bp(0);
                if self.peek() == Some(&ExprToken::RParen) {
                    self.pos += 1;
                }
                let ctor = match name.as_str() {
                    "Some" => ConstructorExpr::Some(Box::new(value)),
                    "Ok" => ConstructorExpr::Ok(Box::new(value)),
                    "Err" => ConstructorExpr::Err(Box::new(value)),
                    _ => unreachable!(),
                };
                self.node("ctor", Expr::Ctor(ctor))
            }
            _ => self.node("ident", Expr::Ident(name)),
        }
    }

    fn peek_binop(&self) -> Option<BinOp> {
        match self.peek()? {
            ExprToken::Op(op) => match op.as_str() {
                "+" => Some(BinOp::Add),
                "-" => Some(BinOp::Sub),
                "*" => Some(BinOp::Mul),
                "/" => Some(BinOp::Div),
                "%" => Some(BinOp::Mod),
                "==" => Some(BinOp::Eq),
                "!=" => Some(BinOp::Neq),
                "<" => Some(BinOp::Lt),
                "<=" => Some(BinOp::Le),
                ">" => Some(BinOp::Gt),
                ">=" => Some(BinOp::Ge),
                "&&" => Some(BinOp::And),
                "||" => Some(BinOp::Or),
                _ => None,
            },
            _ => None,
        }
    }

    fn peek(&self) -> Option<&ExprToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<ExprToken> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn node(&mut self, kind: &str, expr: Expr) -> Node<Expr> {
        *self.id_counter += 1;
        Node {
            id: format!("{kind}:{:04}:{:08x}", *self.id_counter, self.source_hash),
            span: self.span,
            kind: expr,
        }
    }
}

fn infix_binding_power(op: BinOp) -> (u8, u8) {
    match op {
        BinOp::Or => (20, 21),
        BinOp::And => (30, 31),
        BinOp::Eq | BinOp::Neq => (40, 41),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (50, 51),
        BinOp::Add | BinOp::Sub => (60, 61),
        BinOp::Mul | BinOp::Div | BinOp::Mod => (70, 71),
    }
}

fn lex_expr(src: &str) -> Vec<ExprToken> {
    let mut chars = src.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    match chars.next() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        Some(other) => s.push(other),
                        None => break,
                    }
                } else {
                    s.push(c);
                }
            }
            tokens.push(ExprToken::Str(s));
            continue;
        }
        if ch.is_ascii_digit()
            || (ch == '-'
                && chars
                    .clone()
                    .nth(1)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                && should_lex_negative_literal(tokens.last()))
        {
            let mut number = String::new();
            number.push(chars.next().unwrap());
            while let Some(c) = chars.peek().copied() {
                if c.is_ascii_digit() || c == '.' {
                    number.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if number.contains('.') {
                if let Ok(value) = number.parse() {
                    tokens.push(ExprToken::Float(value));
                }
            } else if let Ok(value) = number.parse() {
                tokens.push(ExprToken::Int(value));
            }
            continue;
        }
        if is_ident_start(ch) {
            let mut ident = String::new();
            while let Some(c) = chars.peek().copied() {
                if is_ident_continue(c) {
                    ident.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if ident == "as" {
                if let Some(target) = consume_cast_type(&mut chars) {
                    tokens.push(ExprToken::CastType(target));
                    continue;
                }
            }
            match ident.as_str() {
                "true" => tokens.push(ExprToken::Bool(true)),
                "false" => tokens.push(ExprToken::Bool(false)),
                _ => tokens.push(ExprToken::Ident(ident)),
            }
            continue;
        }
        match ch {
            '(' => {
                chars.next();
                tokens.push(ExprToken::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(ExprToken::RParen);
            }
            ',' => {
                chars.next();
                tokens.push(ExprToken::Comma);
            }
            '&' | '|' | '=' | '!' | '<' | '>' => {
                let mut op = String::new();
                op.push(chars.next().unwrap());
                if let Some(next) = chars.peek().copied() {
                    if next == '=' || (op == "&" && next == '&') || (op == "|" && next == '|') {
                        op.push(next);
                        chars.next();
                    }
                }
                tokens.push(ExprToken::Op(op));
            }
            '+' | '-' | '*' | '/' | '%' => {
                chars.next();
                tokens.push(ExprToken::Op(ch.to_string()));
            }
            _ => {
                chars.next();
            }
        }
    }
    tokens
}

fn consume_cast_type<I>(chars: &mut std::iter::Peekable<I>) -> Option<Type>
where
    I: Iterator<Item = char> + Clone,
{
    let mut probe = chars.clone();
    if probe.next() != Some(':') || probe.next() != Some(':') || probe.next() != Some('<') {
        return None;
    }

    chars.next();
    chars.next();
    chars.next();

    let mut depth = 1usize;
    let mut ty_src = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                depth += 1;
                ty_src.push(ch);
            }
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return parse_type(&ty_src).ok();
                }
                ty_src.push(ch);
            }
            _ => ty_src.push(ch),
        }
    }
    None
}

fn should_lex_negative_literal(last: Option<&ExprToken>) -> bool {
    matches!(
        last,
        None | Some(ExprToken::LParen) | Some(ExprToken::Comma) | Some(ExprToken::Op(_))
    )
}

fn split_token_lines(source: &str, tokens: &[Token]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut current: Vec<&Token> = Vec::new();

    for token in tokens {
        if token.kind == TokenKind::Eof {
            break;
        }
        if token.kind == TokenKind::Newline {
            push_token_line(source, &current, &mut lines);
            current.clear();
        } else {
            current.push(token);
        }
    }
    push_token_line(source, &current, &mut lines);
    lines
}

fn push_token_line(source: &str, tokens: &[&Token], lines: &mut Vec<Line>) {
    let Some(first) = tokens.first() else {
        return;
    };
    let last = tokens.last().unwrap_or(first);
    let line_start = source[..first.span.start_byte]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let line_end = source[first.span.start_byte..]
        .find('\n')
        .map(|idx| first.span.start_byte + idx)
        .unwrap_or(source.len());
    let text = source[line_start..line_end]
        .trim_end_matches('\r')
        .to_string();

    lines.push(Line {
        number: first.span.start_line,
        start: line_start,
        end: last.span.end_byte.max(line_start + text.len()),
        trimmed: text.trim().to_string(),
        text,
    });
}

fn is_significant(trimmed: &str) -> bool {
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

fn span_for_line(line: &Line) -> Span {
    Span {
        start_byte: line.start,
        end_byte: line.end,
        start_line: line.number,
        start_col: 1,
        end_line: line.number,
        end_col: line.text.chars().count() + 1,
    }
}

fn join_span(start: Span, end: Span) -> Span {
    Span {
        start_byte: start.start_byte,
        end_byte: end.end_byte,
        start_line: start.start_line,
        start_col: start.start_col,
        end_line: end.end_line,
        end_col: end.end_col,
    }
}

fn is_ident(src: &str) -> bool {
    let mut chars = src.chars();
    matches!(chars.next(), Some(c) if is_ident_start(c)) && chars.all(is_ident_continue)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_constructor_name(name: &str) -> bool {
    matches!(name, "Some" | "None" | "Ok" | "Err")
}

fn split_top_level_comma(src: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (idx, ch) in src.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((&src[..idx], &src[idx + 1..])),
            _ => {}
        }
    }
    None
}

fn stable_hash(src: &str) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in src.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}
