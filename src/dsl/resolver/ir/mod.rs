// TODO: box LoweringBail to reduce Result size
#![allow(clippy::result_large_err)]

// ─── IrNode trait and macros ─────────────────────────────────
// Defined here so textual macro scoping makes impl_ir_node_struct!
// and impl_ir_node_enum! available in all sub-modules declared below.

use crate::diagnostics::IrSpan;

pub trait IrNode {
    fn span(&self) -> &IrSpan;
}

macro_rules! impl_ir_node_struct {
    ($($ty:ty),* $(,)?) => {
        $(
            impl IrNode for $ty {
                fn span(&self) -> &IrSpan {
                    &self.span
                }
            }
        )*
    };
}

macro_rules! impl_ir_node_enum {
    ($ty:ty { $($variant:ident),* $(,)? }) => {
        impl IrNode for $ty {
            fn span(&self) -> &IrSpan {
                match self {
                    $(Self::$variant { span, .. } => span,)*
                }
            }
        }
    };
}

// ─── Sub-modules ─────────────────────────────────────────────

pub mod legacy;
pub use legacy::*;

mod block;
mod comment;
mod effect;
mod expr;
mod func;
mod ident;
mod interpolation;
mod keys;
mod lowering_trait;
pub(crate) mod marker;
mod plan;
pub(crate) mod regex_validate;
mod stmt;
mod tables;
mod test_def;
mod timeout;

pub use block::*;
pub use comment::*;
pub use effect::*;
pub use expr::*;
pub use func::*;
pub use ident::*;
pub use interpolation::*;
pub use keys::*;
pub use lowering_trait::*;
pub use plan::*;
pub use stmt::*;
pub use tables::*;
pub use test_def::*;
pub use timeout::*;
