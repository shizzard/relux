use std::time::Duration;

use relux_ast::AstTimeout;
use relux_core::diagnostics::IrSpan;
use relux_core::diagnostics::LoweringBail;
use relux_core::table::FileId;

use super::IrNode;
use super::IrNodeLowering;
use super::LoweringContext;

#[derive(Debug, Clone)]
pub enum IrTimeout {
    Tolerance {
        duration: Duration,
        multiplier: f64,
        span: IrSpan,
    },
    Assertion {
        duration: Duration,
        span: IrSpan,
    },
}

impl IrTimeout {
    /// Convenience constructor for a tolerance timeout with default multiplier (1.0).
    pub fn tolerance(duration: Duration) -> Self {
        Self::tolerance_scaled(duration, 1.0)
    }

    /// Convenience constructor for a tolerance timeout with a given multiplier.
    pub fn tolerance_scaled(duration: Duration, multiplier: f64) -> Self {
        Self::Tolerance {
            duration,
            multiplier,
            span: IrSpan::synthetic(),
        }
    }

    /// Apply a multiplier to this timeout. Only affects Tolerance; Assertion is unchanged.
    pub fn apply_multiplier(&mut self, m: f64) {
        if let Self::Tolerance { multiplier, .. } = self {
            *multiplier = m;
        }
    }

    /// The raw duration before any multiplier adjustment.
    pub fn raw_duration(&self) -> Duration {
        match self {
            Self::Tolerance { duration, .. } | Self::Assertion { duration, .. } => *duration,
        }
    }

    /// The effective duration after multiplier adjustment.
    pub fn adjusted_duration(&self) -> Duration {
        match self {
            Self::Tolerance {
                duration,
                multiplier,
                ..
            } => duration.mul_f64(*multiplier),
            Self::Assertion { duration, .. } => *duration,
        }
    }

    /// The effective duration after both static and flaky multiplier adjustment.
    pub fn adjusted_duration_with_flaky(&self, flaky_multiplier: f64) -> Duration {
        match self {
            Self::Tolerance {
                duration,
                multiplier,
                ..
            } => duration.mul_f64(*multiplier * flaky_multiplier),
            Self::Assertion { duration, .. } => *duration,
        }
    }

    /// Whether this is an assertion timeout.
    pub fn is_assertion(&self) -> bool {
        matches!(self, Self::Assertion { .. })
    }
}

impl_ir_node_enum!(IrTimeout {
    Tolerance,
    Assertion
});

impl IrNodeLowering for IrTimeout {
    type Ast = AstTimeout;
    fn lower(
        ast: &AstTimeout,
        file: &FileId,
        ctx: &mut LoweringContext,
    ) -> Result<Self, LoweringBail> {
        Ok(match ast {
            AstTimeout::Tolerance { duration, span } => IrTimeout::Tolerance {
                duration: *duration,
                multiplier: ctx.multiplier(),
                span: IrSpan::new(file.clone(), *span),
            },
            AstTimeout::Assertion { duration, span } => IrTimeout::Assertion {
                duration: *duration,
                span: IrSpan::new(file.clone(), *span),
            },
        })
    }
}
