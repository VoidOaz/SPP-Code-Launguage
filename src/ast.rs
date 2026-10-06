#[derive(Debug, Clone)]
pub struct Program {
    pub package: Option<String>,
    pub imports: Vec<String>,
    pub functions: Vec<Function>,
    pub classes: Vec<Class>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Type,
    pub body: Vec<Stmt>,
    pub method_of: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: String,
    pub fields: Vec<Field>,
    pub methods: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Any,
    Int,
    Float,
    Bool,
    String,
    Void,
    Array(Box<Type>),
    Named(String),
}

impl Type {
    pub fn from_name(name: &str) -> Self {
        match name {
            "any" | "auto" | "var" => Self::Any,
            "int" | "integer" => Self::Int,
            "float" | "double" => Self::Float,
            "bool" | "boolean" => Self::Bool,
            "string" | "str" => Self::String,
            "void" => Self::Void,
            other => Self::Named(other.to_string()),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Any => "any".into(),
            Self::Int => "int".into(),
            Self::Float => "float".into(),
            Self::Bool => "bool".into(),
            Self::String => "string".into(),
            Self::Void => "void".into(),
            Self::Array(inner) => format!("{}[]", inner.name()),
            Self::Named(name) => name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl { ty: Type, name: String, value: Option<Expr> },
    Assign { target: AssignTarget, op: AssignOp, value: Expr },
    Expr(Expr),
    If { branches: Vec<(Expr, Vec<Stmt>)>, else_body: Option<Vec<Stmt>> },
    While { condition: Expr, body: Vec<Stmt> },
    ForEach { name: String, iterable: Expr, body: Vec<Stmt> },
    ForC { init: Option<Box<Stmt>>, condition: Option<Expr>, step: Option<Box<Stmt>>, body: Vec<Stmt> },
    Break,
    Continue,
    Return(Option<Expr>),
}

#[derive(Debug, Clone)]
pub enum AssignTarget {
    Variable(String),
    Index { target: Expr, index: Expr },
    Member { target: Expr, member: String },
}

#[derive(Debug, Clone)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Null,
    Variable(String),
    Array(Vec<Expr>),
    New { class_name: String, args: Vec<Expr> },
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Index { target: Box<Expr>, index: Box<Expr> },
    Member { target: Box<Expr>, member: String },
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
    Positive,
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}
