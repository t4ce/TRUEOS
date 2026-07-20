use super::{ExprData, Token};

/// State inherited by expression operands and operators.
#[derive(Clone, Debug, PartialEq)]
pub struct OpData {
    pub(crate) expr: ExprData,
}

impl OpData {
    pub fn new(token: Token) -> Self {
        Self {
            expr: ExprData::new(token, None),
        }
    }
}
