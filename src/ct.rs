use std::collections::BTreeSet;
use std::str::FromStr;

use crate::config::{Ct003Config, Ct003Mode};
use crate::diagnostics::Diagnostic;
use crate::graph::{
    ClosedDomain, ClosedMember, LiteralValue, ProjectGraph, literal_value, typing_equal,
};
use crate::python::{
    ClosedTypeSiteFact, ConditionalFact, DirectTestFact, DispatchFact, MatchFact, PatternFact,
    SemanticExpr, SingletonFact,
};
use crate::rules::RuleCode;

pub fn run(graph: &ProjectGraph, enabled: &[RuleCode], ct003: &Ct003Config) -> Vec<Diagnostic> {
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
            ClosedTypeSiteFact::Dispatch(site) => &site.path,
        };
        if !diagnostic_paths.contains(path) {
            continue;
        }
        match site {
            ClosedTypeSiteFact::Match(site) => {
                analyze_match(graph, module, site, &selected, &mut diagnostics)
            }
            ClosedTypeSiteFact::Conditional(site)
                if selected.contains("CT003") && ct003.mode == Ct003Mode::Strict =>
            {
                analyze_conditional(graph, module, site, &mut diagnostics)
            }
            ClosedTypeSiteFact::Dispatch(site)
                if selected.contains("CT003") && ct003.mode == Ct003Mode::Dispatch =>
            {
                analyze_dispatch(graph, module, site, &mut diagnostics)
            }
            ClosedTypeSiteFact::Conditional(_) | ClosedTypeSiteFact::Dispatch(_) => {}
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
            if let PatternFact::CatchAll { range } = case.pattern
                && (case.guarded
                    || case.exhaustive_sink.as_ref().is_none_or(|sink| {
                        !matches!(
                            graph
                                .site_symbol(module, site.scope.as_deref(), sink)
                                .as_deref(),
                            Some("typing.assert_never" | "typing_extensions.assert_never")
                        )
                    }))
            {
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
    if member_selection(graph, module, site.scope.as_deref(), &site.test).is_some() {
        diagnostics.push(Diagnostic::new(
            &site.path,
            site.range,
            rule("CT003"),
            "conditional selects a member of a closed type",
            "use an exhaustive match statement for closed-type dispatch",
        ));
    }
}

fn analyze_dispatch(
    graph: &ProjectGraph,
    module: &str,
    site: &DispatchFact,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(selections) = site
        .tests
        .iter()
        .map(|test| member_selection(graph, module, site.scope.as_deref(), test))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    // Dispatch facts contain at least two tests, established by the parser adapter.
    let first = &selections[0];
    if !is_subject_reference(first.subject)
        || selections
            .iter()
            .any(|selection| selection.subject != first.subject)
    {
        return;
    }
    let members = selections
        .iter()
        .map(|selection| &selection.member)
        .collect::<BTreeSet<_>>();
    if members.len() >= 2 {
        diagnostics.push(Diagnostic::new(
            &site.path,
            site.range,
            rule("CT003"),
            "conditional dispatch selects multiple members of the same closed type",
            "use an exhaustive match statement for closed-type dispatch",
        ));
    }
}

fn is_subject_reference(expression: &SemanticExpr) -> bool {
    match expression {
        SemanticExpr::Name(_) => true,
        SemanticExpr::Attribute { value, .. } => is_subject_reference(value),
        _ => false,
    }
}

struct MemberSelection<'a> {
    subject: &'a SemanticExpr,
    member: String,
}

fn member_selection<'a>(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    test: &'a DirectTestFact,
) -> Option<MemberSelection<'a>> {
    match test {
        DirectTestFact::IsInstance { value, member } => {
            if graph.builtin_is_unshadowed(module, scope, "isinstance") {
                class_member_test(graph, module, scope, value, member)
            } else {
                None
            }
        }
        DirectTestFact::Compare { left, right, .. } => {
            type_member_comparison(graph, module, scope, left, right)
                .or_else(|| type_member_comparison(graph, module, scope, right, left))
                .or_else(|| value_member_comparison(graph, module, scope, left, right))
                .or_else(|| value_member_comparison(graph, module, scope, right, left))
        }
    }
}

fn class_member_test<'a>(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    value: &'a SemanticExpr,
    member: &SemanticExpr,
) -> Option<MemberSelection<'a>> {
    let Some(ClosedDomain::ClassUnion(members)) =
        graph.closed_domain(&graph.site_type(module, scope, value))
    else {
        return None;
    };
    let symbol = graph.site_symbol(module, scope, member)?;
    members
        .contains(&ClosedMember::Class(symbol.clone()))
        .then_some(MemberSelection {
            subject: value,
            member: symbol,
        })
}

fn type_member_comparison<'a>(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    type_call: &'a SemanticExpr,
    member: &SemanticExpr,
) -> Option<MemberSelection<'a>> {
    let SemanticExpr::Call {
        function,
        arguments,
        keywords,
    } = type_call
    else {
        return None;
    };
    if !matches!(function.as_ref(), SemanticExpr::Name(name) if name == "type")
        || arguments.len() != 1
        || !keywords.is_empty()
        || !graph.builtin_is_unshadowed(module, scope, "type")
    {
        return None;
    }
    class_member_test(graph, module, scope, &arguments[0], member)
}

fn value_member_comparison<'a>(
    graph: &ProjectGraph,
    module: &str,
    scope: Option<&str>,
    value: &'a SemanticExpr,
    member: &SemanticExpr,
) -> Option<MemberSelection<'a>> {
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
            && let Some(candidate) = literal_value(member)
            && let Some((class, _)) = discriminator
                .values
                .iter()
                .find(|(_, value)| literal_same(value, &candidate))
        {
            return Some(MemberSelection {
                subject: base,
                member: class.clone(),
            });
        }
    }
    let domain = graph.closed_domain(&graph.site_type(module, scope, value))?;
    let member = match domain {
        ClosedDomain::Literal(values) => {
            let candidate = literal_value(member)?;
            let value = values
                .iter()
                .find(|value| literal_same(value, &candidate))?;
            format_literal(value)
        }
        ClosedDomain::Enum { class, members } => {
            let member = enum_member(graph, module, scope, member, &class, &members)?;
            format!("{class}.{member}")
        }
        ClosedDomain::ClassUnion(_) => return None,
    };
    Some(MemberSelection {
        subject: value,
        member,
    })
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
