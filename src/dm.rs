use std::collections::BTreeSet;
use std::str::FromStr;

use crate::config::Dm002Config;
use crate::diagnostics::Diagnostic;
use crate::graph::ProjectGraph;
use crate::rules::RuleCode;

pub fn run(graph: &ProjectGraph, enabled: &[RuleCode], dm002: &Dm002Config) -> Vec<Diagnostic> {
    let selected = enabled
        .iter()
        .map(|rule| rule.as_str())
        .collect::<BTreeSet<_>>();
    if !selected.iter().any(|code| code.starts_with("DM")) {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for class in graph.diagnostic_classes() {
        let fields = graph.declared_model_fields(&class.qualified_name);
        if fields.is_empty() {
            continue;
        }
        let nullable = fields
            .iter()
            .filter(|field| ProjectGraph::top_level_admits_none(&field.annotation))
            .collect::<Vec<_>>();
        if selected.contains("DM001") {
            diagnostics.extend(nullable.iter().map(|field| {
                Diagnostic::new(
                    &field.path,
                    field.range,
                    rule("DM001"),
                    format!("model field `{}` explicitly admits None", field.name),
                    "represent absence as an explicit model variant or document a design exception",
                )
            }));
        }
        if selected.contains("DM002")
            && nullable.len() >= dm002.min_nullable_fields
            && nullable.len() as f64 / fields.len() as f64 >= dm002.min_nullable_ratio
        {
            diagnostics.push(Diagnostic::new(
                &class.path,
                class.range,
                rule("DM002"),
                format!(
                    "model has {} nullable fields out of {} declared fields",
                    nullable.len(),
                    fields.len()
                ),
                "split nullable state combinations into explicit model variants",
            ));
        }
        if selected.contains("DM003") {
            diagnostics.extend(fields.iter().filter(|field| field.defaulted).map(|field| {
                Diagnostic::new(
                    &field.path,
                    field.range,
                    rule("DM003"),
                    format!("model field `{}` has a default", field.name),
                    "require callers to provide the field or move policy into a named factory",
                )
            }));
        }
    }
    diagnostics
}

fn rule(code: &str) -> RuleCode {
    RuleCode::from_str(code).expect("domain-model rule code is registered")
}
