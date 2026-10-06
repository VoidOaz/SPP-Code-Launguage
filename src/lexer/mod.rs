use crate::token::{Token, TokenKind};

pub struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    current: Option<char>,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut lexer = Self { chars: source.chars(), current: None, line: 1, column: 0 };
        lexer.advance();
        lexer
    }

    fn advance(&mut self) -> Option<char> {
        let old = self.current;
        self.current = self.chars.next();
        if old == Some('\n') {
            self.line += 1;
            self.column = 0;
        }
        if self.current.is_some() {
            self.column += 1;
        }
        old
    }

    fn peek(&self) -> Option<char> { self.current }

    fn error<T>(&self, msg: impl Into<String>) -> Result<T, String> {
        Err(format!("SPP:{}:{}: {}", self.line, self.column.max(1), msg.into()))
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::with_capacity(256);
        while let Some(ch) = self.peek() {
            let line = self.line;
            let column = self.column.max(1);
            match ch {
                ' ' | '\t' | '\r' | '\n' => { self.advance(); }
                '#' => {
                    while self.peek().is_some() && self.peek() != Some('\n') { self.advance(); }
                }
                '/' => {
                    self.advance();
                    match self.peek() {
                        Some('/') => { while self.peek().is_some() && self.peek() != Some('\n') { self.advance(); } }
                        Some('*') => {
                            self.advance();
                            let mut closed = false;
                            while let Some(c) = self.peek() {
                                if c == '*' {
                                    self.advance();
                                    if self.peek() == Some('/') { self.advance(); closed = true; break; }
                                } else { self.advance(); }
                            }
                            if !closed { return self.error("unterminated block comment"); }
                        }
                        Some('=') => { self.advance(); tokens.push(Token::new(TokenKind::SlashEqual, line, column)); }
                        _ => tokens.push(Token::new(TokenKind::Slash, line, column)),
                    }
                }
                '0'..='9' => tokens.push(self.number(line, column)?),
                '"' | '\'' => tokens.push(self.string(ch, line, column)?),
                '_' | 'a'..='z' | 'A'..='Z' => tokens.push(self.identifier(line, column)),
                '+' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::PlusEqual, line, column)); } else if self.peek() == Some('+') { self.advance(); tokens.push(Token::new(TokenKind::Increment, line, column)); } else { tokens.push(Token::new(TokenKind::Plus, line, column)); } }
                '-' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::MinusEqual, line, column)); } else if self.peek() == Some('-') { self.advance(); tokens.push(Token::new(TokenKind::Decrement, line, column)); } else if self.peek() == Some('>') { self.advance(); tokens.push(Token::new(TokenKind::Arrow, line, column)); } else { tokens.push(Token::new(TokenKind::Minus, line, column)); } }
                '*' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::StarEqual, line, column)); } else { tokens.push(Token::new(TokenKind::Star, line, column)); } }
                '%' => { self.advance(); tokens.push(Token::new(TokenKind::Percent, line, column)); }
                '=' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::Equal, line, column)); } else { tokens.push(Token::new(TokenKind::Assign, line, column)); } }
                '!' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::NotEqual, line, column)); } else { tokens.push(Token::new(TokenKind::Not, line, column)); } }
                '<' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::LessEqual, line, column)); } else { tokens.push(Token::new(TokenKind::Less, line, column)); } }
                '>' => { self.advance(); if self.peek() == Some('=') { self.advance(); tokens.push(Token::new(TokenKind::GreaterEqual, line, column)); } else { tokens.push(Token::new(TokenKind::Greater, line, column)); } }
                '&' => { self.advance(); if self.peek() == Some('&') { self.advance(); tokens.push(Token::new(TokenKind::And, line, column)); } else { return self.error("expected '&' for &&"); } }
                '|' => { self.advance(); if self.peek() == Some('|') { self.advance(); tokens.push(Token::new(TokenKind::Or, line, column)); } else { return self.error("expected '|' for ||"); } }
                '(' => { self.advance(); tokens.push(Token::new(TokenKind::LParen, line, column)); }
                ')' => { self.advance(); tokens.push(Token::new(TokenKind::RParen, line, column)); }
                '{' => { self.advance(); tokens.push(Token::new(TokenKind::LBrace, line, column)); }
                '}' => { self.advance(); tokens.push(Token::new(TokenKind::RBrace, line, column)); }
                '[' => { self.advance(); tokens.push(Token::new(TokenKind::LBracket, line, column)); }
                ']' => { self.advance(); tokens.push(Token::new(TokenKind::RBracket, line, column)); }
                ',' => { self.advance(); tokens.push(Token::new(TokenKind::Comma, line, column)); }
                ':' => { self.advance(); tokens.push(Token::new(TokenKind::Colon, line, column)); }
                '.' => { self.advance(); tokens.push(Token::new(TokenKind::Dot, line, column)); }
                ';' => { self.advance(); tokens.push(Token::new(TokenKind::Semicolon, line, column)); }
                _ => return self.error(format!("unexpected character '{ch}'")),
            }
        }
        tokens.push(Token::new(TokenKind::Eof, self.line, self.column.max(1)));
        Ok(tokens)
    }

    fn number(&mut self, line: usize, column: usize) -> Result<Token, String> {
        let mut text = String::new();
        while matches!(self.peek(), Some('0'..='9')) { text.push(self.advance().unwrap()); }
        let mut is_float = false;
        if self.peek() == Some('.') {
            is_float = true;
            text.push(self.advance().unwrap());
            if !matches!(self.peek(), Some('0'..='9')) { return self.error("expected digits after '.'"); }
            while matches!(self.peek(), Some('0'..='9')) { text.push(self.advance().unwrap()); }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            text.push(self.advance().unwrap());
            if matches!(self.peek(), Some('+' | '-')) { text.push(self.advance().unwrap()); }
            if !matches!(self.peek(), Some('0'..='9')) { return self.error("invalid exponent"); }
            while matches!(self.peek(), Some('0'..='9')) { text.push(self.advance().unwrap()); }
        }
        if is_float {
            let v = text.parse::<f64>().map_err(|_| format!("SPP:{line}:{column}: invalid float '{text}'"))?;
            Ok(Token::new(TokenKind::Float(v), line, column))
        } else {
            let v = text.parse::<i64>().map_err(|_| format!("SPP:{line}:{column}: invalid integer '{text}'"))?;
            Ok(Token::new(TokenKind::Int(v), line, column))
        }
    }

    fn string(&mut self, quote: char, line: usize, column: usize) -> Result<Token, String> {
        self.advance();
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            if ch == quote { self.advance(); return Ok(Token::new(TokenKind::String(out), line, column)); }
            if ch == '\\' {
                self.advance();
                let escaped = match self.peek() {
                    Some('n') => '\n', Some('r') => '\r', Some('t') => '\t', Some('\\') => '\\', Some('"') => '"', Some('\'') => '\'',
                    Some(other) => other, None => return self.error("unterminated string escape"),
                };
                self.advance(); out.push(escaped);
            } else {
                if ch == '\n' { return self.error("newline inside string literal"); }
                self.advance(); out.push(ch);
            }
        }
        self.error("unterminated string literal")
    }

    fn identifier(&mut self, line: usize, column: usize) -> Token {
        let mut name = String::new();
        while matches!(self.peek(), Some('_' | 'a'..='z' | 'A'..='Z' | '0'..='9')) { name.push(self.advance().unwrap()); }
        match name.as_str() {
            "true" => Token::new(TokenKind::Identifier("true".into()), line, column),
            "false" => Token::new(TokenKind::Identifier("false".into()), line, column),
            "null" => Token::new(TokenKind::Identifier("null".into()), line, column),
            _ => Token::new(TokenKind::Identifier(name), line, column),
        }
    }
}
