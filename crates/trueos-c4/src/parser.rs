extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ast::{
    AssignKind, BinaryOp, Expr, ExprKind, Program, Stmt, StmtKind, Symbol, Type, UnaryOp,
};
use crate::lexer::{LexError, Lexer, Span, Token, TokenKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl From<LexError> for ParseError {
    fn from(value: LexError) -> Self {
        Self {
            message: value.message,
            span: value.span,
        }
    }
}

#[derive(Clone, Debug)]
struct Scope {
    symbols: BTreeMap<String, Symbol>,
}

impl Scope {
    fn new() -> Self {
        Self {
            symbols: BTreeMap::new(),
        }
    }
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    look: Token,
    scopes: Vec<Scope>,
    used: usize,
    loop_depth: usize,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(source);
        let look = lexer.next_token()?;
        Ok(Self {
            lexer,
            look,
            scopes: Vec::new(),
            used: 0,
            loop_depth: 0,
        })
    }

    fn bump(&mut self) -> Result<Token, ParseError> {
        let prev = self.look.clone();
        self.look = self.lexer.next_token()?;
        Ok(prev)
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            span: self.look.span,
        })
    }

    fn expect_punct(&mut self, expected: TokenKind) -> Result<Token, ParseError> {
        if self.look.kind == expected {
            self.bump()
        } else {
            self.err(expected_name(&expected))
        }
    }

    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        self.parse_rust_blueprint_preamble()?;
        let block = self.parse_block_stmt()?;
        if self.look.kind != TokenKind::Eof {
            return self.err("expected eof");
        }
        Ok(Program { block })
    }

    fn parse_rust_blueprint_preamble(&mut self) -> Result<(), ParseError> {
        let mut saw_no_std = false;
        let mut saw_no_main = false;
        while matches!(self.look.kind, TokenKind::NoStd | TokenKind::NoMain) {
            match self.look.kind {
                TokenKind::NoStd if saw_no_std => return self.err("duplicate #![no_std]"),
                TokenKind::NoStd => saw_no_std = true,
                TokenKind::NoMain if saw_no_main => return self.err("duplicate #![no_main]"),
                TokenKind::NoMain => saw_no_main = true,
                _ => {}
            }
            let _ = self.bump()?;
        }
        Ok(())
    }

    fn parse_block_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.look.span;
        let saved_used = self.used;
        self.expect_punct(TokenKind::LBrace)?;
        self.scopes.push(Scope::new());
        self.parse_decls()?;
        let mut stmts = Vec::new();
        while self.look.kind != TokenKind::RBrace {
            stmts.push(self.parse_stmt()?);
        }
        self.expect_punct(TokenKind::RBrace)?;
        self.scopes.pop();
        self.used = saved_used;
        Ok(Stmt {
            kind: StmtKind::Block(stmts),
            span,
        })
    }

    fn parse_decls(&mut self) -> Result<(), ParseError> {
        while let TokenKind::Basic(base_ty) = self.look.kind.clone() {
            let ty = self.parse_type_from(base_ty)?;
            loop {
                let token = self.bump()?;
                let TokenKind::Id(name) = token.kind else {
                    return Err(ParseError {
                        message: "expected identifier".to_string(),
                        span: token.span,
                    });
                };
                let sym = Symbol {
                    name: name.clone(),
                    ty: ty.clone(),
                    offset: self.used,
                    declared_at: token.span,
                };
                self.used = self.used.saturating_add(ty.width());
                let scope = self.scopes.last_mut().expect("block scope");
                if scope.symbols.insert(name.clone(), sym).is_some() {
                    return Err(ParseError {
                        message: alloc::format!("variable {name} redeclared"),
                        span: token.span,
                    });
                }
                if self.look.kind != TokenKind::Comma {
                    break;
                }
                let _ = self.bump()?;
            }
            self.expect_punct(TokenKind::Semi)?;
        }
        Ok(())
    }

    fn parse_type_from(&mut self, base_ty: Type) -> Result<Type, ParseError> {
        let _ = self.bump()?;
        if self.look.kind != TokenKind::LBracket {
            return Ok(base_ty);
        }
        self.parse_dims(base_ty)
    }

    fn parse_dims(&mut self, base: Type) -> Result<Type, ParseError> {
        self.expect_punct(TokenKind::LBracket)?;
        let size_tok = self.bump()?;
        let TokenKind::Num(len) = size_tok.kind else {
            return Err(ParseError {
                message: "expected array size".to_string(),
                span: size_tok.span,
            });
        };
        self.expect_punct(TokenKind::RBracket)?;
        let inner = if self.look.kind == TokenKind::LBracket {
            self.parse_dims(base)?
        } else {
            base
        };
        Ok(Type::Array {
            len: len as usize,
            of: Box::new(inner),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.look.span;
        match self.look.kind.clone() {
            TokenKind::Semi => {
                let _ = self.bump()?;
                Ok(Stmt {
                    kind: StmtKind::Empty,
                    span,
                })
            }
            TokenKind::If => {
                let _ = self.bump()?;
                self.expect_punct(TokenKind::LParen)?;
                let condition = self.parse_bool()?;
                self.expect_punct(TokenKind::RParen)?;
                let then_branch = Box::new(self.parse_stmt()?);
                if self.look.kind == TokenKind::Else {
                    let _ = self.bump()?;
                    let else_branch = Box::new(self.parse_stmt()?);
                    Ok(Stmt {
                        kind: StmtKind::IfElse {
                            condition,
                            then_branch,
                            else_branch,
                        },
                        span,
                    })
                } else {
                    Ok(Stmt {
                        kind: StmtKind::If {
                            condition,
                            then_branch,
                        },
                        span,
                    })
                }
            }
            TokenKind::While => {
                let _ = self.bump()?;
                self.expect_punct(TokenKind::LParen)?;
                let condition = self.parse_bool()?;
                self.expect_punct(TokenKind::RParen)?;
                self.loop_depth += 1;
                let body = self.parse_stmt();
                self.loop_depth -= 1;
                Ok(Stmt {
                    kind: StmtKind::While {
                        condition,
                        body: Box::new(body?),
                    },
                    span,
                })
            }
            TokenKind::Do => {
                let _ = self.bump()?;
                self.loop_depth += 1;
                let body = self.parse_stmt();
                self.loop_depth -= 1;
                self.expect_keyword(TokenKind::While)?;
                self.expect_punct(TokenKind::LParen)?;
                let condition = self.parse_bool()?;
                self.expect_punct(TokenKind::RParen)?;
                self.expect_punct(TokenKind::Semi)?;
                Ok(Stmt {
                    kind: StmtKind::DoWhile {
                        body: Box::new(body?),
                        condition,
                    },
                    span,
                })
            }
            TokenKind::Break => {
                let tok = self.bump()?;
                self.expect_punct(TokenKind::Semi)?;
                if self.loop_depth == 0 {
                    return Err(ParseError {
                        message: "break outside loop".to_string(),
                        span: tok.span,
                    });
                }
                Ok(Stmt {
                    kind: StmtKind::Break,
                    span,
                })
            }
            TokenKind::LBrace => self.parse_block_stmt(),
            TokenKind::Id(_) => {
                let assign = self.parse_assign()?;
                self.expect_punct(TokenKind::Semi)?;
                Ok(Stmt {
                    kind: StmtKind::Assign(assign),
                    span,
                })
            }
            TokenKind::For => {
                let _ = self.bump()?;
                self.expect_punct(TokenKind::LParen)?;
                let init = self.parse_assign()?;
                self.expect_punct(TokenKind::Semi)?;
                let condition = self.parse_bool()?;
                self.expect_punct(TokenKind::Semi)?;
                let step = self.parse_assign()?;
                self.expect_punct(TokenKind::RParen)?;
                self.loop_depth += 1;
                let body = self.parse_stmt();
                self.loop_depth -= 1;
                Ok(Stmt {
                    kind: StmtKind::For {
                        init,
                        condition,
                        step,
                        body: Box::new(body?),
                    },
                    span,
                })
            }
            _ => self.err("expected statement"),
        }
    }

    fn expect_keyword(&mut self, expected: TokenKind) -> Result<Token, ParseError> {
        self.expect_punct(expected)
    }

    fn parse_assign(&mut self) -> Result<AssignKind, ParseError> {
        let ident_tok = self.bump()?;
        let TokenKind::Id(name) = ident_tok.kind else {
            return Err(ParseError {
                message: "expected identifier".to_string(),
                span: ident_tok.span,
            });
        };
        let sym = self.lookup(&name, ident_tok.span)?;
        if self.look.kind == TokenKind::Assign {
            let _ = self.bump()?;
            let value = self.parse_bool()?;
            if sym.ty != value.ty {
                return Err(ParseError {
                    message: alloc::format!("type mismatch in assignment to {}", sym.name.as_str()),
                    span: ident_tok.span,
                });
            }
            return Ok(AssignKind::Var { target: sym, value });
        }
        let target = self.parse_offset_from_symbol(sym, ident_tok.span)?;
        self.expect_punct(TokenKind::Assign)?;
        let value = self.parse_bool()?;
        if target.ty != value.ty {
            return Err(ParseError {
                message: "type mismatch in indexed assignment".to_string(),
                span: ident_tok.span,
            });
        }
        Ok(AssignKind::Index { target, value })
    }

    fn parse_bool(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_join()?;
        while self.look.kind == TokenKind::OrOr {
            let span = self.bump()?.span;
            let rhs = self.parse_join()?;
            ensure_bool(&expr, span, "|| lhs")?;
            ensure_bool(&rhs, span, "|| rhs")?;
            expr = Expr {
                ty: Type::Bool,
                span,
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    lhs: Box::new(expr),
                    rhs: Box::new(rhs),
                },
            };
        }
        Ok(expr)
    }

    fn parse_join(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_equality()?;
        while self.look.kind == TokenKind::AndAnd {
            let span = self.bump()?.span;
            let rhs = self.parse_equality()?;
            ensure_bool(&expr, span, "&& lhs")?;
            ensure_bool(&rhs, span, "&& rhs")?;
            expr = Expr {
                ty: Type::Bool,
                span,
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    lhs: Box::new(expr),
                    rhs: Box::new(rhs),
                },
            };
        }
        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_rel()?;
        loop {
            let op = match self.look.kind {
                TokenKind::EqEq => Some(BinaryOp::Eq),
                TokenKind::NotEq => Some(BinaryOp::NotEq),
                _ => None,
            };
            let Some(op) = op else { break };
            let span = self.bump()?.span;
            let rhs = self.parse_rel()?;
            if expr.ty != rhs.ty {
                return Err(ParseError {
                    message: "equality operands must have same type".to_string(),
                    span,
                });
            }
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(expr),
                    rhs: Box::new(rhs),
                },
                ty: Type::Bool,
                span,
            };
        }
        Ok(expr)
    }

    fn parse_rel(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_expr()?;
        let op = match self.look.kind {
            TokenKind::Less => Some(BinaryOp::Less),
            TokenKind::LessEq => Some(BinaryOp::LessEq),
            TokenKind::Greater => Some(BinaryOp::Greater),
            TokenKind::GreaterEq => Some(BinaryOp::GreaterEq),
            _ => None,
        };
        let Some(op) = op else { return Ok(lhs) };
        let span = self.bump()?.span;
        let rhs = self.parse_expr()?;
        if lhs.ty != rhs.ty {
            return Err(ParseError {
                message: "relational operands must have same type".to_string(),
                span,
            });
        }
        if !is_numeric(&lhs.ty) {
            return Err(ParseError {
                message: "relational operands must be numeric".to_string(),
                span,
            });
        }
        Ok(Expr {
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty: Type::Bool,
            span,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_term()?;
        loop {
            let op = match self.look.kind {
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            let Some(op) = op else { break };
            let span = self.bump()?.span;
            let rhs = self.parse_term()?;
            let ty = numeric_result_type(&expr.ty, &rhs.ty, span)?;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(expr),
                    rhs: Box::new(rhs),
                },
                ty,
                span,
            };
        }
        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = match self.look.kind {
                TokenKind::Mul => Some(BinaryOp::Mul),
                TokenKind::Div => Some(BinaryOp::Div),
                _ => None,
            };
            let Some(op) = op else { break };
            let span = self.bump()?.span;
            let rhs = self.parse_unary()?;
            let ty = numeric_result_type(&expr.ty, &rhs.ty, span)?;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(expr),
                    rhs: Box::new(rhs),
                },
                ty,
                span,
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.look.kind {
            TokenKind::Minus => {
                let span = self.bump()?.span;
                let expr = self.parse_unary()?;
                if !is_numeric(&expr.ty) {
                    return Err(ParseError {
                        message: "unary minus expects numeric operand".to_string(),
                        span,
                    });
                }
                let ty = expr.ty.clone();
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(expr),
                    },
                    ty,
                    span,
                })
            }
            TokenKind::Not => {
                let span = self.bump()?.span;
                let expr = self.parse_unary()?;
                ensure_bool(&expr, span, "! operand")?;
                Ok(Expr {
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(expr),
                    },
                    ty: Type::Bool,
                    span,
                })
            }
            _ => self.parse_factor(),
        }
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let token = self.look.clone();
        match token.kind {
            TokenKind::LParen => {
                let _ = self.bump()?;
                let expr = self.parse_bool()?;
                self.expect_punct(TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Num(value) => {
                let _ = self.bump()?;
                Ok(Expr {
                    kind: ExprKind::Int(value),
                    ty: Type::Int,
                    span: token.span,
                })
            }
            TokenKind::Real(ref value) => {
                let _ = self.bump()?;
                Ok(Expr {
                    kind: ExprKind::Float(value.clone()),
                    ty: Type::Float,
                    span: token.span,
                })
            }
            TokenKind::True => {
                let _ = self.bump()?;
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    ty: Type::Bool,
                    span: token.span,
                })
            }
            TokenKind::False => {
                let _ = self.bump()?;
                Ok(Expr {
                    kind: ExprKind::Bool(false),
                    ty: Type::Bool,
                    span: token.span,
                })
            }
            TokenKind::Id(ref name) => {
                let _ = self.bump()?;
                let sym = self.lookup(name, token.span)?;
                if self.look.kind == TokenKind::LBracket {
                    self.parse_offset_from_symbol(sym, token.span)
                } else {
                    let ty = sym.ty.clone();
                    Ok(Expr {
                        kind: ExprKind::Id(sym),
                        ty,
                        span: token.span,
                    })
                }
            }
            _ => self.err("expected factor"),
        }
    }

    fn parse_offset_from_symbol(&mut self, sym: Symbol, span: Span) -> Result<Expr, ParseError> {
        let base = Expr {
            kind: ExprKind::Id(sym.clone()),
            ty: sym.ty.clone(),
            span,
        };
        self.parse_offset(base)
    }

    fn parse_offset(&mut self, mut base: Expr) -> Result<Expr, ParseError> {
        while self.look.kind == TokenKind::LBracket {
            self.expect_punct(TokenKind::LBracket)?;
            let index = self.parse_bool()?;
            self.expect_punct(TokenKind::RBracket)?;
            if index.ty != Type::Int {
                return Err(ParseError {
                    message: "array index must be int".to_string(),
                    span: index.span,
                });
            }
            let next_ty = match &base.ty {
                Type::Array { of, .. } => (**of).clone(),
                _ => {
                    return Err(ParseError {
                        message: "indexing non-array expression".to_string(),
                        span: base.span,
                    });
                }
            };
            let base_span = base.span;
            base = Expr {
                kind: ExprKind::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                },
                ty: next_ty,
                span: base_span,
            };
        }
        Ok(base)
    }

    fn lookup(&self, name: &str, span: Span) -> Result<Symbol, ParseError> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.symbols.get(name) {
                return Ok(sym.clone());
            }
        }
        Err(ParseError {
            message: alloc::format!("{name} undeclared"),
            span,
        })
    }
}

fn expected_name(token: &TokenKind) -> &'static str {
    match token {
        TokenKind::LBrace => "expected '{'",
        TokenKind::RBrace => "expected '}'",
        TokenKind::LBracket => "expected '['",
        TokenKind::RBracket => "expected ']'",
        TokenKind::LParen => "expected '('",
        TokenKind::RParen => "expected ')'",
        TokenKind::Assign => "expected '='",
        TokenKind::Semi => "expected ';'",
        TokenKind::Comma => "expected ','",
        TokenKind::While => "expected 'while'",
        _ => "unexpected token",
    }
}

fn is_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float | Type::Char)
}

fn ensure_bool(expr: &Expr, span: Span, ctx: &str) -> Result<(), ParseError> {
    if expr.ty == Type::Bool {
        Ok(())
    } else {
        Err(ParseError {
            message: alloc::format!("{ctx} must be bool"),
            span,
        })
    }
}

fn numeric_result_type(lhs: &Type, rhs: &Type, span: Span) -> Result<Type, ParseError> {
    if !is_numeric(lhs) || !is_numeric(rhs) {
        return Err(ParseError {
            message: "arithmetic operands must be numeric".to_string(),
            span,
        });
    }
    if lhs == &Type::Float || rhs == &Type::Float {
        Ok(Type::Float)
    } else {
        Ok(Type::Int)
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::ast::{ExprKind, StmtKind, Type};

    #[test]
    fn parses_nested_blocks_and_loops() {
        let src = r#"
        {
            int i, j;
            bool ok;
            float x;
            int[4][2] grid;
            i = 1;
            j = 2;
            ok = true;
            if (ok && (i < j)) {
                grid[1][0] = i + j;
            } else {
                do j = j - 1; while (j > 0);
            }
            for (i = 0; i < 4; i = i + 1) {
                x = 1.5;
            }
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let StmtKind::Block(stmts) = program.block.kind else {
            panic!("expected block");
        };
        assert!(!stmts.is_empty());
    }

    #[test]
    fn preserves_array_index_type() {
        let src = r#"
        {
            int[3] a;
            int x;
            x = a[1];
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let StmtKind::Block(stmts) = program.block.kind else {
            panic!("expected block");
        };
        let assign = match &stmts[0].kind {
            StmtKind::Assign(assign) => assign,
            _ => match &stmts[1].kind {
                StmtKind::Assign(assign) => assign,
                _ => panic!("expected assignment"),
            },
        };
        let crate::ast::AssignKind::Var { value, .. } = assign else {
            panic!("expected var assign");
        };
        match &value.kind {
            ExprKind::Index { .. } => assert_eq!(value.ty, Type::Int),
            _ => panic!("expected index expr"),
        }
    }

    #[test]
    fn rejects_break_outside_loop() {
        let src = "{ break; }";
        assert!(Parser::new(src).unwrap().parse_program().is_err());
    }

    #[test]
    fn accepts_tiny_rust_blueprint_preamble_tokens() {
        let src = r#"
        #![no_std]
        #![no_main]
        {
            int x;
            x = 1;
        }
        "#;
        Parser::new(src).unwrap().parse_program().unwrap();
    }

    #[test]
    fn rejects_duplicate_rust_blueprint_preamble_tokens() {
        let src = r#"
        #![no_std]
        #![no_std]
        {
            int x;
            x = 1;
        }
        "#;
        assert!(Parser::new(src).unwrap().parse_program().is_err());
    }
}
