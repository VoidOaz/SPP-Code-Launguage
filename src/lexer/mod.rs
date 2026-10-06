//! High-throughput SPP lexer.
//!
//! The lexer operates directly on the UTF-8 byte stream instead of a
//! `Vec<char>`, which removes one full allocation + copy of the source and
//! lets every hot branch be a plain byte comparison. ASCII identifiers and
//! numbers never touch the UTF-8 decoding path at all.

use crate::token::{Keyword, Token, TokenKind};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    /// Byte offset where the current line started, used for column reporting.
    line_start: usize,
}

/// Maximum nesting depth for block comments (`/* /* */ */` is supported).
const MAX_COMMENT_DEPTH: u32 = 64;

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { src: source.as_bytes(), pos: 0, line: 1, line_start: 0 }
    }

    #[inline(always)]
    fn peek(&self) -> Option<u8> { self.src.get(self.pos).copied() }

    #[inline(always)]
    fn peek_at(&self, offset: usize) -> Option<u8> { self.src.get(self.pos + offset).copied() }

    #[inline(always)]
    fn bump(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        Some(ch)
    }

    #[inline]
    fn column(&self) -> usize { self.pos.saturating_sub(self.line_start) + 1 }

    fn error<T>(&self, msg: impl Into<String>) -> Result<T, String> {
        Err(format!("SPP:{}:{}: {}", self.line, self.column(), msg.into()))
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        // ~1 token per 4 source bytes is a good heuristic for real programs.
        let mut tokens = Vec::with_capacity((self.src.len() / 4).max(16));
        while self.pos < self.src.len() {
            let start_line = self.line;
            let start_col = self.column();
            let ch = self.src[self.pos];
            match ch {
                b' ' | b'\t' | b'\r' | b'\n' => { self.bump(); }
                b'#' => { self.skip_line_comment(); }
                b'/' => {
                    match self.peek_at(1) {
                        Some(b'/') => { self.skip_line_comment(); }
                        Some(b'*') => { self.block_comment(start_line)?; }
                        Some(b'=') => { self.pos += 2; tokens.push(Token::new(TokenKind::SlashEqual, start_line, start_col)); }
                        _ => { self.pos += 1; tokens.push(Token::new(TokenKind::Slash, start_line, start_col)); }
                    }
                }
                b'0'..=b'9' => tokens.push(self.number(start_line, start_col)?),
                b'"' | b'\'' => tokens.push(self.string(ch, start_line, start_col)?),
                b'_' | b'a'..=b'z' | b'A'..=b'Z' => tokens.push(self.identifier(start_line, start_col)),
                b'+' => { self.pos += 1; match self.peek() {
                        Some(b'=') => { self.pos += 1; tokens.push(Token::new(TokenKind::PlusEqual, start_line, start_col)); }
                        Some(b'+') => { self.pos += 1; tokens.push(Token::new(TokenKind::Increment, start_line, start_col)); }
                        _ => tokens.push(Token::new(TokenKind::Plus, start_line, start_col)),
                    } }
                b'-' => { self.pos += 1; match self.peek() {
                        Some(b'=') => { self.pos += 1; tokens.push(Token::new(TokenKind::MinusEqual, start_line, start_col)); }
                        Some(b'-') => { self.pos += 1; tokens.push(Token::new(TokenKind::Decrement, start_line, start_col)); }
                        Some(b'>') => { self.pos += 1; tokens.push(Token::new(TokenKind::Arrow, start_line, start_col)); }
                        _ => tokens.push(Token::new(TokenKind::Minus, start_line, start_col)),
                    } }
                b'*' => { self.pos += 1; if self.peek() == Some(b'=') { self.pos += 1; tokens.push(Token::new(TokenKind::StarEqual, start_line, start_col)); } else { tokens.push(Token::new(TokenKind::Star, start_line, start_col)); } }
                b'%' => { self.pos += 1; tokens.push(Token::new(TokenKind::Percent, start_line, start_col)); }
                b'=' => { self.pos += 1; if self.peek() == Some(b'=') { self.pos += 1; tokens.push(Token::new(TokenKind::Equal, start_line, start_col)); } else { tokens.push(Token::new(TokenKind::Assign, start_line, start_col)); } }
                b'!' => { self.pos += 1; if self.peek() == Some(b'=') { self.pos += 1; tokens.push(Token::new(TokenKind::NotEqual, start_line, start_col)); } else { tokens.push(Token::new(TokenKind::Not, start_line, start_col)); } }
                b'<' => { self.pos += 1; if self.peek() == Some(b'=') { self.pos += 1; tokens.push(Token::new(TokenKind::LessEqual, start_line, start_col)); } else { tokens.push(Token::new(TokenKind::Less, start_line, start_col)); } }
                b'>' => { self.pos += 1; if self.peek() == Some(b'=') { self.pos += 1; tokens.push(Token::new(TokenKind::GreaterEqual, start_line, start_col)); } else { tokens.push(Token::new(TokenKind::Greater, start_line, start_col)); } }
                b'&' => { self.pos += 1; if self.peek() == Some(b'&') { self.pos += 1; tokens.push(Token::new(TokenKind::And, start_line, start_col)); } else { return self.error("expected '&' to complete '&&'"); } }
                b'|' => { self.pos += 1; if self.peek() == Some(b'|') { self.pos += 1; tokens.push(Token::new(TokenKind::Or, start_line, start_col)); } else { return self.error("expected '|' to complete '||'"); } }
                b'(' => { self.pos += 1; tokens.push(Token::new(TokenKind::LParen, start_line, start_col)); }
                b')' => { self.pos += 1; tokens.push(Token::new(TokenKind::RParen, start_line, start_col)); }
                b'{' => { self.pos += 1; tokens.push(Token::new(TokenKind::LBrace, start_line, start_col)); }
                b'}' => { self.pos += 1; tokens.push(Token::new(TokenKind::RBrace, start_line, start_col)); }
                b'[' => { self.pos += 1; tokens.push(Token::new(TokenKind::LBracket, start_line, start_col)); }
                b']' => { self.pos += 1; tokens.push(Token::new(TokenKind::RBracket, start_line, start_col)); }
                b',' => { self.pos += 1; tokens.push(Token::new(TokenKind::Comma, start_line, start_col)); }
                b':' => { self.pos += 1; tokens.push(Token::new(TokenKind::Colon, start_line, start_col)); }
                b'.' => { self.pos += 1; tokens.push(Token::new(TokenKind::Dot, start_line, start_col)); }
                b';' => { self.pos += 1; tokens.push(Token::new(TokenKind::Semicolon, start_line, start_col)); }
                other => {
                    // Non-ASCII: decode one UTF-8 character for a friendly message.
                    let rest = &self.src[self.pos..];
                    let len = utf8_char_len(other).unwrap_or(1);
                    let text = String::from_utf8_lossy(&rest[..len.min(rest.len())]);
                    return self.error(format!("unexpected character '{text}'"));
                }
            }
        }
        tokens.push(Token::new(TokenKind::Eof, self.line, self.column()));
        Ok(tokens)
    }

    fn skip_line_comment(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos] != b'\n' { self.pos += 1; }
    }

    fn block_comment(&mut self, start_line: usize) -> Result<(), String> {
        self.pos += 2; // consume /*
        let mut depth: u32 = 1;
        while depth > 0 {
            if self.pos >= self.src.len() {
                return Err(format!("SPP:{start_line}:?: unterminated block comment"));
            }
            match (self.src[self.pos], self.peek_at(1)) {
                (b'/', Some(b'*')) => {
                    self.pos += 2;
                    depth += 1;
                    if depth > MAX_COMMENT_DEPTH {
                        return Err(format!("SPP:{}:{}: block comment nested too deeply", self.line, self.column()));
                    }
                }
                (b'*', Some(b'/')) => { self.pos += 2; depth -= 1; }
                (b'\n', _) => { self.bump(); }
                (_, _) => { self.pos += 1; }
            }
        }
        Ok(())
    }

    fn number(&mut self, line: usize, column: usize) -> Result<Token, String> {
        // Hex literal: 0x / 0X followed by hex digits.
        if self.src[self.pos] == b'0' && matches!(self.peek_at(1), Some(b'x') | Some(b'X')) {
            let start = self.pos;
            self.pos += 2;
            let digits_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'_')) { self.pos += 1; }
            if self.pos == digits_start { return self.error("expected hexadecimal digits after '0x'"); }
            let text: String = self.src[start + 2..self.pos].iter().map(|&b| b as char).filter(|&c| c != '_').collect();
            let value = u64::from_str_radix(&text, 16)
                .map_err(|_| format!("SPP:{line}:{column}: invalid hexadecimal literal '0x{text}'"))?;
            return Ok(Token::new(TokenKind::Int(value as i64), line, column));
        }

        let digits_start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'_')) { self.pos += 1; }
        if self.pos == digits_start { return self.error("expected decimal digits"); }

        let mut is_float = false;
        // '.' only continues a number when followed by a digit — otherwise it is
        // the member-access operator (e.g. `values.length`).
        if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b'0'..=b'9')) {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'_')) { self.pos += 1; }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let sign_after = if matches!(self.peek_at(1), Some(b'+') | Some(b'-')) { self.peek_at(2) } else { self.peek_at(1) };
            if matches!(sign_after, Some(b'0'..=b'9')) {
                is_float = true;
                self.pos += 1;
                if matches!(self.peek(), Some(b'+') | Some(b'-')) { self.pos += 1; }
                while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'_')) { self.pos += 1; }
            }
        }

        let raw = &self.src[digits_start..self.pos];
        if raw.last().is_some_and(|&b| b == b'.') { return self.error("expected digits after '.'"); }
        let text: String = raw.iter().map(|&b| b as char).filter(|&c| c != '_').collect();
        if is_float {
            let value: f64 = text.parse()
                .map_err(|_| format!("SPP:{line}:{column}: invalid float literal '{text}'"))?;
            Ok(Token::new(TokenKind::Float(value), line, column))
        } else {
            let value: i64 = text.parse().map_err(|_| {
                if text.chars().all(|c| c.is_ascii_digit()) {
                    format!("SPP:{line}:{column}: integer literal '{text}' out of range for int (64-bit)")
                } else {
                    format!("SPP:{line}:{column}: invalid integer literal '{text}'")
                }
            })?;
            Ok(Token::new(TokenKind::Int(value), line, column))
        }
    }

    fn string(&mut self, quote: u8, line: usize, column: usize) -> Result<Token, String> {
        self.pos += 1; // opening quote
        let content_start = self.pos;
        let mut has_escape = false;

        // Fast scan: find the closing quote without allocating anything.
        loop {
            match self.peek() {
                None => return self.error("unterminated string literal"),
                Some(b'\\') => { has_escape = true; self.pos += 2; }
                Some(b'\n') => return self.error("newline inside string literal"),
                Some(c) if c == quote => break,
                Some(_) => self.pos += 1,
            }
        }
        let content_end = self.pos;
        self.pos += 1; // closing quote

        let raw = &self.src[content_start..content_end];
        if !has_escape {
            // Zero-copy validation + single allocation for the common case.
            let text = std::str::from_utf8(raw)
                .map_err(|_| format!("SPP:{line}:{column}: string literal contains invalid UTF-8"))?
                .to_string();
            return Ok(Token::new(TokenKind::String(text), line, column));
        }

        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while i < raw.len() {
            let ch = raw[i];
            if ch != b'\\' {
                // Bulk-copy every plain byte until the next escape or end.
                let run_start = i;
                while i < raw.len() && raw[i] != b'\\' { i += 1; }
                let run = &raw[run_start..i];
                let text = std::str::from_utf8(run)
                    .map_err(|_| format!("SPP:{line}:{column}: string literal contains invalid UTF-8"))?;
                out.push_str(text);
                continue;
            }
            i += 1;
            let Some(&esc) = raw.get(i) else { return self.error("unterminated string escape") };
            i += 1;
            match esc {
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'0' => out.push('\0'),
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'u' => {
                    // \u{XXXX} form.
                    if raw.get(i) != Some(&b'{') { return self.error("expected '{' after \\u escape"); }
                    i += 1;
                    let mut code = 0u32;
                    let mut digits = 0usize;
                    loop {
                        let Some(&d) = raw.get(i) else { return self.error("malformed \\u{...} escape") };
                        i += 1;
                        match d {
                            b'0'..=b'9' => code = code * 16 + (d - b'0') as u32,
                            b'a'..=b'f' => code = code * 16 + (d - b'a') as u32 + 10,
                            b'A'..=b'F' => code = code * 16 + (d - b'A') as u32 + 10,
                            b'}' => break,
                            _ => return self.error("malformed \\u{...} escape"),
                        }
                        digits += 1;
                        if digits > 6 { return self.error("\\u escape exceeds 6 hex digits"); }
                    }
                    let c = char::from_u32(code).ok_or_else(|| format!("SPP:{line}:{column}: invalid unicode escape"))?;
                    out.push(c);
                }
                other => out.push(other as char), // unknown escape: keep the character verbatim
            }
        }
        Ok(Token::new(TokenKind::String(out), line, column))
    }

    fn identifier(&mut self, line: usize, column: usize) -> Token {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_')) { self.pos += 1; }
        let name = &self.src[start..self.pos];
        // SAFETY-equivalent: identifier bytes are guaranteed ASCII here.
        let text = unsafe { std::str::from_utf8_unchecked(name) };
        let kind = match Keyword::from_ident(text) {
            Some(keyword) => TokenKind::Keyword(keyword),
            None => TokenKind::Identifier(text.to_string()),
        };
        Token::new(kind, line, column)
    }
}

fn utf8_char_len(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7F => Some(1),
        0xC0..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF7 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        Lexer::new(src).tokenize().unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn keywords_are_distinguished_from_identifiers() {
        let toks = kinds("if iff else true");
        assert_eq!(toks[0], TokenKind::Keyword(Keyword::If));
        assert_eq!(toks[1], TokenKind::Identifier("iff".into()));
        assert_eq!(toks[2], TokenKind::Keyword(Keyword::Else));
        assert_eq!(toks[3], TokenKind::Keyword(Keyword::True));
    }

    #[test]
    fn dot_is_member_access_not_float_when_unfollowed_by_digit() {
        let toks = kinds("values.length");
        assert_eq!(toks[0], TokenKind::Identifier("values".into()));
        assert_eq!(toks[1], TokenKind::Dot);
        assert_eq!(toks[2], TokenKind::Identifier("length".into()));
    }

    #[test]
    fn floats_and_scientific_notation() {
        assert_eq!(kinds("3.14")[0], TokenKind::Float(3.14));
        assert_eq!(kinds("1e3")[0], TokenKind::Float(1000.0));
        assert_eq!(kinds("2.5e-2")[0], TokenKind::Float(0.025));
        assert_eq!(kinds("1_000_000")[0], TokenKind::Int(1_000_000));
    }

    #[test]
    fn hex_literals() {
        assert_eq!(kinds("0xFF")[0], TokenKind::Int(255));
        assert_eq!(kinds("0x10")[0], TokenKind::Int(16));
    }

    #[test]
    fn nested_block_comments() {
        let toks = kinds("/* a /* b */ c */ 1");
        assert_eq!(toks, vec![TokenKind::Int(1), TokenKind::Eof]);
    }

    #[test]
    fn unterminated_comment_is_error() {
        assert!(Lexer::new("/* nope").tokenize().is_err());
    }

    #[test]
    fn string_escapes_and_unicode() {
        let toks = kinds(r#" "tab\there" "#);
        assert_eq!(toks[0], TokenKind::String("tab\there".into()));
        let toks = kinds(r#" "\u{1F600}" "#);
        assert_eq!(toks[0], TokenKind::String("😀".into()));
    }

    #[test]
    fn newline_tracking() {
        let tokens = Lexer::new("a\nbb\nc").tokenize().unwrap();
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[2].line, 3);
        assert_eq!(tokens[2].column, 1);
    }
}
