use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T, S = Range<usize>> {
    pub node: T,
    pub span: S,
}

impl<T, S> Spanned<T, S> {
    pub fn new(node: T, span: S) -> Self {
        Self { node, span }
    }
}

pub mod dsl;
pub mod runtime;
