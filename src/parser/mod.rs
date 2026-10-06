//! Recursive-descent SPP parser with precedence climbing for expressions.
//!
//! Improvements over the previous implementation:
//! - keywords are real token kinds, so reserved words cannot be used as names;
//! - no `TokenKind::clone()` on every peek/advance (comparisons are by reference);
//! - explicit error recovery at statement boundaries so a single typo reports
//!   several errors instead of aborting at the first one;
//! - recursion depth is bounded to protect against stack overflow on hostile input.

use crate::{
    ast::*,
    token::{Keyword, Token, TokenKind},
};

const MAX_DEPTH: usize = 256;

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    depth: usize,
    errors: Vec<String>,
    package_name: Option<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, current: 0, depth: 0, errors: Vec::new(), package_name: None } }

    // ------------------------------------------------------------------
    // Program / top level
    // ------------------------------------------------------------------

    pub fn parse(mut self) -> Result<Program, String> {
        let mut package = None;
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();

        while !self.is_at_end() {
            let result = match self.peek_kind() {
                TokenKind::Keyword(Keyword::Package) => {
                    if package.is_some() || self.package_name.is_some() {
                        Err(self.error("duplicate 'package' declaration"))
                    } else {
                        self.parse_package().map(|_| package = self.package_name.clone())
                    }
                }
                TokenKind::Keyword(Keyword::Import) => self.parse_import(&mut imports),
                TokenKind::Keyword(Keyword::Class) => self.class_declaration().map(|class| classes.push(class)),
                TokenKind::Keyword(Keyword::Fn) => self.function_declaration(None).map(|function| functions.push(function)),
                _ => Err(self.error("expected 'package', 'import', 'class', or 'fn' at top level")),
            };
            if let Err(error) = result {
                // Report all top-level errors we can find, not just the first.
                self.errors.push(error);
                self.synchronize();
            }
        }

        if !self.errors.is_empty() {
            return Err(self.errors.join("\n"));
        }

        let main_count = functions.iter().filter(|f| f.name == "main").count();
        if main_count != 1 {
            return Err(if main_count == 0 {
                "SPP: program must contain exactly one fn main() function".into()
            } else {
                format!("SPP: program must contain exactly one fn main() function, found {main_count}")
            });
        }
        Ok(Program { package, imports, functions, classes })
    }

    fn parse_package(&mut self) -> Result<(), String> {
        self.advance(); // package
        let name = self.consume_identifier("expected package name")?;
        self.consume(&TokenKind::Semicolon, "expected ';' after package name")?;
        self.package_name = Some(name);
        Ok(())
    }

    fn parse_import(&mut self, imports: &mut Vec<String>) -> Result<(), String> {
        self.advance(); // import
        let name = match self.peek_kind() {
            TokenKind::String(s) => { let s = s.clone(); self.advance(); s }
            TokenKind::Identifier(s) => { let s = s.clone(); self.advance(); s }
            _ => return Err(self.error("expected an imported module or .spp file name")),
        };
        self.consume(&TokenKind::Semicolon, "expected ';' after import")?;
        imports.push(name);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Declarations
    // ------------------------------------------------------------------

    fn class_declaration(&mut self) -> Result<Class, String> {
        self.advance(); // class
        let name = self.consume_non_reserved_identifier("expected class name")?;
        self.consume(&TokenKind::LBrace, "expected '{' after class name")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.check_kw(Keyword::Fn) {
                methods.push(self.function_declaration(Some(name.clone()))?);
                continue;
            }
            let ty = self.parse_type()?;
            let field_name = self.consume_non_reserved_identifier("expected field name")?;
            let default = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            self.consume(&TokenKind::Semicolon, "expected ';' after field declaration")?;
            if fields.iter().any(|f: &Field| f.name == field_name) {
                return Err(self.error(format!("duplicate field '{field_name}' in class '{name}'")));
            }
            fields.push(Field { name: field_name, ty, default });
        }
        self.consume(&TokenKind::RBrace, "expected '}' after class body")?;
        Ok(Class { name, fields, methods })
    }

    fn function_declaration(&mut self, method_of: Option<String>) -> Result<Function, String> {
        self.advance(); // fn
        let name = self.consume_non_reserved_identifier("expected function name")?;
        self.consume(&TokenKind::LParen, "expected '(' after function name")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let (param_name, ty) = self.parse_parameter()?;
                if params.iter().any(|p: &Parameter| p.name == param_name) {
                    return Err(self.error(format!("duplicate parameter '{param_name}' in function '{name}'")));
                }
                params.push(Parameter { name: param_name, ty });
                if !self.match_kind(&TokenKind::Comma) { break; }
                if self.check(&TokenKind::RParen) { break; } // trailing comma
            }
        }
        self.consume(&TokenKind::RParen, "expected ')' after parameter list")?;
        let return_type = if self.match_kind(&TokenKind::Arrow) { self.parse_type()? } else { Type::Void };
        self.consume(&TokenKind::LBrace, "expected '{' before function body")?;
        let body = self.block()?;
        Ok(Function { name, params, return_type, body, method_of })
    }

    /// Accepts both `x: int` and C-style `int x` parameter forms.
    fn parse_parameter(&mut self) -> Result<(String, Type), String> {
        let token = self.peek().clone();
        let first = match &token.kind {
            TokenKind::Identifier(s) => s.clone(),
            _ => return Err(self.error_with(token, "expected parameter name or type")),
        };
        if self.check_at(1, &TokenKind::Colon) {
            self.advance(); // name
            self.advance(); // :
            let ty = self.parse_type()?;
            return Ok((first, ty));
        }
        if is_type_keyword(&first) {
            if matches!(self.peek_kind_at(1), TokenKind::Identifier(_)) {
                self.advance(); // type
                let name = self.consume_identifier("expected parameter name")?;
                return Ok((name, self.parse_type_from_name(&first)?));
            }
            return Err(self.error(format!("expected parameter name after type '{first}'")));
        }
        if matches!(self.peek_kind_at(1), TokenKind::Identifier(_)) {
            self.advance(); // implicit `any` type keyword position
            let ty = self.parse_type_from_name(&first)?;
            let name = self.consume_identifier("expected parameter name")?;
            return Ok((name, ty));
        }
        // Bare identifier: untyped parameter.
        self.advance();
        Ok((first, Type::Any))
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.enter_recursion()?;
        let mut statements = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            statements.push(self.statement()?);
        }
        self.consume(&TokenKind::RBrace, "expected '}' after block")?;
        self.leave_recursion();
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        self.enter_recursion()?;
        let stmt = self.statement_inner();
        self.leave_recursion();
        stmt
    }

    fn statement_inner(&mut self) -> Result<Stmt, String> {
        match self.peek_kind() {
            TokenKind::Keyword(Keyword::If) => return self.if_statement(),
            TokenKind::Keyword(Keyword::While) => return self.while_statement(),
            TokenKind::Keyword(Keyword::For) => return self.for_statement(),
            TokenKind::Keyword(Keyword::Break) => {
                self.advance();
                self.optional_semicolon("after break")?;
                return Ok(Stmt::Break);
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.advance();
                self.optional_semicolon("after continue")?;
                return Ok(Stmt::Continue);
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.advance();
                let expr = if self.check(&TokenKind::Semicolon) || self.check(&TokenKind::RBrace) || self.is_at_end() {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.optional_semicolon("after return")?;
                return Ok(Stmt::Return(expr));
            }
            TokenKind::Keyword(Keyword::Let) | TokenKind::Keyword(Keyword::Var) => {
                self.advance();
                let name = self.consume_non_reserved_identifier("expected variable name")?;
                let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
                self.consume(&TokenKind::Semicolon, "expected ';' after variable declaration")?;
                return Ok(Stmt::VarDecl { ty: Type::Any, name, value });
            }
            _ => {}
        }

        if self.at_typed_declaration() {
            let ty = self.parse_type()?;
            let name = self.consume_non_reserved_identifier("expected variable name")?;
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
                _ => return Err(self.error("invalid assignment target")),
            };
            self.advance();
            let value = self.expression()?;
            self.consume(&TokenKind::Semicolon, "expected ';' after assignment")?;
            return Ok(Stmt::Assign { target, op, value });
        }
        if self.match_kind(&TokenKind::Increment) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return Err(self.error("'++' requires a variable")) };
            self.consume(&TokenKind::Semicolon, "expected ';' after increment")?;
            return Ok(Stmt::Assign { target, op: AssignOp::Add, value: Expr::Int(1) });
        }
        if self.match_kind(&TokenKind::Decrement) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return Err(self.error("'--' requires a variable")) };
            self.consume(&TokenKind::Semicolon, "expected ';' after decrement")?;
            return Ok(Stmt::Assign { target, op: AssignOp::Sub, value: Expr::Int(1) });
        }
        self.consume(&TokenKind::Semicolon, "expected ';' after expression statement")?;
        Ok(Stmt::Expr(expr))
    }

    fn if_statement(&mut self) -> Result<Stmt, String> {
        self.advance(); // if
        self.consume(&TokenKind::LParen, "expected '(' after 'if'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RParen, "expected ')' after if condition")?;
        self.consume(&TokenKind::LBrace, "expected '{' after if condition")?;
        let then_body = self.block()?;
        let mut branches = vec![(condition, then_body)];
        let mut else_body = None;
        while self.check_kw(Keyword::Else) {
            self.advance();
            if self.match_kw(Keyword::If) {
                self.consume(&TokenKind::LParen, "expected '(' after 'else if'")?;
                let condition = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after else-if condition")?;
                self.consume(&TokenKind::LBrace, "expected '{' after else-if condition")?;
                let body = self.block()?;
                branches.push((condition, body));
            } else {
                self.consume(&TokenKind::LBrace, "expected '{' after 'else'")?;
                else_body = Some(self.block()?);
                break;
            }
        }
        Ok(Stmt::If { branches, else_body })
    }

    fn while_statement(&mut self) -> Result<Stmt, String> {
        self.advance(); // while
        self.consume(&TokenKind::LParen, "expected '(' after 'while'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RParen, "expected ')' after while condition")?;
        self.consume(&TokenKind::LBrace, "expected '{' after while condition")?;
        Ok(Stmt::While { condition, body: self.block()? })
    }

    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.advance(); // for
        self.consume(&TokenKind::LParen, "expected '(' after 'for'")?;
        if self.looks_like_foreach() {
            let name = self.consume_non_reserved_identifier("expected loop variable")?;
            self.consume_kw(Keyword::In, "expected 'in' in foreach loop")?;
            let iterable = self.expression()?;
            self.consume(&TokenKind::RParen, "expected ')' after foreach header")?;
            self.consume(&TokenKind::LBrace, "expected '{' after foreach header")?;
            return Ok(Stmt::ForEach { name, iterable, body: self.block()? });
        }
        let init = if self.check(&TokenKind::Semicolon) { self.advance(); None } else { Some(Box::new(self.for_clause()?)) };
        let condition = if self.check(&TokenKind::Semicolon) { None } else { Some(self.expression()?) };
        self.consume(&TokenKind::Semicolon, "expected ';' in for header")?;
        let step = if self.check(&TokenKind::RParen) { None } else { Some(Box::new(self.for_clause()?)) };
        self.consume(&TokenKind::RParen, "expected ')' after for header")?;
        self.consume(&TokenKind::LBrace, "expected '{' after for header")?;
        Ok(Stmt::ForC { init, condition, step, body: self.block()? })
    }

    fn looks_like_foreach(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Identifier(_)) && self.check_at(1, &TokenKind::Keyword(Keyword::In))
    }

    /// For-init / for-step clause: declaration, assignment, ++/-- or expression.
    /// Note: these clauses never consume their own semicolon.
    fn for_clause(&mut self) -> Result<Stmt, String> {
        if matches!(self.peek_kind(), TokenKind::Keyword(Keyword::Let) | TokenKind::Keyword(Keyword::Var)) {
            self.advance();
            let name = self.consume_non_reserved_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            return Ok(Stmt::VarDecl { ty: Type::Any, name, value });
        }
        if self.at_typed_declaration() {
            let ty = self.parse_type()?;
            let name = self.consume_non_reserved_identifier("expected variable name")?;
            let value = if self.match_kind(&TokenKind::Assign) { Some(self.expression()?) } else { None };
            return Ok(Stmt::VarDecl { ty, name, value });
        }
        let expr = self.expression()?;
        if let Some(op) = self.assignment_operator() {
            let target = match expr {
                Expr::Variable(name) => AssignTarget::Variable(name),
                Expr::Index { target, index } => AssignTarget::Index { target: *target, index: *index },
                Expr::Member { target, member } => AssignTarget::Member { target: *target, member },
                _ => return Err(self.error("invalid assignment target in for clause")),
            };
            self.advance();
            let value = self.expression()?;
            return Ok(Stmt::Assign { target, op, value });
        }
        if self.match_kind(&TokenKind::Increment) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return Err(self.error("'++' requires a variable")) };
            return Ok(Stmt::Assign { target, op: AssignOp::Add, value: Expr::Int(1) });
        }
        if self.match_kind(&TokenKind::Decrement) {
            let target = match expr { Expr::Variable(name) => AssignTarget::Variable(name), _ => return Err(self.error("'--' requires a variable")) };
            return Ok(Stmt::Assign { target, op: AssignOp::Sub, value: Expr::Int(1) });
        }
        Ok(Stmt::Expr(expr))
    }

    // ------------------------------------------------------------------
    // Expressions — precedence climbing (no per-level function chain)
    // ------------------------------------------------------------------

    fn expression(&mut self) -> Result<Expr, String> { self.parse_binary(0) }

    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, String> {
        self.enter_recursion()?;
        let mut left = self.unary()?;
        while let Some((op, precedence, right_assoc)) = self.infix_operator() {
            if precedence < min_precedence { break; }
            self.advance();
            let next_min = if right_assoc { precedence } else { precedence + 1 };
            let right = self.parse_binary(next_min)?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        self.leave_recursion();
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        self.enter_recursion()?;
        let result = if self.match_kind(&TokenKind::Minus) {
            self.unary().map(|expr| Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr) })
        } else if self.match_kind(&TokenKind::Plus) {
            self.unary().map(|expr| Expr::Unary { op: UnaryOp::Positive, expr: Box::new(expr) })
        } else if self.match_kind(&TokenKind::Not) {
            self.unary().map(|expr| Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr) })
        } else {
            self.postfix()
        };
        self.leave_recursion();
        result
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        loop {
            if self.match_kind(&TokenKind::LParen) {
                let args = self.arguments()?;
                expr = Expr::Call { callee: Box::new(expr), args };
            } else if self.match_kind(&TokenKind::LBracket) {
                let index = self.expression()?;
                self.consume(&TokenKind::RBracket, "expected ']' after index expression")?;
                expr = Expr::Index { target: Box::new(expr), index: Box::new(index) };
            } else if self.match_kind(&TokenKind::Dot) {
                let member = self.consume_any_word("expected member name after '.'")?;
                expr = Expr::Member { target: Box::new(expr), member };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self.peek();
        let kind = token.kind.clone();
        match kind {
            TokenKind::Int(v) => { self.advance(); Ok(Expr::Int(v)) }
            TokenKind::Float(v) => { self.advance(); Ok(Expr::Float(v)) }
            TokenKind::String(s) => { self.advance(); Ok(Expr::String(s)) }
            TokenKind::Keyword(Keyword::True) => { self.advance(); Ok(Expr::Bool(true)) }
            TokenKind::Keyword(Keyword::False) => { self.advance(); Ok(Expr::Bool(false)) }
            TokenKind::Keyword(Keyword::Null) => { self.advance(); Ok(Expr::Null) }
            TokenKind::Keyword(Keyword::New) => {
                self.advance();
                let class_name = self.consume_any_word("expected class name after 'new'")?;
                self.consume(&TokenKind::LParen, "expected '(' after class name in 'new' expression")?;
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
                        if self.check(&TokenKind::RBracket) { break; } // trailing comma
                    }
                }
                self.consume(&TokenKind::RBracket, "expected ']' after array literal")?;
                Ok(Expr::Array(items))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after parenthesized expression")?;
                Ok(expr)
            }
            _ => Err(self.error_with(token.clone(), "expected expression")),
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                args.push(self.expression()?);
                if !self.match_kind(&TokenKind::Comma) { break; }
                if self.check(&TokenKind::RParen) { break; } // trailing comma
            }
        }
        self.consume(&TokenKind::RParen, "expected ')' after argument list")?;
        Ok(args)
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    fn parse_type(&mut self) -> Result<Type, String> {
        let name = self.consume_any_word("expected type name")?;
        self.parse_type_from_name(&name)
    }

    fn parse_type_from_name(&mut self, name: &str) -> Result<Type, String> {
        let mut ty = Type::from_name(name);
        while self.match_kind(&TokenKind::LBracket) {
            self.consume(&TokenKind::RBracket, "expected ']' in array type")?;
            ty = Type::Array(Box::new(ty));
        }
        Ok(ty)
    }

    /// True when the parser sits on something that *starts* a type annotation.
    fn at_type_start(&self) -> bool {
        match self.peek_kind() {
            TokenKind::Identifier(s) if is_type_keyword(s) => true,
            // User class names are only treated as types when followed by an
            // identifier AND then `=` or `;` (a real declaration shape).
            TokenKind::Identifier(name) => {
                !is_reserved_word(name) && matches!(self.peek_kind_at(1), TokenKind::Identifier(_))
            }
            _ => false,
        }
    }

    /// Statement-level check: a type-ish prefix must be followed by `name =` or
    /// `name ;` to actually be a typed declaration. This keeps calls like
    /// `factorial(8);` from being misparsed as declarations.
    fn at_typed_declaration(&self) -> bool {
        if !self.at_type_start() { return false; }
        let mut offset = 1;
        if !matches!(self.peek_kind_at(offset), TokenKind::Identifier(_)) { return false; }
        offset += 1;
        // Optional array suffixes on the type: `int[] x` handled inside parse_type;
        // here the trailing token after the name decides.
        while matches!(self.peek_kind_at(offset), TokenKind::LBracket) && matches!(self.peek_kind_at(offset + 1), TokenKind::RBracket) {
            offset += 2;
        }
        matches!(self.peek_kind_at(offset), TokenKind::Assign | TokenKind::Semicolon | TokenKind::RParen)
    }

    // ------------------------------------------------------------------
    // Token helpers
    // ------------------------------------------------------------------

    #[inline]
    fn peek(&self) -> &Token { &self.tokens[self.current.min(self.tokens.len() - 1)] }
    #[inline]
    fn peek_kind(&self) -> &TokenKind { &self.peek().kind }
    #[inline]
    fn peek_kind_at(&self, offset: usize) -> &TokenKind {
        let index = (self.current + offset).min(self.tokens.len() - 1);
        &self.tokens[index].kind
    }
    #[inline]
    fn check(&self, kind: &TokenKind) -> bool { self.peek_kind() == kind }
    #[inline]
    fn check_at(&self, offset: usize, kind: &TokenKind) -> bool { self.peek_kind_at(offset) == kind }
    #[inline]
    fn check_kw(&self, kw: Keyword) -> bool { matches!(self.peek_kind(), TokenKind::Keyword(k) if *k == kw) }
    #[inline]
    fn match_kw(&mut self, kw: Keyword) -> bool { if self.check_kw(kw) { self.advance(); true } else { false } }
    #[inline]
    fn match_kind(&mut self, kind: &TokenKind) -> bool { if self.check(kind) { self.advance(); true } else { false } }
    #[inline]
    fn advance(&mut self) { if !self.is_at_end() { self.current += 1; } }
    #[inline]
    fn is_at_end(&self) -> bool { matches!(self.peek_kind(), TokenKind::Eof) }

    fn consume(&mut self, expected: &TokenKind, message: &str) -> Result<(), String> {
        if self.check(expected) { self.advance(); Ok(()) }
        else { Err(self.error(message)) }
    }

    fn consume_kw(&mut self, kw: Keyword, message: &str) -> Result<(), String> {
        if self.match_kw(kw) { Ok(()) } else { Err(self.error(message)) }
    }

    fn consume_identifier(&mut self, message: &str) -> Result<String, String> {
        match self.peek_kind() {
            TokenKind::Identifier(s) => { let s = s.clone(); self.advance(); Ok(s) }
            _ => Err(self.error(message)),
        }
    }

    /// Identifier that must not collide with a reserved word.
    fn consume_non_reserved_identifier(&mut self, message: &str) -> Result<String, String> {
        match self.peek_kind() {
            TokenKind::Identifier(s) => { let s = s.clone(); self.advance(); Ok(s) }
            TokenKind::Keyword(kw) => Err(self.error(format!("'{}' is a reserved keyword and cannot be used as a name ({message})", kw.as_str()))),
            _ => Err(self.error(message)),
        }
    }

    /// Identifiers or contextual keywords (type names like `int` are lexed as
    /// identifiers today, but member names may look keyword-ish).
    fn consume_any_word(&mut self, message: &str) -> Result<String, String> {
        match self.peek_kind() {
            TokenKind::Identifier(s) => { let s = s.clone(); self.advance(); Ok(s) }
            TokenKind::Keyword(kw) => { let s = kw.as_str().to_string(); self.advance(); Ok(s) }
            _ => Err(self.error(message)),
        }
    }

    /// Semicolons are required except immediately before `}` where they may be
    /// omitted for ergonomic control-flow statements.
    fn optional_semicolon(&mut self, context: &str) -> Result<(), String> {
        if self.match_kind(&TokenKind::Semicolon) { return Ok(()); }
        if self.check(&TokenKind::RBrace) || self.is_at_end() { return Ok(()); }
        Err(self.error(format!("expected ';' {context}")))
    }

    fn assignment_operator(&self) -> Option<AssignOp> {
        Some(match self.peek_kind() {
            TokenKind::Assign => AssignOp::Set,
            TokenKind::PlusEqual => AssignOp::Add,
            TokenKind::MinusEqual => AssignOp::Sub,
            TokenKind::StarEqual => AssignOp::Mul,
            TokenKind::SlashEqual => AssignOp::Div,
            _ => return None,
        })
    }

    fn infix_operator(&self) -> Option<(BinaryOp, u8, bool)> {
        Some(match *self.peek_kind() {
            TokenKind::Or => (BinaryOp::Or, 1, false),
            TokenKind::And => (BinaryOp::And, 2, false),
            TokenKind::Equal | TokenKind::NotEqual => (match self.peek_kind() { TokenKind::Equal => BinaryOp::Equal, _ => BinaryOp::NotEqual }, 3, false),
            TokenKind::Less | TokenKind::LessEqual | TokenKind::Greater | TokenKind::GreaterEqual => {
                let op = match self.peek_kind() {
                    TokenKind::Less => BinaryOp::Less,
                    TokenKind::LessEqual => BinaryOp::LessEqual,
                    TokenKind::Greater => BinaryOp::Greater,
                    _ => BinaryOp::GreaterEqual,
                };
                (op, 4, false)
            }
            TokenKind::Plus => (BinaryOp::Add, 5, false),
            TokenKind::Minus => (BinaryOp::Sub, 5, false),
            TokenKind::Star => (BinaryOp::Mul, 6, false),
            TokenKind::Slash => (BinaryOp::Div, 6, false),
            TokenKind::Percent => (BinaryOp::Mod, 6, false),
            _ => return None,
        })
    }

    fn enter_recursion(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("expression/statement nesting too deep (limit 256)"));
        }
        Ok(())
    }

    #[inline]
    fn leave_recursion(&mut self) { self.depth -= 1; }

    fn error(&self, message: impl Into<String>) -> String {
        let token = self.peek();
        format!("SPP:{}:{}: {}", token.line, token.column, message.into())
    }

    fn error_with(&self, token: Token, message: impl Into<String>) -> String {
        format!("SPP:{}:{}: {}", token.line, token.column, message.into())
    }

    /// Skip tokens until the next plausible statement boundary. Keeps parsing
    /// after an error so multiple independent mistakes can be reported at once.
    fn synchronize(&mut self) {
        while !self.is_at_end() {
            if matches!(self.peek_kind(), TokenKind::Semicolon) { self.advance(); return; }
            if matches!(self.peek_kind(), TokenKind::Keyword(
                Keyword::Fn | Keyword::Class | Keyword::Import | Keyword::Package | Keyword::Let | Keyword::Var | Keyword::Return
            )) {
                return;
            }
            self.advance();
        }
    }
}

fn is_type_keyword(name: &str) -> bool {
    matches!(name, "any" | "auto" | "var" | "int" | "integer" | "float" | "double" | "bool" | "boolean" | "string" | "str" | "void")
}

fn is_reserved_word(name: &str) -> bool {
    Keyword::from_ident(name).is_some() || is_type_keyword(name)
}
