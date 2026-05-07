use crate::ast::Span;
use crate::diag::{fix, Diagnostic, Phase};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Tag,
    Keyword,
    Ident,
    Int,
    Float,
    String,
    Operator,
    Punct,
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    #[allow(dead_code)]
    pub lexeme: String,
    pub span: Span,
}

pub fn lex_source(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lexer = Lexer::new(source);
    lexer.lex();
    (lexer.tokens, lexer.diags)
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: usize,
    col: usize,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            diags: Vec::new(),
        }
    }

    fn lex(&mut self) {
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => self.newline(),
                '#' => self.comment(),
                '@' => self.tag(),
                '"' => self.string(),
                '{' | '}' => self.rejected_brace(),
                '-' if self.peek_next() == Some('-') && self.peek_nth(2) == Some('-') => {
                    self.fixed("---", TokenKind::Punct)
                }
                c if is_ident_start(c) => self.ident_or_keyword(),
                c if c.is_ascii_digit() || self.starts_negative_number() => self.number(),
                ':' if self.peek_next() == Some(':') => self.fixed("::", TokenKind::Operator),
                '-' if self.peek_next() == Some('>') => self.fixed("->", TokenKind::Operator),
                '=' if self.peek_next() == Some('=') => self.fixed("==", TokenKind::Operator),
                '!' if self.peek_next() == Some('=') => self.fixed("!=", TokenKind::Operator),
                '<' if self.peek_next() == Some('=') => self.fixed("<=", TokenKind::Operator),
                '>' if self.peek_next() == Some('=') => self.fixed(">=", TokenKind::Operator),
                '&' if self.peek_next() == Some('&') => self.fixed("&&", TokenKind::Operator),
                '|' if self.peek_next() == Some('|') => self.fixed("||", TokenKind::Operator),
                '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' => {
                    self.single(TokenKind::Operator)
                }
                '(' | ')' | ',' | ':' => self.single(TokenKind::Punct),
                _ => self.invalid_char(),
            }
        }
        let start = self.mark();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            span: self.span_from(start),
        });
    }

    fn tag(&mut self) {
        let start = self.mark();
        self.bump();
        let mut text = String::from("@");
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                text.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        let span = self.span_from(start);
        if is_known_tag(&text) {
            self.tokens.push(Token {
                kind: TokenKind::Tag,
                lexeme: text,
                span,
            });
        } else {
            self.diags.push(Diagnostic::error(
                "E_LEX004",
                Phase::Lex,
                None,
                span,
                "unknown @ tag",
                Some("@func | @loop | @condition | @intent | @in | @out | @end".to_string()),
                Some(text),
                fix("replace", "@condition"),
            ));
        }
    }

    fn ident_or_keyword(&mut self) {
        let start = self.mark();
        let mut text = String::new();
        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                text.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        let kind = if is_keyword(&text) {
            TokenKind::Keyword
        } else {
            TokenKind::Ident
        };
        let span = self.span_from(start);
        if matches!(kind, TokenKind::Ident) && text.len() > 256 {
            self.diags.push(Diagnostic::error(
                "E_LEX005",
                Phase::Lex,
                None,
                span,
                "identifier exceeds 256 bytes",
                Some("identifier length <= 256 bytes".to_string()),
                Some(text.len().to_string()),
                fix("rename", text.chars().take(256).collect::<String>()),
            ));
            return;
        }
        self.tokens.push(Token {
            kind,
            lexeme: text,
            span,
        });
    }

    fn number(&mut self) {
        let start = self.mark();
        let mut text = String::new();
        if self.peek() == Some('-') {
            text.push('-');
            self.bump();
        }
        let mut has_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                text.push(ch);
                self.bump();
            } else if ch == '.' && !has_dot {
                has_dot = true;
                text.push(ch);
                self.bump();
            } else {
                break;
            }
        }
        self.tokens.push(Token {
            kind: if has_dot {
                TokenKind::Float
            } else {
                TokenKind::Int
            },
            lexeme: text,
            span: self.span_from(start),
        });
    }

    fn string(&mut self) {
        let start = self.mark();
        let mut text = String::new();
        self.bump();
        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::String,
                        lexeme: text,
                        span: self.span_from(start),
                    });
                    return;
                }
                '\\' => {
                    self.bump();
                    match self.peek() {
                        Some('n') => {
                            text.push('\n');
                            self.bump();
                        }
                        Some('t') => {
                            text.push('\t');
                            self.bump();
                        }
                        Some('"') => {
                            text.push('"');
                            self.bump();
                        }
                        Some('\\') => {
                            text.push('\\');
                            self.bump();
                        }
                        Some('u') => {
                            self.bump();
                            if self.peek() == Some('{') {
                                self.bump();
                                let mut hex = String::new();
                                while let Some(ch) = self.peek() {
                                    if ch == '}' {
                                        self.bump();
                                        break;
                                    }
                                    hex.push(ch);
                                    self.bump();
                                }
                                if let Ok(value) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(value) {
                                        text.push(ch);
                                    }
                                }
                            }
                        }
                        Some(other) => {
                            text.push(other);
                            self.bump();
                        }
                        None => break,
                    }
                }
                '\n' => break,
                other => {
                    text.push(other);
                    self.bump();
                }
            }
        }

        self.diags.push(Diagnostic::error(
            "E_LEX002",
            Phase::Lex,
            None,
            self.span_from(start),
            "unterminated string literal",
            Some("closing quote".to_string()),
            Some("end of line".to_string()),
            fix("insert", "\""),
        ));
    }

    fn rejected_brace(&mut self) {
        let start = self.mark();
        let actual = self.peek().unwrap_or('{').to_string();
        self.bump();
        self.diags.push(Diagnostic::error(
            "E_LEX003",
            Phase::Lex,
            None,
            self.span_from(start),
            "bare braces are reserved; JapalityBean uses named @end closures",
            Some("@end <kind> <name>".to_string()),
            Some(actual),
            fix("replace", "@end func <name>"),
        ));
    }

    fn invalid_char(&mut self) {
        let start = self.mark();
        let actual = self.peek().unwrap_or('\0').to_string();
        self.bump();
        self.diags.push(Diagnostic::error(
            "E_LEX001",
            Phase::Lex,
            None,
            self.span_from(start),
            "invalid character",
            Some("JapalityBean token".to_string()),
            Some(actual),
            fix("delete", ""),
        ));
    }

    fn newline(&mut self) {
        let start = self.mark();
        self.bump();
        self.tokens.push(Token {
            kind: TokenKind::Newline,
            lexeme: "\\n".to_string(),
            span: self.span_from(start),
        });
    }

    fn comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.bump();
        }
    }

    fn fixed(&mut self, text: &str, kind: TokenKind) {
        let start = self.mark();
        for _ in 0..text.len() {
            self.bump();
        }
        self.tokens.push(Token {
            kind,
            lexeme: text.to_string(),
            span: self.span_from(start),
        });
    }

    fn single(&mut self, kind: TokenKind) {
        let start = self.mark();
        let lexeme = self.peek().unwrap_or('\0').to_string();
        self.bump();
        self.tokens.push(Token {
            kind,
            lexeme,
            span: self.span_from(start),
        });
    }

    fn starts_negative_number(&self) -> bool {
        self.peek() == Some('-')
            && self
                .peek_next()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false)
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next()?;
        chars.next()
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.source[self.pos..].chars().nth(n)
    }

    fn bump(&mut self) {
        let Some(ch) = self.peek() else {
            return;
        };
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
    }

    fn mark(&self) -> (usize, usize, usize) {
        (self.pos, self.line, self.col)
    }

    fn span_from(&self, start: (usize, usize, usize)) -> Span {
        Span {
            start_byte: start.0,
            end_byte: self.pos,
            start_line: start.1,
            start_col: start.2,
            end_line: self.line,
            end_col: self.col,
        }
    }
}

fn is_known_tag(text: &str) -> bool {
    matches!(
        text,
        "@func"
            | "@loop"
            | "@condition"
            | "@intent"
            | "@in"
            | "@out"
            | "@end"
            | "@continue"
            | "@break"
    )
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "let"
            | "set"
            | "if"
            | "else"
            | "return"
            | "in"
            | "true"
            | "false"
            | "as"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
    )
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}
