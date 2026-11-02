use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use crate::config::Dm002Config;
use crate::diagnostics::Diagnostic;
use crate::graph::{FieldInfo, ProjectGraph};
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
            .copied()
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
                "review whether nullable fields encode correlated states; independent absence may be intentional",
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
        if selected.contains("DM004") {
            diagnostics.extend(event_metadata_diagnostics(&nullable));
        }
    }
    diagnostics
}

fn event_metadata_diagnostics(nullable: &[&FieldInfo]) -> Vec<Diagnostic> {
    let fields = nullable
        .iter()
        .map(|field| (field.name.as_str(), *field))
        .collect::<BTreeMap<_, _>>();
    fields
        .values()
        .filter_map(|actor| {
            let prefix = actor.name.strip_suffix("_by")?;
            if prefix.chars().count() < 5 {
                return None;
            }
            let timestamp_name = format!("{prefix}_at");
            let timestamp = fields.get(timestamp_name.as_str())?;
            let first = if actor.range.start() <= timestamp.range.start() {
                actor
            } else {
                timestamp
            };
            Some(Diagnostic::new(
                &first.path,
                first.range,
                rule("DM004"),
                format!(
                    "nullable event metadata `{}` and `{}` share prefix `{prefix}`",
                    actor.name, timestamp.name
                ),
                "group the actor and timestamp into one optional metadata value or document independent absence",
            ))
        })
        .collect()
}

fn rule(code: &str) -> RuleCode {
    RuleCode::from_str(code).expect("domain-model rule code is registered")
}
