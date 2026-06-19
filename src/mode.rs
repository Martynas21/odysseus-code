/// Directory (relative to the workspace root) where specification documents
/// live. Spec mode may only write `*.md` files under this directory.
pub const SPEC_DIR: &str = "docs/edds";

/// How the agent should approach the current turn.
///
/// `Implement` is the default and matches the historical behaviour: build the
/// feature, editing code as needed. `Spec` is a non-mutating mode whose only
/// output is a specification document describing the feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Implement,
    Spec,
}

impl Mode {
    /// The other mode — used to toggle with Shift+Tab in the TUI.
    pub fn next(self) -> Self {
        match self {
            Mode::Implement => Mode::Spec,
            Mode::Spec => Mode::Implement,
        }
    }

    /// Lowercase label for status lines and the `--mode` flag.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Implement => "implement",
            Mode::Spec => "spec",
        }
    }

    /// Parse a `--mode` value. Anything unrecognised falls back to `Implement`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "spec" => Mode::Spec,
            _ => Mode::Implement,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_cycles_both_ways() {
        assert_eq!(Mode::Implement.next(), Mode::Spec);
        assert_eq!(Mode::Spec.next(), Mode::Implement);
    }

    #[test]
    fn parse_maps_known_values() {
        assert_eq!(Mode::parse("spec"), Mode::Spec);
        assert_eq!(Mode::parse("SPEC"), Mode::Spec);
        assert_eq!(Mode::parse("implement"), Mode::Implement);
    }

    #[test]
    fn parse_falls_back_to_implement() {
        assert_eq!(Mode::parse("nonsense"), Mode::Implement);
        assert_eq!(Mode::parse(""), Mode::Implement);
    }

    #[test]
    fn default_is_implement() {
        assert_eq!(Mode::default(), Mode::Implement);
    }
}
