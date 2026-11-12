use std::collections::BTreeSet;
use std::str::FromStr;

use crate::diagnostics::Diagnostic;
use crate::graph::ProjectGraph;
use crate::python::ConstructorFallbackKind;
use crate::rules::RuleCode;

pub fn run(graph: &ProjectGraph, enabled: &[RuleCode]) -> Vec<Diagnostic> {
    if !enabled
        .iter()
        .any(|rule| matches!(rule.as_str(), "DI001" | "DI002"))
    {
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
        .filter(|fallback| !graph.immutable_fallback(fallback))
        .filter_map(|fallback| {
            if graph.mutable_builtin_fallback(fallback) {
                return (fallback.kind == ConstructorFallbackKind::DirectDefault
                    && enabled.iter().any(|rule| rule.as_str() == "DI002"))
                .then(|| {
                    Diagnostic::new(
                        &fallback.path,
                        fallback.range,
                        RuleCode::from_str("DI002").unwrap(),
                        format!(
                            "constructor parameter `{}` shares a mutable default across calls",
                            fallback.parameter
                        ),
                        "allocate mutable state per call instead of in the function signature",
                    )
                });
            }
            if !enabled.iter().any(|rule| rule.as_str() == "DI001") {
                return None;
            }
            let form = match fallback.kind {
                ConstructorFallbackKind::DirectDefault => "call expression as its default",
                ConstructorFallbackKind::OrExpression => "falsy `or` call fallback",
                ConstructorFallbackKind::ConditionalExpression => {
                    "conditional-expression call fallback"
                }
                ConstructorFallbackKind::NoneAssignment => "None-guarded call fallback",
            };
            Some(Diagnostic::new(
                &fallback.path,
                fallback.range,
                RuleCode::from_str("DI001").expect("DI001 is registered"),
                format!(
                    "constructor parameter `{}` selects a {form} when omitted",
                    fallback.parameter
                ),
                "require explicit injection or move composition into a named factory",
            ))
        })
        .collect()
}
