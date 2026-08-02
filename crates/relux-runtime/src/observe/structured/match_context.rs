use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// The exact place a pure-match ran, for failure diagnostics. Replaces the
/// bare shell string on `Failure::PureMatch`: a pure-match can fail in a
/// `fn`/`pure fn` body, a test preamble, an effect preamble/overlay, or a
/// shell block, and this names which. Serialized inside
/// `FailureRecord::PureMatch` and exported to the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../viewer/src/types/")
)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MatchContext {
    Fn { name: String },
    TestPreamble { name: String },
    EffectPreamble { name: String },
    Shell { name: String },
}

impl MatchContext {
    /// The bare name regardless of kind.
    pub fn name(&self) -> &str {
        match self {
            Self::Fn { name }
            | Self::TestPreamble { name }
            | Self::EffectPreamble { name }
            | Self::Shell { name } => name,
        }
    }

    /// The shell name, only when this is a shell context (feeds
    /// `Failure::Runtime.shell` and the TAP shell line). `None` otherwise.
    pub fn shell_name_ref(&self) -> Option<&str> {
        match self {
            Self::Shell { name } => Some(name),
            _ => None,
        }
    }

    /// Kind rendered with a space, for human labels.
    fn kind_label(&self) -> &'static str {
        match self {
            Self::Fn { .. } => "fn",
            Self::TestPreamble { .. } => "test preamble",
            Self::EffectPreamble { .. } => "effect preamble",
            Self::Shell { .. } => "shell",
        }
    }

    /// Backtick label for Ariadne/console, e.g. "fn `build_url`".
    pub fn backtick_label(&self) -> String {
        format!("{} `{}`", self.kind_label(), self.name())
    }
}

/// Single-quote label for summaries / thiserror `#[error]`, e.g.
/// "fn 'build_url'".
impl std::fmt::Display for MatchContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} '{}'", self.kind_label(), self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_labels_per_variant() {
        let cases = [
            (
                MatchContext::Fn {
                    name: "build_url".into(),
                },
                "fn 'build_url'",
                "fn `build_url`",
            ),
            (
                MatchContext::TestPreamble {
                    name: "login".into(),
                },
                "test preamble 'login'",
                "test preamble `login`",
            ),
            (
                MatchContext::EffectPreamble { name: "Db".into() },
                "effect preamble 'Db'",
                "effect preamble `Db`",
            ),
            (
                MatchContext::Shell {
                    name: "default".into(),
                },
                "shell 'default'",
                "shell `default`",
            ),
        ];
        for (mc, display, backtick) in cases {
            assert_eq!(mc.to_string(), display);
            assert_eq!(mc.backtick_label(), backtick);
        }
    }

    #[test]
    fn shell_name_ref_only_for_shell() {
        assert_eq!(
            MatchContext::Shell { name: "sh".into() }.shell_name_ref(),
            Some("sh")
        );
        assert_eq!(MatchContext::Fn { name: "f".into() }.shell_name_ref(), None);
        assert_eq!(
            MatchContext::TestPreamble { name: "t".into() }.shell_name_ref(),
            None
        );
        assert_eq!(
            MatchContext::EffectPreamble { name: "e".into() }.shell_name_ref(),
            None
        );
    }

    #[test]
    fn serde_tag_is_kebab_kind() {
        let mc = MatchContext::EffectPreamble { name: "Db".into() };
        let json = serde_json::to_string(&mc).unwrap();
        assert!(
            json.contains("\"type\":\"effect-preamble\""),
            "json: {json}"
        );
        let back: MatchContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name(), "Db");
    }
}
