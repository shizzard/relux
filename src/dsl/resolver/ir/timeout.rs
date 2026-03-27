use std::time::Duration;

use crate::core::table::FileId;
use crate::diagnostics::IrSpan;
use crate::diagnostics::LoweringBail;
use crate::dsl::parser::ast::AstTimeout;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn tolerance_default_multiplier() {
        let t = IrTimeout::tolerance(Duration::from_secs(5));
        assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
    }

    #[test]
    fn tolerance_apply_multiplier() {
        let mut t = IrTimeout::tolerance(Duration::from_secs(5));
        t.apply_multiplier(2.0);
        assert_eq!(t.adjusted_duration(), Duration::from_secs(10));
    }

    #[test]
    fn tolerance_apply_multiplier_fractional() {
        let mut t = IrTimeout::tolerance(Duration::from_secs(5));
        t.apply_multiplier(0.5);
        assert_eq!(t.adjusted_duration(), Duration::from_millis(2500));
    }

    #[test]
    fn tolerance_apply_multiplier_zero() {
        let mut t = IrTimeout::tolerance(Duration::from_secs(5));
        t.apply_multiplier(0.0);
        assert_eq!(t.adjusted_duration(), Duration::ZERO);
    }

    #[test]
    fn assertion_ignores_multiplier() {
        let mut t = IrTimeout::Assertion {
            duration: Duration::from_secs(5),
            span: IrSpan::synthetic(),
        };
        t.apply_multiplier(2.0);
        assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
    }

    #[test]
    fn tolerance_raw_duration() {
        let mut t = IrTimeout::tolerance(Duration::from_secs(5));
        t.apply_multiplier(3.0);
        assert_eq!(t.raw_duration(), Duration::from_secs(5));
        assert_eq!(t.adjusted_duration(), Duration::from_secs(15));
    }

    #[test]
    fn tolerance_constructor_synthetic_span() {
        let t = IrTimeout::tolerance(Duration::from_secs(1));
        assert_eq!(t.span().file().path().to_str().unwrap(), "<synthetic>");
    }

    #[test]
    fn assertion_adjusted_is_raw() {
        let t = IrTimeout::Assertion {
            duration: Duration::from_secs(3),
            span: IrSpan::synthetic(),
        };
        assert_eq!(t.adjusted_duration(), Duration::from_secs(3));
        assert_eq!(t.raw_duration(), Duration::from_secs(3));
        assert_eq!(t.adjusted_duration(), t.raw_duration());
    }

    #[test]
    fn tolerance_scaled_applies_multiplier() {
        let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 2.0);
        assert_eq!(t.raw_duration(), Duration::from_secs(5));
        assert_eq!(t.adjusted_duration(), Duration::from_secs(10));
    }

    #[test]
    fn tolerance_scaled_with_unit_multiplier() {
        let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 1.0);
        assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
    }

    #[test]
    fn tolerance_scaled_fractional() {
        let t = IrTimeout::tolerance_scaled(Duration::from_secs(10), 0.5);
        assert_eq!(t.adjusted_duration(), Duration::from_secs(5));
    }

    #[test]
    fn tolerance_flaky_multiplier() {
        let t = IrTimeout::tolerance_scaled(Duration::from_secs(5), 2.0);
        // base=5s, static_m=2.0, flaky_m=1.5 → 5*2.0*1.5 = 15s
        assert_eq!(t.adjusted_duration_with_flaky(1.5), Duration::from_secs(15));
    }

    #[test]
    fn assertion_ignores_flaky_multiplier() {
        let t = IrTimeout::Assertion {
            duration: Duration::from_secs(5),
            span: IrSpan::synthetic(),
        };
        assert_eq!(t.adjusted_duration_with_flaky(2.0), Duration::from_secs(5));
    }

    #[test]
    fn tolerance_flaky_multiplier_unit() {
        let t = IrTimeout::tolerance_scaled(Duration::from_secs(10), 1.0);
        assert_eq!(t.adjusted_duration_with_flaky(1.0), Duration::from_secs(10));
    }
}
