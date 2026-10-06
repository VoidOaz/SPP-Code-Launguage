use crate::{ast::*, token::{Token, TokenKind}};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, current: 0 } }

    pub fn parse(mut self) -> Result<Program, String> {
        let mut package = None;
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        while !self.is_at_end() {
            if self.check_id("package") {
                self.advance();
                let name = self.consume_identifier("expected package name")?;
                self.consume(&TokenKind::Semicolon, "expected ';' after package")?;
                package = Some(name);
            } else if self.check_id("import") {
                self.advance();
                let name = match self.peek().kind.clone() {
                    TokenKind::String(s) => { self.advance(); s }
                    TokenKind::Identifier(s) => { self.advance(); s }
                    _ => return self.error("expected imported .spp filename")
                };
                self.consume(&TokenKind::Semicolon, "expected ';' after import")?;
                imports.push(name);
            } else if self.check_id("class") {
                classes.push(self.class_declaration()?);
            } else if self.check_id("fn") || self.check_id("function") {
                functions.push(self.function_declaration(None)?);
            } else {
                return self.error("expected package, import, class, or fn at top level");
            }
        }
        if functions.iter().filter(|f| f.name == "main").count() != 1 {
            return self.error("SPP program must contain exactly one fn main() function");
        }
        Ok(Program { package, imports, functions, classes })
    }

    fn class_declaration(&mut self) -> Result<Class, String> {
        self.advance();
        let name = self.consume_identifier("expected class name")?;
        self.consume(&TokenKind::LBrace, "expected '{' after class name")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.check_id("fn") || self.check_id("function") {
                methods.push(self.function_declaration(Some(name.clone()))?);
                continue;
            }
            let ty = self.parse_type()?;
            let field_name = self.consume_identifier("expected field name")?;
            let default = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after field")?;
            fields.push(Field { name: field_name, ty, default });
        }
        self.consume(&TokenKind::RBrace, "expected '}' after class")?;
        Ok(Class { name, fields, methods })
    }

    fn function_declaration(&mut self, method_of: Option<String>) -> Result<Function, String> {
        self.advance();
        let name = self.consume_identifier("expected function name")?;
        self.consume(&TokenKind::LParen, "expected '(' after function name")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let (param_name, ty) = self.parse_parameter()?;
                params.push(Parameter { name: param_name, ty });
                if !self.match_kind(&TokenKind::Comma) { break; }
            }
        }
        self.consume(&TokenKind::RParen, "expected ')' after parameters")?;
        let return_type = if self.match_kind(&TokenKind::Arrow) { self.parse_type()? } else { Type::Void };
        self.consume(&TokenKind::LBrace, "expected '{' before function body")?;
        let body = self.block()?;
        Ok(Function { name, params, return_type, body, method_of })
    }

    fn parse_parameter(&mut self) -> Result<(String, Type), String> {
        let first = self.consume_identifier("expected parameter name or type")?;
        if self.check(&TokenKind::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok((first, ty));
        }
        if matches!(self.peek_kind(), Some(TokenKind::Identifier(_))) {
            let ty = self.parse_type_from_name(first)?;
            let name = self.consume_identifier("expected parameter name")?;
            return Ok((name, ty));
        }
        if is_type_name(&first) {
            return Err(format!("SPP:{}:{}: expected parameter name after type '{}'", self.peek().line, self.peek().column, first));
        }
        Err(format!("SPP:{}:{}: expected ':' or parameter name after '{}'", self.peek().line, self.peek().column, first))
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() { statements.push(self.statement()?); }
        self.consume(&TokenKind::RBrace, "expected '}' after block")?;
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        if self.match_keyword("if") { return self.if_statement(); }
        if self.match_keyword("while") { return self.while_statement(); }
        if self.match_keyword("for") { return self.for_statement(); }
        if self.match_keyword("break") { self.consume(&TokenKind::Semicolon, "expected ';' after break")?; return Ok(Stmt::Break); }
        if self.match_keyword("continue") { self.consume(&TokenKind::Semicolon, "expected ';' after continue")?; return Ok(Stmt::Continue); }
        if self.match_keyword("return") {
            let expr = if self.check(&TokenKind::Semicolon) { None } else { Some(self.expression()?) };
            self.consume(&TokenKind::Semicolon, "expected ';' after return")?;
            return Ok(Stmt::Return(expr));
        }
        if self.check_id("let") || self.check_id("var") {
            self.advance();
            let name = self.consume_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after variable declaration")?;
            return Ok(Stmt::VarDecl { ty: Type::Any, name, value });
        }
        if self.check_type_name() {
            let ty = self.parse_type()?;
            let name = self.consume_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after variable declaration")?;
            return Ok(Stmt::VarDecl { ty, name, value });
        }

        let expr = self.expression()?;
        if let Some(op) = self.assignment_operator() {
            let target = match expr {
                Expr::Variable(name) => AssignTarget::Variable(name),
                Expr::Index { target, index } => AssignTarget::Index { target: *target, index: *index },
                Expr::Member { target, member } => AssignTarget::Member { target: *target, member },
                _ => return self.error("invalid assignment target"),
            };
            self.advance();
            let value = self.expression()?;
            self.consume(&TokenKind::Semicolon, "expected ';' after assignment")?;
            return Ok(Stmt::Assign { target, op, value });
        }

        self.consume(&TokenKind::Semicolon, "expected ';' after expression")?;
        Ok(Stmt::Expr(expr))
    }

    fn if_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LParen, "expected '(' after if")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RParen, "expected ')' after if condition")?;
        self.consume(&TokenKind::LBrace, "expected '{' after if condition")?;
        let then_body = self.block()?;
        let mut branches = vec![(condition, then_body)];
        let mut else_body = None;
        while self.match_keyword("else") {
            if self.match_keyword("if") {
                self.consume(&TokenKind::LParen, "expected '(' after else if")?;
                let condition = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after else if condition")?;
                self.consume(&TokenKind::LBrace, "expected '{' after else if condition")?;
                branches.push((condition, self.block()?));
            } else {
                self.consume(&TokenKind::LBrace, "expected '{' after else")?;
                else_body = Some(self.block()?);
                break;
            }
        }
        Ok(Stmt::If { branches, else_body })
    }

    fn while_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LParen, "expected '(' after while")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RParen, "expected ')' after while condition")?;
        self.consume(&TokenKind::LBrace, "expected '{' after while condition")?;
        Ok(Stmt::While { condition, body: self.block()? })
    }

    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.consume(&TokenKind::LParen, "expected '(' after for")?;
        if self.looks_like_foreach() {
            let name = self.consume_identifier("expected loop variable")?;
            self.consume_id("in", "expected 'in' in foreach")?;
            let iterable = self.expression()?;
            self.consume(&TokenKind::RParen, "expected ')' after foreach")?;
            self.consume(&TokenKind::LBrace, "expected '{' after foreach")?;
            return Ok(Stmt::ForEach { name, iterable, body: self.block()? });
        }
        let init = if self.check(&TokenKind::Semicolon) { self.advance(); None } else { Some(Box::new(self.for_clause()?)) };
        let condition = if self.check(&TokenKind::Semicolon) { None } else { Some(self.expression()?) };
        self.consume(&TokenKind::Semicolon, "expected ';' in for")?;
        let step = if self.check(&TokenKind::RParen) { None } else { Some(Box::new(self.for_clause()?)) };
        self.consume(&TokenKind::RParen, "expected ')' after for")?;
        self.consume(&TokenKind::LBrace, "expected '{' after for")?;
        Ok(Stmt::ForC { init, condition, step, body: self.block()? })
    }

    fn looks_like_foreach(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Identifier(_)))
            && matches!(self.peek_kind_at(1), Some(TokenKind::Identifier(ref s)) if s == "in")
    }

    fn for_clause(&mut self) -> Result<Stmt, String> {
        if self.check_id("let") || self.check_id("var") {
            self.advance();
            let name = self.consume_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after for initializer")?;
            return Ok(Stmt::VarDecl { ty: Type::Any, name, value });
        }
        if self.check_type_name() {
            let ty = self.parse_type()?;
            let name = self.consume_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after for initializer")?;
            return Ok(Stmt::VarDecl { ty, name, value });
        }
        let expr = self.expression()?;
        if let Some(op) = self.assignment_operator() {
            let target = match expr {
                Expr::Variable(name) => AssignTarget::Variable(name),
                Expr::Index { target, index } => AssignTarget::Index { target: *target, index: *index },
                Expr::Member { target, member } => AssignTarget::Member { target: *target, member },
                _ => return self.error("invalid assignment target in for clause"),
            };
            self.advance();
            let value = self.expression()?;
            return Ok(Stmt::Assign { target, op, value });
        }
        if self.match_kind(&TokenKind::Increment) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return self.error("++ requires a variable") };
            return Ok(Stmt::Assign { target, op: AssignOp::Add, value: Expr::Int(1) });
        }
        if self.match_kind(&TokenKind::Decrement) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return self.error("-- requires a variable") };
            return Ok(Stmt::Assign { target, op: AssignOp::Sub, value: Expr::Int(1) });
        }
        Ok(Stmt::Expr(expr))
    }

    fn expression(&mut self) -> Result<Expr, String> { self.logical_or() }

    fn logical_or(&mut self) -> Result<Expr, String> { self.binary_level(Self::logical_and, &[(TokenKind::Or, BinaryOp::Or)]) }
    fn logical_and(&mut self) -> Result<Expr, String> { self.binary_level(Self::equality, &[(TokenKind::And, BinaryOp::And)]) }
    fn equality(&mut self) -> Result<Expr, String> { self.binary_level(Self::comparison, &[(TokenKind::Equal, BinaryOp::Equal), (TokenKind::NotEqual, BinaryOp::NotEqual)]) }
    fn comparison(&mut self) -> Result<Expr, String> { self.binary_level(Self::term, &[(TokenKind::Less, BinaryOp::Less), (TokenKind::LessEqual, BinaryOp::LessEqual), (TokenKind::Greater, BinaryOp::Greater), (TokenKind::GreaterEqual, BinaryOp::GreaterEqual)]) }
    fn term(&mut self) -> Result<Expr, String> { self.binary_level(Self::factor, &[(TokenKind::Plus, BinaryOp::Add), (TokenKind::Minus, BinaryOp::Sub)]) }
    fn factor(&mut self) -> Result<Expr, String> { self.binary_level(Self::unary, &[(TokenKind::Star, BinaryOp::Mul), (TokenKind::Slash, BinaryOp::Div), (TokenKind::Percent, BinaryOp::Mod)]) }

    fn binary_level<F>(&mut self, next: F, ops: &[(TokenKind, BinaryOp)]) -> Result<Expr, String>
    where F: Fn(&mut Self) -> Result<Expr, String> {
        let mut expr = next(self)?;
        loop {
            let found = ops.iter().find(|(kind, _)| self.check(kind)).cloned();
            let Some((_, op)) = found else { break };
            self.advance();
            let right = next(self)?;
            expr = Expr::Binary { left: Box::new(expr), op, right: Box::new(right) };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.match_kind(&TokenKind::Minus) { return Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(self.unary()?) }); }
        if self.match_kind(&TokenKind::Plus) { return Ok(Expr::Unary { op: UnaryOp::Positive, expr: Box::new(self.unary()?) }); }
        if self.match_kind(&TokenKind::Not) { return Ok(Expr::Unary { op: UnaryOp::Not, expr: Box::new(self.unary()?) }); }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        loop {
            if self.match_kind(&TokenKind::LParen) {
                let args = self.arguments()?;
                expr = Expr::Call { callee: Box::new(expr), args };
            } else if self.match_kind(&TokenKind::LBracket) {
                let index = self.expression()?;
                self.consume(&TokenKind::RBracket, "expected ']' after index")?;
                expr = Expr::Index { target: Box::new(expr), index: Box::new(index) };
            } else if self.match_kind(&TokenKind::Dot) {
                let member = self.consume_identifier("expected member name after '.'")?;
                expr = Expr::Member { target: Box::new(expr), member };
            } else { break; }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(v) => { self.advance(); Ok(Expr::Int(v)) }
            TokenKind::Float(v) => { self.advance(); Ok(Expr::Float(v)) }
            TokenKind::String(s) => { self.advance(); Ok(Expr::String(s)) }
            TokenKind::Identifier(name) if name == "true" => { self.advance(); Ok(Expr::Bool(true)) }
            TokenKind::Identifier(name) if name == "false" => { self.advance(); Ok(Expr::Bool(false)) }
            TokenKind::Identifier(name) if name == "null" => { self.advance(); Ok(Expr::Null) }
            TokenKind::Identifier(name) if name == "new" => {
                self.advance();
                let class_name = self.consume_identifier("expected class name after new")?;
                self.consume(&TokenKind::LParen, "expected '(' after class name")?;
                let args = self.arguments()?;
                Ok(Expr::New { class_name, args })
            }
            TokenKind::Identifier(name) => { self.advance(); Ok(Expr::Variable(name)) }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        items.push(self.expression()?);
                        if !self.match_kind(&TokenKind::Comma) { break; }
                    }
                }
                self.consume(&TokenKind::RBracket, "expected ']' after array literal")?;
                Ok(Expr::Array(items))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after expression")?;
                Ok(expr)
            }
            _ => self.error("expected expression"),
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                args.push(self.expression()?);
                if !self.match_kind(&TokenKind::Comma) { break; }
                if self.check(&TokenKind::RParen) { break; }
            }
        }
        self.consume(&TokenKind::RParen, "expected ')' after arguments")?;
        Ok(args)
    }

    fn parse_type(&mut self) -> Result<Type, String> {
        let name = self.consume_identifier("expected type name")?;
        self.parse_type_from_name(name)
    }

    fn parse_type_from_name(&mut self, name: String) -> Result<Type, String> {
        let mut ty = Type::from_name(&name);
        while self.match_kind(&TokenKind::LBracket) {
            self.consume(&TokenKind::RBracket, "expected ']' in array type")?;
            ty = Type::Array(Box::new(ty));
        }
        Ok(ty)
    }

    fn check_type_name(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Identifier(s) if is_type_name(s) => true,
            TokenKind::Identifier(_) => matches!(self.peek_kind_at(1), Some(TokenKind::Identifier(_))),
            _ => false,
        }
    }

    fn consume_identifier(&mut self, msg: &str) -> Result<String, String> {
        match self.peek().kind.clone() {
            TokenKind::Identifier(s) => { self.advance(); Ok(s) }
            _ => self.error(msg),
        }
    }

    fn consume_id(&mut self, expected: &str, msg: &str) -> Result<(), String> {
        if self.check_id(expected) { self.advance(); Ok(()) } else { self.error(msg) }
    }

    fn consume(&mut self, expected: &TokenKind, msg: &str) -> Result<(), String> {
        if self.check(expected) { self.advance(); Ok(()) } else { self.error(format!("{msg}; found {:?}", self.peek().kind)) }
    }

    fn assignment_operator(&self) -> Option<AssignOp> {
        match self.peek_kind() {
            Some(TokenKind::Assign) => Some(AssignOp::Set),
            Some(TokenKind::PlusEqual) => Some(AssignOp::Add),
            Some(TokenKind::MinusEqual) => Some(AssignOp::Sub),
            Some(TokenKind::StarEqual) => Some(AssignOp::Mul),
            Some(TokenKind::SlashEqual) => Some(AssignOp::Div),
            _ => None,
        }
    }

    fn match_keyword(&mut self, word: &str) -> bool { if self.check_id(word) { self.advance(); true } else { false } }
    fn check_id(&self, word: &str) -> bool { matches!(&self.peek().kind, TokenKind::Identifier(s) if s == word) }
    fn match_kind(&mut self, kind: &TokenKind) -> bool { if self.check(kind) { self.advance(); true } else { false } }
    fn check(&self, kind: &TokenKind) -> bool { self.peek().kind == *kind }
    fn peek(&self) -> &Token { &self.tokens[self.current] }
    fn peek_kind(&self) -> Option<TokenKind> { Some(self.peek().kind.clone()) }
    fn peek_kind_at(&self, offset: usize) -> Option<TokenKind> { self.tokens.get(self.current + offset).map(|t| t.kind.clone()) }
    fn advance(&mut self) -> &Token { if !self.is_at_end() { self.current += 1; } &self.tokens[self.current - 1] }
    fn is_at_end(&self) -> bool { matches!(self.peek().kind, TokenKind::Eof) }
    fn error<T>(&self, msg: impl Into<String>) -> Result<T, String> {
        let t = self.peek();
        Err(format!("SPP:{}:{}: {}", t.line, t.column, msg.into()))
    }
}

fn is_type_name(name: &str) -> bool {
    matches!(name, "any" | "auto" | "var" | "int" | "integer" | "float" | "double" | "bool" | "boolean" | "string" | "str" | "void")
}
