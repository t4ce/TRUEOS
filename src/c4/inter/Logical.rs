use super::{Expr, ExprData, Token};

/// State shared by `And`, `Or`, `Not`, and `Rel`.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalData {
    pub(crate) expr: ExprData,
    expr1: Expr,
    expr2: Expr,
}

impl LogicalData {
    pub fn new(token: Token, expr1: Expr, expr2: Expr) -> Self {
        Self {
            expr: ExprData::new(token, None),
            expr1,
            expr2,
        }
    }

    pub fn get_expr1(&self) -> &Expr {
        &self.expr1
    }
    pub fn get_expr2(&self) -> &Expr {
        &self.expr2
    }
    pub fn set_expr1(&mut self, expr: Expr) {
        self.expr1 = expr;
    }
    pub fn set_expr2(&mut self, expr: Expr) {
        self.expr2 = expr;
    }
}
