#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    Package,
    Import,
    Class,
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    New,
    True,
    False,
    Null,
}

impl Keyword {
    pub fn from_ident(name: &str) -> Option<Self> {
        Some(match name {
            "package" => Self::Package,
            "import" => Self::Import,
            "class" => Self::Class,
            "fn" | "function" => Self::Fn,
            "let" => Self::Let,
            "var" => Self::Var,
            "if" => Self::If,
            "else" => Self::Else,
            "while" => Self::While,
            "for" => Self::For,
            "in" => Self::In,
            "break" => Self::Break,
            "continue" => Self::Continue,
            "return" => Self::Return,
            "new" => Self::New,
            "true" => Self::True,
            "false" => Self::False,
            "null" => Self::Null,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Import => "import",
            Self::Class => "class",
            Self::Fn => "fn",
            Self::Let => "let",
            Self::Var => "var",
            Self::If => "if",
            Self::Else => "else",
            Self::While => "while",
            Self::For => "for",
            Self::In => "in",
            Self::Break => "break",
            Self::Continue => "continue",
            Self::Return => "return",
            Self::New => "new",
            Self::True => "true",
            Self::False => "false",
            Self::Null => "null",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Keyword(Keyword),
    Int(i64),
    Float(f64),
    String(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    Not,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    Increment,
    Decrement,
    Arrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Semicolon,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self { kind, line, column }
    }
}
