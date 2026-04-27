extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ast::{
    AssignKind, BinaryOp, Expr, ExprKind, Program, Stmt, StmtKind, Symbol, Type, UnaryOp,
};
use crate::lexer::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SymbolKey {
    name: String,
    declared_at: Span,
}

impl SymbolKey {
    fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            name: symbol.name.clone(),
            declared_at: symbol.declared_at,
        }
    }
}

struct RustEmitter {
    out: String,
    indent: usize,
    scopes: Vec<Vec<SymbolKey>>,
}

/// Emit a first-pass Rust C ABI blueprint entrypoint from a parsed C4 program.
///
/// The emitted code is deliberately conservative: it keeps C4 control-flow
/// structure recognizable, initializes variables before first emitted use, and
/// uses no_std-friendly Rust scalar/array types so the result can be compiled
/// or inspected as an early TRUEOS blueprint source entrypoint.
pub fn emit_rust(program: &Program) -> String {
    let mut emitter = RustEmitter {
        out: String::new(),
        indent: 0,
        scopes: Vec::new(),
    };
    emitter.emit_program(program);
    emitter.out
}

impl RustEmitter {
    fn emit_program(&mut self, program: &Program) {
        self.line("#![no_std]");
        self.line("#![allow(unused_mut, unused_variables, unused_assignments, unused_parens)]");
        self.line("");
        self.line("#[unsafe(no_mangle)]");
        self.line("pub extern \"C\" fn main() {");
        self.indent += 1;
        self.push_scope();
        self.emit_stmt_contents(&program.block);
        self.pop_scope();
        self.indent -= 1;
        self.line("}");
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Empty => self.line(";"),
            StmtKind::Block(_) => {
                self.line("{");
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(stmt);
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::If {
                condition,
                then_branch,
            } => {
                self.declare_expr_symbols(condition);
                self.line(&format!("if {} {{", expr(condition)));
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(then_branch);
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::IfElse {
                condition,
                then_branch,
                else_branch,
            } => {
                self.declare_expr_symbols(condition);
                self.line(&format!("if {} {{", expr(condition)));
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(then_branch);
                self.pop_scope();
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(else_branch);
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::While { condition, body } => {
                self.declare_expr_symbols(condition);
                self.line(&format!("while {} {{", expr(condition)));
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(body);
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::DoWhile { body, condition } => {
                self.declare_expr_symbols(condition);
                self.line("loop {");
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(body);
                self.line(&format!("if !({}) {{", expr(condition)));
                self.indent += 1;
                self.line("break;");
                self.indent -= 1;
                self.line("}");
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.declare_assign_symbols(init);
                self.line(&format!("{};", assign(init)));
                self.declare_expr_symbols(condition);
                self.declare_assign_symbols(step);
                self.line(&format!("while {} {{", expr(condition)));
                self.indent += 1;
                self.push_scope();
                self.emit_stmt_contents(body);
                self.line(&format!("{};", assign(step)));
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Break => self.line("break;"),
            StmtKind::Assign(assign_kind) => {
                self.declare_assign_symbols(assign_kind);
                self.line(&format!("{};", assign(assign_kind)));
            }
        }
    }

    fn emit_stmt_contents(&mut self, stmt: &Stmt) {
        if let StmtKind::Block(stmts) = &stmt.kind {
            for stmt in stmts {
                self.emit_stmt(stmt);
            }
        } else {
            self.emit_stmt(stmt);
        }
    }

    fn declare_assign_symbols(&mut self, assign_kind: &AssignKind) {
        match assign_kind {
            AssignKind::Var { target, value } => {
                self.declare_symbol(target);
                self.declare_expr_symbols(value);
            }
            AssignKind::Index { target, value } => {
                self.declare_expr_symbols(target);
                self.declare_expr_symbols(value);
            }
        }
    }

    fn declare_expr_symbols(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Id(symbol) => self.declare_symbol(symbol),
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => {}
            ExprKind::Unary { expr, .. } => self.declare_expr_symbols(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.declare_expr_symbols(lhs);
                self.declare_expr_symbols(rhs);
            }
            ExprKind::Index { base, index } => {
                self.declare_expr_symbols(base);
                self.declare_expr_symbols(index);
            }
        }
    }

    fn declare_symbol(&mut self, symbol: &Symbol) {
        let key = SymbolKey::from_symbol(symbol);
        if self
            .scopes
            .iter()
            .any(|scope| scope.iter().any(|declared| declared == &key))
        {
            return;
        }
        self.line(&format!(
            "let mut {}: {} = {};",
            ident(&symbol.name),
            ty(&symbol.ty),
            default_value(&symbol.ty)
        ));
        self.scopes
            .last_mut()
            .expect("backend emits with an active scope")
            .push(key);
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        let _ = self.scopes.pop();
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }
}

fn assign(assign: &AssignKind) -> String {
    match assign {
        AssignKind::Var { target, value } => format!("{} = {}", ident(&target.name), expr(value)),
        AssignKind::Index { target, value } => format!("{} = {}", lvalue(target), expr(value)),
    }
}

fn lvalue(value: &Expr) -> String {
    match &value.kind {
        ExprKind::Id(symbol) => ident(&symbol.name),
        ExprKind::Index { base, index } => format!("{}[({}) as usize]", lvalue(base), expr(index)),
        _ => expr(value),
    }
}

fn expr(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Id(symbol) => ident(&symbol.name),
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Float(value) => value.clone(),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Unary { op, expr } => match op {
            UnaryOp::Neg => format!("(-{})", self::expr(expr)),
            UnaryOp::Not => format!("(!{})", self::expr(expr)),
        },
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", self::expr(lhs), binary_op(*op), self::expr(rhs))
        }
        ExprKind::Index { base, index } => {
            format!("{}[({}) as usize]", self::expr(base), self::expr(index))
        }
    }
}

fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Less => "<",
        BinaryOp::LessEq => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEq => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

fn ty(ty: &Type) -> String {
    match ty {
        Type::Int => "i32".to_string(),
        Type::Float => "f64".to_string(),
        Type::Char => "u8".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Array { len, of } => format!("[{}; {}]", self::ty(of), len),
    }
}

fn default_value(ty: &Type) -> String {
    match ty {
        Type::Int => "0".to_string(),
        Type::Float => "0.0".to_string(),
        Type::Char => "0".to_string(),
        Type::Bool => "false".to_string(),
        Type::Array { len, of } => format!("[{}; {}]", default_value(of), len),
    }
}

fn ident(name: &str) -> String {
    match name {
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
        | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
        | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
        | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await"
        | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override"
        | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "try" => {
            format!("r#{name}")
        }
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::emit_rust;
    use crate::Parser;

    #[test]
    fn emits_assignments_conditions_loops_and_arrays() {
        let src = r#"
        {
            int i, j;
            bool ok;
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
                if (i == 3) break;
            }
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let rust = emit_rust(&program);

        assert!(rust.contains("#![no_std]"));
        assert!(rust.contains("#[unsafe(no_mangle)]"));
        assert!(rust.contains("pub extern \"C\" fn main()"));
        assert!(rust.contains("let mut i: i32 = 0;"));
        assert!(rust.contains("let mut grid: [[i32; 2]; 4] = [[0; 2]; 4];"));
        assert!(rust.contains("if (ok && (i < j)) {"));
        assert!(rust.contains("grid[(1) as usize][(0) as usize] = (i + j);"));
        assert!(rust.contains("loop {"));
        assert!(rust.contains("while (i < 4) {"));
        assert!(rust.contains("break;"));
    }

    #[test]
    fn escapes_rust_keywords_used_as_identifiers() {
        let src = "{ int fn; fn = 7; }";
        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let rust = emit_rust(&program);

        assert!(rust.contains("let mut r#fn: i32 = 0;"));
        assert!(rust.contains("r#fn = 7;"));
    }

    #[test]
    fn maps_c4_primitives_to_stable_rust_types() {
        let src = r#"
        {
            int i;
            float f;
            char c;
            bool b;
            i = i + 1;
            f = 2.5;
            c = c;
            b = !false;
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let rust = emit_rust(&program);

        assert!(rust.contains("let mut i: i32 = 0;"));
        assert!(rust.contains("let mut f: f64 = 0.0;"));
        assert!(rust.contains("let mut c: u8 = 0;"));
        assert!(rust.contains("let mut b: bool = false;"));
        assert!(rust.contains("i = (i + 1);"));
        assert!(rust.contains("b = (!false);"));
    }
}
