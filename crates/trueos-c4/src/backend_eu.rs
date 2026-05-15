extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ast::{AssignKind, BinaryOp, Expr, ExprKind, Program, Stmt, StmtKind, Type, UnaryOp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eu32Object {
    pub name: String,
    pub words: Vec<u32>,
    pub expected_store_value: u32,
    pub store_send_dword: Option<usize>,
    pub visible_seed_dword: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eu32EmitError {
    pub message: String,
}

impl Eu32EmitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn emit_eu32_object(program: &Program) -> Result<Eu32Object, Eu32EmitError> {
    let stmt = single_effect_stmt(&program.block)?;
    let StmtKind::Assign(AssignKind::Var { target, value }) = &stmt.kind else {
        return Err(Eu32EmitError::new("expected one assignment to `out`"));
    };
    if target.name.as_str() != "out" || target.ty != Type::Int {
        return Err(Eu32EmitError::new("expected target `int out`"));
    }

    let value = eval_u32(value)?;
    let words = trueos_eu::gfx12::c4_store_imm32_stateless_words(value);
    Ok(Eu32Object {
        name: "c4-gfx12-store-imm32-stateless".to_string(),
        words: words.to_vec(),
        expected_store_value: value,
        store_send_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD),
    })
}

fn single_effect_stmt(stmt: &Stmt) -> Result<&Stmt, Eu32EmitError> {
    match &stmt.kind {
        StmtKind::Block(stmts) => {
            let mut found = None;
            for stmt in stmts {
                if matches!(stmt.kind, StmtKind::Empty) {
                    continue;
                }
                if found.is_some() {
                    return Err(Eu32EmitError::new(
                        "EU subset accepts exactly one non-empty statement",
                    ));
                }
                found = Some(stmt);
            }
            found.ok_or_else(|| Eu32EmitError::new("EU subset requires one statement"))
        }
        _ => Ok(stmt),
    }
}

fn eval_u32(expr: &Expr) -> Result<u32, Eu32EmitError> {
    let value = eval_i64(expr)?;
    u32::try_from(value).map_err(|_| Eu32EmitError::new("constant does not fit u32"))
}

fn eval_i64(expr: &Expr) -> Result<i64, Eu32EmitError> {
    match &expr.kind {
        ExprKind::Int(value) => Ok(*value),
        ExprKind::Unary { op, expr } => match op {
            UnaryOp::Neg => eval_i64(expr)?
                .checked_neg()
                .ok_or_else(|| Eu32EmitError::new("constant negation overflow")),
            UnaryOp::Not => Err(Eu32EmitError::new("boolean constants are not in the EU subset")),
        },
        ExprKind::Binary { op, lhs, rhs } => {
            let lhs = eval_i64(lhs)?;
            let rhs = eval_i64(rhs)?;
            match op {
                BinaryOp::Add => lhs.checked_add(rhs),
                BinaryOp::Sub => lhs.checked_sub(rhs),
                BinaryOp::Mul => lhs.checked_mul(rhs),
                BinaryOp::Div => {
                    if rhs == 0 {
                        return Err(Eu32EmitError::new("division by zero in constant"));
                    }
                    lhs.checked_div(rhs)
                }
                _ => {
                    return Err(Eu32EmitError::new(
                        "only integer arithmetic constants are in the EU subset",
                    ));
                }
            }
            .ok_or_else(|| Eu32EmitError::new("constant arithmetic overflow"))
        }
        _ => Err(Eu32EmitError::new(
            "EU subset requires an integer constant expression",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::emit_eu32_object;
    use crate::Parser;

    #[test]
    fn emits_store_imm32_template() {
        let program = Parser::new("{ int out; out = 1234 + 6; }")
            .unwrap()
            .parse_program()
            .unwrap();
        let object = emit_eu32_object(&program).unwrap();
        assert_eq!(object.expected_store_value, 1240);
        assert_eq!(
            object.words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD],
            1240
        );
    }
}
