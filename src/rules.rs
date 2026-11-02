use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleFamily {
    pub selector: &'static str,
    pub name: &'static str,
}

pub const DOMAIN_MODEL: RuleFamily = RuleFamily {
    selector: "DM",
    name: "Domain model",
};
pub const CLOSED_TYPE: RuleFamily = RuleFamily {
    selector: "CT",
    name: "Closed type",
};
pub const DEPENDENCY_INJECTION: RuleFamily = RuleFamily {
    selector: "DI",
    name: "Dependency injection",
};
pub const SUPPRESSION_HYGIENE: RuleFamily = RuleFamily {
    selector: "SUP",
    name: "Suppression hygiene",
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleConfigField {
    pub name: &'static str,
    pub default: &'static str,
}

const NO_CONFIGURATION: &[RuleConfigField] = &[];
const CT003_CONFIGURATION: &[RuleConfigField] = &[RuleConfigField {
    name: "mode",
    default: "\"dispatch\"",
}];
const DM002_CONFIGURATION: &[RuleConfigField] = &[
    RuleConfigField {
        name: "min-nullable-fields",
        default: "3",
    },
    RuleConfigField {
        name: "min-nullable-ratio",
        default: "0.30",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleSummary {
    pub code: &'static str,
    pub name: &'static str,
    pub family: RuleFamily,
    pub documentation: &'static str,
    pub configuration: &'static [RuleConfigField],
}

pub const RULES: &[RuleSummary] = &[
    RuleSummary {
        code: "DM001",
        name: "nullable-model-field",
        family: DOMAIN_MODEL,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm001-nullable-model-field.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "DM002",
        name: "nullable-heavy-model",
        family: DOMAIN_MODEL,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm002-nullable-heavy-model.md",
        configuration: DM002_CONFIGURATION,
    },
    RuleSummary {
        code: "DM003",
        name: "defaulted-model-field",
        family: DOMAIN_MODEL,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/dm003-defaulted-model-field.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "CT001",
        name: "non-exhaustive-match",
        family: CLOSED_TYPE,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct001-non-exhaustive-match.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "CT002",
        name: "catch-all-on-closed-type",
        family: CLOSED_TYPE,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct002-catch-all-on-closed-type.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "CT003",
        name: "conditional-closed-type-dispatch",
        family: CLOSED_TYPE,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct003-conditional-closed-type-dispatch.md",
        configuration: CT003_CONFIGURATION,
    },
    RuleSummary {
        code: "CT004",
        name: "discriminator-match",
        family: CLOSED_TYPE,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/ct004-discriminator-match.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "DI001",
        name: "hidden-constructor-fallback",
        family: DEPENDENCY_INJECTION,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/di001-hidden-constructor-fallback.md",
        configuration: NO_CONFIGURATION,
    },
    RuleSummary {
        code: "SUP001",
        name: "unused-suppression",
        family: SUPPRESSION_HYGIENE,
        documentation: "https://github.com/mpecovnik/ddd-lint/blob/main/docs/rules/sup001-unused-suppression.md",
        configuration: NO_CONFIGURATION,
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

    pub const fn as_str(self) -> &'static str {
        self.0.code
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

    #[test]
    fn registry_owns_family_and_rule_configuration() {
        let dm002 = RuleCode::from_str("DM002").expect("known rule").summary();
        assert_eq!(dm002.family.selector, "DM");
        assert_eq!(dm002.family.name, "Domain model");
        assert_eq!(
            dm002
                .configuration
                .iter()
                .map(|field| (field.name, field.default))
                .collect::<Vec<_>>(),
            [("min-nullable-fields", "3"), ("min-nullable-ratio", "0.30")]
        );
        assert!(
            RuleCode::from_str("DM001")
                .expect("known rule")
                .summary()
                .configuration
                .is_empty()
        );
    }
}
