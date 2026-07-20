use std::fmt;

use super::{Constant, DreiAdrCode, ExprData, Id, Temp, Token, Type};

/// State shared by elementary expressions.
#[derive(Clone, Debug, PartialEq)]
pub struct SingletonData {
    pub(crate) expr: ExprData,
}

impl SingletonData {
    pub fn new(token: Token, ty: Type) -> Self {
        Self {
            expr: ExprData::new(token, Some(ty)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Singleton {
    Constant(Constant),
    Id(Id),
    Temp(Temp),
}

impl Singleton {
    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        DreiAdrCode::Assign {
            target,
            value: self.clone(),
        }
    }
}

impl fmt::Display for Singleton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Constant(value) => fmt::Display::fmt(value, f),
            Self::Id(value) => fmt::Display::fmt(value, f),
            Self::Temp(value) => fmt::Display::fmt(value, f),
        }
    }
}
