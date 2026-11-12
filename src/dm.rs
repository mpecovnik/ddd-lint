use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use crate::config::Dm002Config;
use crate::diagnostics::Diagnostic;
use crate::graph::{DefaultKind, FieldInfo, ProjectGraph, ResolvedType};
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
        let start = diagnostics.len();
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
        for field in &fields {
            let (code, description) = match field.default_kind {
                DefaultKind::Required | DefaultKind::LiteralTag => continue,
                DefaultKind::Absence => ("DM005", "defaults to absence"),
                DefaultKind::Initial => ("DM006", "defaults to zero, false, or an empty sequence"),
                DefaultKind::Factory => ("DM007", "implicitly generates a default"),
                DefaultKind::Choice => ("DM011", "supplies an implicit choice"),
            };
            let code = if selected.contains("DM003") {
                "DM003"
            } else if selected.contains(code) {
                code
            } else {
                continue;
            };
            let mut diagnostic = Diagnostic::new(
                &field.path,
                field.range,
                rule(code),
                if code == "DM003" {
                    format!("model field `{}` has a default", field.name)
                } else {
                    format!("model field `{}` {description}", field.name)
                },
                "require callers to provide the field or move policy into a named factory",
            );
            diagnostic.fields.push(field.name.clone());
            diagnostic.evidence.push(description.to_owned());
            diagnostics.push(diagnostic);
        }
        if selected.contains("DM004") {
            diagnostics.extend(event_metadata_diagnostics(&nullable));
        }
        for relationship in graph.relationships(class) {
            if relationship.kind == "DM010" {
                continue;
            }
            if !selected.contains(relationship.kind)
                || !relationship
                    .fields
                    .iter()
                    .all(|name| nullable.iter().any(|field| &field.name == name))
            {
                continue;
            }
            let mut diagnostic = Diagnostic::new(
                &class.path,
                class.range,
                rule(relationship.kind),
                format!(
                    "{} invariant enforced at runtime for {}",
                    if relationship.kind == "DM008" {
                        "all-or-none"
                    } else {
                        "exactly-one"
                    },
                    relationship.fields.join(", ")
                ),
                "consider a structural model that represents this invariant directly",
            );
            diagnostic.fields = relationship.fields.clone();
            diagnostic.evidence.push(format!(
                "after model validator `{}` rejects incompatible presence combinations",
                relationship.method
            ));
            diagnostics.push(diagnostic);
        }
        if selected.contains("DM010") && !nullable.is_empty() {
            for status in fields.iter().filter(|field| matches!(&field.annotation, ResolvedType::Literal(values) if values.len() > 1)) {
                let mut diagnostic = Diagnostic::new(&class.path, status.range, rule("DM010"),
                    format!("finite-state field `{}` coexists with nullable payload", status.name),
                    "review whether payload presence depends on state; independent absence is legitimate");
                diagnostic.fields = std::iter::once(status.name.clone()).chain(nullable.iter().map(|field| field.name.clone())).collect();
                diagnostic.confidence = "low";
                diagnostic.evidence.push("multi-value Literal and nullable fields coexist; dependency is not proven".to_owned());
                let relationships = graph.relationships(class);
                let validators = relationships.iter().filter(|relationship| relationship.kind == "DM010" && relationship.fields.contains(&status.name)
                    && relationship.fields.iter().any(|name| nullable.iter().any(|field| &field.name == name))).collect::<Vec<_>>();
                if !validators.is_empty() {
                    diagnostic.confidence = "high";
                    diagnostic.message = format!("state-dependent payload invariant for `{}` enforced at runtime", status.name);
                    diagnostic.remediation = "consider state variants with required payload fields".to_owned();
                    diagnostic.evidence = validators.iter().map(|relationship| format!("after model validator `{}` rejects state/presence combinations: {}", relationship.method, relationship.fields.join(", "))).collect();
                    diagnostic.fields = validators.iter().flat_map(|relationship| relationship.fields.clone()).collect::<BTreeSet<_>>().into_iter().collect();
                }
                if validators.is_empty() && !matches!(status.name.as_str(), "status" | "state")
                    && !status.name.ends_with("_status") && !status.name.ends_with("_state") { continue; }
                diagnostics.push(diagnostic);
            }
        }
        for diagnostic in &mut diagnostics[start..] {
            diagnostic.model = Some(class.qualified_name.clone());
            if diagnostic.fields.is_empty() {
                diagnostic.fields = nullable
                    .iter()
                    .filter(|field| {
                        diagnostic.rule.as_str() == "DM002" || field.range == diagnostic.range
                    })
                    .map(|field| field.name.clone())
                    .collect();
            }
            if matches!(diagnostic.rule.as_str(), "DM002" | "DM004" | "DM011") {
                diagnostic.confidence = "low";
            }
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
            let mut diagnostic = Diagnostic::new(
                &first.path,
                first.range,
                rule("DM004"),
                format!(
                    "nullable event metadata `{}` and `{}` share prefix `{prefix}`",
                    actor.name, timestamp.name
                ),
                "group the actor and timestamp into one optional metadata value or document independent absence",
            );
            diagnostic.fields = vec![actor.name.clone(), timestamp.name.clone()];
            diagnostic.evidence.push("shared event prefix; correlation is not proven".to_owned());
            Some(diagnostic)
        })
        .collect()
}

fn rule(code: &str) -> RuleCode {
    RuleCode::from_str(code).expect("domain-model rule code is registered")
}
