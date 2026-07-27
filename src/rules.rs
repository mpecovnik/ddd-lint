use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleSummary {
    pub code: &'static str,
    pub name: &'static str,
    pub documentation: &'static str,
}

pub const RULES: &[RuleSummary] = &[
    RuleSummary {
        code: "DM001",
        name: "nullable-model-field",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md",
    },
    RuleSummary {
        code: "DM002",
        name: "nullable-heavy-model",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm002-nullable-heavy-model.md",
    },
    RuleSummary {
        code: "DM003",
        name: "defaulted-model-field",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm003-defaulted-model-field.md",
    },
    RuleSummary {
        code: "CT001",
        name: "non-exhaustive-match",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct001-non-exhaustive-match.md",
    },
    RuleSummary {
        code: "CT002",
        name: "catch-all-on-closed-type",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct002-catch-all-on-closed-type.md",
    },
    RuleSummary {
        code: "CT003",
        name: "conditional-closed-type-dispatch",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct003-conditional-closed-type-dispatch.md",
    },
    RuleSummary {
        code: "CT004",
        name: "discriminator-match",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct004-discriminator-match.md",
    },
    RuleSummary {
        code: "DI001",
        name: "hidden-constructor-fallback",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/di001-hidden-constructor-fallback.md",
    },
    RuleSummary {
        code: "SUP001",
        name: "unused-suppression",
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/sup001-unused-suppression.md",
    },
];

fn find(code: &str) -> Option<&'static RuleSummary> {
    RULES.iter().find(|rule| rule.code == code)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleCode(&'static RuleSummary);

impl RuleCode {
    pub const fn summary(self) -> &'static RuleSummary {
        self.0
    }
}

impl Display for RuleCode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.code)
    }
}

impl FromStr for RuleCode {
    type Err = ParseRuleCodeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        find(value)
            .map(Self)
            .ok_or_else(|| ParseRuleCodeError(value.to_owned()))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ParseRuleCodeError(String);

impl Display for ParseRuleCodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown rule code: {}", self.0)
    }
}

impl Error for ParseRuleCodeError {}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{RULES, RuleCode};

    #[test]
    fn rule_codes_are_unique() {
        for (index, rule) in RULES.iter().enumerate() {
            assert!(
                !RULES[..index]
                    .iter()
                    .any(|candidate| candidate.code == rule.code),
                "duplicate rule code: {}",
                rule.code
            );
        }
    }

    #[test]
    fn parses_only_known_rule_codes() {
        assert_eq!(
            RuleCode::from_str("DM001")
                .expect("known rule should parse")
                .summary()
                .name,
            "nullable-model-field"
        );
        assert_eq!(
            RuleCode::from_str("UNKNOWN")
                .expect_err("unknown rule should fail")
                .to_string(),
            "unknown rule code: UNKNOWN"
        );
    }
}
