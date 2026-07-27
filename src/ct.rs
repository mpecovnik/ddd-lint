use std::collections::BTreeSet;
use std::str::FromStr;

use crate::diagnostics::Diagnostic;
use crate::graph::{
    ClosedDomain, ClosedMember, LiteralValue, ProjectGraph, literal_value, typing_equal,
};
use crate::python::{
    ClosedTypeSiteFact, ConditionalFact, DirectTestFact, MatchFact, PatternFact, SemanticExpr,
    SingletonFact,
};
use crate::rules::RuleCode;

pub fn run(graph: &ProjectGraph, enabled: &[RuleCode]) -> Vec<Diagnostic> {
    let selected = enabled
        .iter()
        .map(|rule| rule.as_str())
        .collect::<BTreeSet<_>>();
    if !selected.iter().any(|code| code.starts_with("CT")) {
        return Vec::new();
    }
    let diagnostic_paths = graph
        .files()
        .iter()
        .filter(|file| file.diagnostic)
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    for (module, site) in graph.closed_type_sites() {
        let path = match site {
            ClosedTypeSiteFact::Match(site) => &site.path,
            ClosedTypeSiteFact::Conditional(site) => &site.path,
        };
        if !diagnostic_paths.contains(path) {
            continue;
        }
        match site {
            ClosedTypeSiteFact::Match(site) => {
                analyze_match(graph, module, site, &selected, &mut diagnostics)
            }
            ClosedTypeSiteFact::Conditional(site) if selected.contains("CT003") => {
                analyze_conditional(graph, module, site, &mut diagnostics)
            }
            ClosedTypeSiteFact::Conditional(_) => {}
        }
    }
    diagnostics
}

fn analyze_match(
    graph: &ProjectGraph,
    module: &str,
    site: &MatchFact,
    selected: &BTreeSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if selected.contains("CT004")
        && let SemanticExpr::Attribute { value, name } = &site.subject
        && let Some(ClosedDomain::ClassUnion(members)) =
            graph.closed_domain(&graph.site_type(module, site.scope.as_deref(), value))
        && members
            .iter()
            .all(|member| matches!(member, ClosedMember::Class(_)))
        && graph
            .common_discriminator(&ClosedDomain::ClassUnion(members))
            .is_some_and(|discriminator| discriminator.field == *name)
    {
        diagnostics.push(Diagnostic::new(
            &site.path,
            site.subject_range,
            rule("CT004"),
            format!("match dispatches through discriminator `{name}`"),
            "match the union value with member class patterns",
        ));
    }

    let subject_type = graph.site_type(module, site.scope.as_deref(), &site.subject);
    let Some(domain) = graph.closed_domain(&subject_type) else {
        return;
    };

    if selected.contains("CT001") {
        let expected = domain_members(&domain);
        let mut covered = BTreeSet::new();
        for case in &site.cases {
            if !case.guarded {
                pattern_members(
                    graph,
                    module,
                    site.scope.as_deref(),
                    &domain,
                    &case.pattern,
                    &mut covered,
                );
            }
        }
        let missing = expected.difference(&covered).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            diagnostics.push(Diagnostic::new(
                &site.path,
                site.range,
                rule("CT001"),
                format!("match omits closed members: {}", missing.join(", ")),
                "add an unguarded explicit case for every closed member",
            ));
        }
    }

    if selected.contains("CT002") {
        for case in &site.cases {
            if let PatternFact::CatchAll { range } = case.pattern {
                diagnostics.push(Diagnostic::new(
                    &site.path,
                    range,
                    rule("CT002"),
                    "catch-all hides changes to the closed type",
                    "replace the wildcard or capture with explicit member cases",
                ));
            }
        }
    }
}

fn analyze_conditional(
    graph: &ProjectGraph,
    module: &str,
    site: &ConditionalFact,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let recognized = match &site.test {
        DirectTestFact::IsInstance { value, member } => {
            graph.builtin_is_unshadowed(module, site.scope.as_deref(), "isinstance")
                && class_member_test(graph, module, site.scope.as_deref(), value, member)
        }
        DirectTestFact::Compare { left, right, .. } => {
            type_member_comparison(graph, module, site.scope.as_deref(), left, right)
                || type_member_comparison(graph, module, site.scope.as_deref(), right, left)
                || value_member_comparison(graph, module, site.scope.as_deref(), left, right)
                || value_member_comparison(graph, module, site.scope.as_deref(), right, left)
        }
    };
    if recognized {
        diagnostics.push(Diagnostic::new(
            &site.path,
            site.range,
            rule("CT003"),
            "conditional selects a member of a closed type",
            "use an exhaustive match statement for closed-type dispatch",
        ));
    }
}

fn class_member_test(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    value: &SemanticExpr,
    member: &SemanticExpr,
) -> bool {
    let Some(ClosedDomain::ClassUnion(members)) =
        graph.closed_domain(&graph.site_type(module, scope, value))
    else {
        return false;
    };
    let Some(symbol) = graph.site_symbol(module, scope, member) else {
        return false;
    };
    members.contains(&ClosedMember::Class(symbol))
}

fn type_member_comparison(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    type_call: &SemanticExpr,
    member: &SemanticExpr,
) -> bool {
    let SemanticExpr::Call {
        function,
        arguments,
        keywords,
    } = type_call
    else {
        return false;
    };
    if !matches!(function.as_ref(), SemanticExpr::Name(name) if name == "type")
        || arguments.len() != 1
        || !keywords.is_empty()
        || !graph.builtin_is_unshadowed(module, scope, "type")
    {
        return false;
    }
    class_member_test(graph, module, scope, &arguments[0], member)
}

fn value_member_comparison(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    value: &SemanticExpr,
    member: &SemanticExpr,
) -> bool {
    if let SemanticExpr::Attribute { value: base, name } = value
        && let Some(ClosedDomain::ClassUnion(members)) =
            graph.closed_domain(&graph.site_type(module, scope, base))
        && members
            .iter()
            .all(|member| matches!(member, ClosedMember::Class(_)))
    {
        let domain = ClosedDomain::ClassUnion(members);
        if let Some(discriminator) = graph.common_discriminator(&domain)
            && discriminator.field == *name
            && literal_value(member).is_some_and(|candidate| {
                discriminator
                    .values
                    .values()
                    .any(|value| literal_same(value, &candidate))
            })
        {
            return true;
        }
    }
    let Some(domain) = graph.closed_domain(&graph.site_type(module, scope, value)) else {
        return false;
    };
    match domain {
        ClosedDomain::Literal(values) => literal_value(member)
            .is_some_and(|candidate| values.iter().any(|value| literal_same(value, &candidate))),
        ClosedDomain::Enum { class, members } => {
            enum_member(graph, module, scope, member, &class, &members).is_some()
        }
        ClosedDomain::ClassUnion(_) => false,
    }
}

fn pattern_members(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    domain: &ClosedDomain,
    pattern: &PatternFact,
    covered: &mut BTreeSet<String>,
) {
    match pattern {
        PatternFact::Class { class, .. } => {
            if let ClosedDomain::ClassUnion(members) = domain
                && let Some(symbol) = graph.site_symbol(module, scope, class)
                && members.contains(&ClosedMember::Class(symbol.clone()))
            {
                covered.insert(symbol);
            }
        }
        PatternFact::Value { value, .. } => match domain {
            ClosedDomain::Literal(values) => {
                if let Some(candidate) = literal_value(value)
                    && let Some(value) = values.iter().find(|value| literal_same(value, &candidate))
                {
                    covered.insert(format_literal(value));
                }
            }
            ClosedDomain::Enum { class, members } => {
                if let Some(member) = enum_member(graph, module, scope, value, class, members) {
                    covered.insert(format!("{class}.{member}"));
                }
            }
            ClosedDomain::ClassUnion(_) => {}
        },
        PatternFact::Singleton { value, .. } => {
            let value = match value {
                SingletonFact::None => LiteralValue::None,
                SingletonFact::True => LiteralValue::Boolean(true),
                SingletonFact::False => LiteralValue::Boolean(false),
            };
            match domain {
                ClosedDomain::ClassUnion(members)
                    if value == LiteralValue::None && members.contains(&ClosedMember::None) =>
                {
                    covered.insert("None".to_owned());
                }
                ClosedDomain::Literal(values) if values.contains(&value) => {
                    covered.insert(format_literal(&value));
                }
                _ => {}
            }
        }
        PatternFact::As { pattern, .. } => {
            pattern_members(graph, module, scope, domain, pattern, covered)
        }
        PatternFact::Or { patterns, .. } => {
            for pattern in patterns {
                pattern_members(graph, module, scope, domain, pattern, covered);
            }
        }
        PatternFact::CatchAll { .. } | PatternFact::Other { .. } => {}
    }
}

fn enum_member(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    expression: &SemanticExpr,
    class: &str,
    members: &[String],
) -> Option<String> {
    let SemanticExpr::Attribute { value, name } = expression else {
        return None;
    };
    (graph.site_symbol(module, scope, value).as_deref() == Some(class) && members.contains(name))
        .then(|| name.clone())
}

fn domain_members(domain: &ClosedDomain) -> BTreeSet<String> {
    match domain {
        ClosedDomain::ClassUnion(members) => members
            .iter()
            .map(|member| match member {
                ClosedMember::Class(class) => class.clone(),
                ClosedMember::None => "None".to_owned(),
            })
            .collect(),
        ClosedDomain::Literal(values) => values.iter().map(format_literal).collect(),
        ClosedDomain::Enum { class, members } => members
            .iter()
            .map(|member| format!("{class}.{member}"))
            .collect(),
    }
}

fn literal_same(left: &LiteralValue, right: &LiteralValue) -> bool {
    left == right || typing_equal(left, right)
}

fn format_literal(value: &LiteralValue) -> String {
    match value {
        LiteralValue::String(value) => format!("{value:?}"),
        LiteralValue::Bytes(value) | LiteralValue::Number(value) => value.clone(),
        LiteralValue::Boolean(true) => "True".to_owned(),
        LiteralValue::Boolean(false) => "False".to_owned(),
        LiteralValue::None => "None".to_owned(),
    }
}

fn rule(code: &str) -> RuleCode {
    RuleCode::from_str(code).expect("closed-type rule code is registered")
}
