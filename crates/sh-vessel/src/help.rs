#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Help {
    pub text: &'static str,
}

impl Help {
    pub const fn new(text: &'static str) -> Self {
        Self { text }
    }
}
