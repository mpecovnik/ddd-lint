use std::collections::BTreeSet;
use std::str::FromStr;

use crate::diagnostics::Diagnostic;
use crate::graph::ProjectGraph;
use crate::python::ConstructorFallbackKind;
use crate::rules::RuleCode;

pub fn run(graph: &ProjectGraph, enabled: &[RuleCode]) -> Vec<Diagnostic> {
    if !enabled.iter().any(|rule| rule.as_str() == "DI001") {
        return Vec::new();
    }
    let diagnostic_paths = graph
        .files()
        .iter()
        .filter(|file| file.diagnostic)
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    graph
        .constructor_fallbacks()
        .filter(|fallback| diagnostic_paths.contains(&fallback.path))
        .map(|fallback| {
            let form = match fallback.kind {
                ConstructorFallbackKind::DirectDefault => "call expression as its default",
                ConstructorFallbackKind::OrExpression => "falsy `or` call fallback",
                ConstructorFallbackKind::ConditionalExpression => {
                    "conditional-expression call fallback"
                }
                ConstructorFallbackKind::NoneAssignment => "None-guarded call fallback",
            };
            Diagnostic::new(
                &fallback.path,
                fallback.range,
                RuleCode::from_str("DI001").expect("DI001 is registered"),
                format!(
                    "constructor parameter `{}` selects a {form} when omitted",
                    fallback.parameter
                ),
                "require explicit injection or move composition into a named factory",
            )
        })
        .collect()
}
