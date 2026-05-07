use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone)]
pub struct Node<T> {
    pub id: String,
    pub span: Span,
    pub kind: T,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Node<FunctionDecl>>,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub intent: String,
    pub inputs: Vec<StickyBinding>,
    pub output: StickyBinding,
    pub body: Vec<Node<Statement>>,
    pub close_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickyBinding {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub enum Statement {
    Let(LetStmt),
    Set(SetStmt),
    Return(ReturnStmt),
    Loop(LoopBlock),
    Condition(ConditionBlock),
    Expr(Node<Expr>),
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub binding: StickyBinding,
    pub value: Node<Expr>,
}

#[derive(Debug, Clone)]
pub struct SetStmt {
    pub target: String,
    pub value: Node<Expr>,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<Node<Expr>>,
}

#[derive(Debug, Clone)]
pub struct LoopBlock {
    pub item: StickyBinding,
    pub collection: String,
    pub body: Vec<Node<Statement>>,
    pub close_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConditionBlock {
    pub name: String,
    pub guard: Node<GuardStmt>,
}

#[derive(Debug, Clone)]
pub struct GuardStmt {
    pub cond: Node<Expr>,
    pub action: GuardAction,
}

#[derive(Debug, Clone)]
pub enum GuardAction {
    Continue,
    Break,
    Return(Node<Expr>),
    SetAssign { target: String, value: Node<Expr> },
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Literal),
    Ident(String),
    Binary {
        op: BinOp,
        lhs: Box<Node<Expr>>,
        rhs: Box<Node<Expr>>,
    },
    Unary {
        op: UnOp,
        operand: Box<Node<Expr>>,
    },
    Call {
        callee: String,
        args: Vec<Node<Expr>>,
    },
    Cast {
        expr: Box<Node<Expr>>,
        target: Type,
    },
    Ctor(ConstructorExpr),
    Paren(Box<Node<Expr>>),
}

#[derive(Debug, Clone)]
pub enum ConstructorExpr {
    Some(Box<Node<Expr>>),
    None,
    Ok(Box<Node<Expr>>),
    Err(Box<Node<Expr>>),
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    String,
    Unit,
    Vector(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Boxed(Box<Type>),
    Unknown,
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::F32
                | Type::F64
        )
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Unit => write!(f, "unit"),
            Type::Vector(inner) => write!(f, "Vector<{inner}>"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
            Type::Result(ok, err) => write!(f, "Result<{ok},{err}>"),
            Type::Boxed(inner) => write!(f, "Box<{inner}>"),
            Type::Unknown => write!(f, "unknown"),
        }
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Neq => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        };
        write!(f, "{op}")
    }
}

impl fmt::Display for UnOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnOp::Neg => write!(f, "-"),
            UnOp::Not => write!(f, "!"),
        }
    }
}
