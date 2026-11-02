use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use num_bigint::BigInt;
use num_traits::FromPrimitive;
use rayon::prelude::*;

use crate::config::TargetVersion;
use crate::project::{DiscoveredFiles, Project};
use crate::python::{
    AssignmentFact, ByteRange, ClassFact, ClosedTypeSiteFact, Comment, ConstructorFallbackFact,
    FunctionFact, ImportFact, ModuleFacts, ParameterKind, ParseErrors, PythonVersion, SemanticExpr,
    SourceFile, SourceKind, SyntaxNode, parse_module,
};

#[derive(Clone, Debug)]
pub struct ProjectGraph {
    modules: BTreeMap<String, RawModule>,
    files: Vec<GraphFile>,
    classes: BTreeMap<String, ClassInfo>,
    functions: BTreeMap<String, FunctionInfo>,
    diagnostic_classes: Vec<ClassInfo>,
    diagnostic_functions: Vec<FunctionInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphFile {
    pub path: PathBuf,
    pub module: Option<String>,
    pub source_kind: SourceKind,
    pub diagnostic: bool,
    pub source: Option<SourceFile>,
    pub comments: Vec<Comment>,
    pub nodes: Vec<SyntaxNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassInfo {
    pub qualified_name: String,
    pub module: String,
    pub name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub bases: Vec<String>,
    pub fields: Vec<FieldInfo>,
    pub methods: BTreeMap<String, FunctionInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldInfo {
    pub name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub annotation: ResolvedType,
    pub defaulted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionInfo {
    pub qualified_name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub parameters: Vec<ParameterInfo>,
    pub returns: ResolvedType,
    pub locals: BTreeMap<String, ResolvedType>,
    pub bound_names: BTreeSet<String>,
    pub global_names: BTreeSet<String>,
    pub nonlocal_names: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterInfo {
    pub name: String,
    pub range: ByteRange,
    pub kind: ParameterKind,
    pub annotation: ResolvedType,
    pub defaulted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ResolvedType {
    Unknown,
    None,
    Any,
    Object,
    Class(String),
    Named(String),
    Union(Vec<Self>),
    Literal(Vec<LiteralValue>),
    Applied {
        origin: String,
        arguments: Vec<Self>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LiteralValue {
    String(String),
    Bytes(String),
    Number(String),
    Boolean(bool),
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelKind {
    NotModel,
    BaseModel,
    RootModel,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumKind {
    NotEnum,
    Enum,
    IntEnum,
    StrEnum,
    Flag,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosedDomain {
    ClassUnion(Vec<ClosedMember>),
    Literal(Vec<LiteralValue>),
    Enum { class: String, members: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClosedMember {
    Class(String),
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discriminator {
    pub field: String,
    pub values: BTreeMap<String, LiteralValue>,
}

#[derive(Debug)]
pub struct GraphBuild {
    graph: ProjectGraph,
    syntax_errors: Vec<ParseErrors>,
}

impl GraphBuild {
    pub fn files(&self) -> &[GraphFile] {
        self.graph.files()
    }

    pub fn syntax_errors(&self) -> &[ParseErrors] {
        &self.syntax_errors
    }

    pub fn into_rule_graph(self) -> Result<ProjectGraph, Vec<ParseErrors>> {
        if self.syntax_errors.is_empty() {
            Ok(self.graph)
        } else {
            Err(self.syntax_errors)
        }
    }
}

impl ProjectGraph {
    pub fn build(
        project: &Project,
        discovered: &DiscoveredFiles,
    ) -> Result<GraphBuild, GraphError> {
        let module_roots = normalized_roots(project.module_roots());
        let diagnostic_paths = discovered
            .diagnostics
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut supporting = discovered.supporting.clone();
        supporting.sort();
        supporting.dedup();
        let mut identities = BTreeMap::<String, ModuleIdentity>::new();
        let mut derived = BTreeMap::<PathBuf, Option<(String, bool)>>::new();
        for path in &supporting {
            let identity = derive_module(path, &module_roots)?;
            if let Some((name, is_package)) = &identity {
                let entry = identities.entry(name.clone()).or_default();
                let paths = if path.extension().is_some_and(|extension| extension == "pyi") {
                    &mut entry.stubs
                } else {
                    &mut entry.implementations
                };
                paths.push(path.clone());
                entry.is_package |= *is_package;
            }
            derived.insert(path.clone(), identity);
        }
        for (module, identity) in &identities {
            if identity.implementations.len() > 1
                || identity.stubs.len() > 1
                || !valid_source_pair(identity)
            {
                let mut paths = identity
                    .implementations
                    .iter()
                    .chain(&identity.stubs)
                    .cloned()
                    .collect::<Vec<_>>();
                paths.sort();
                return Err(GraphError::DuplicateModule {
                    module: module.clone(),
                    paths,
                });
            }
        }

        let target_version = python_version(project.config().target_version);
        let parsed = supporting
            .par_iter()
            .map(|path| {
                let source = fs::read_to_string(path).map_err(|source| GraphError::ReadSource {
                    path: path.clone(),
                    source,
                })?;
                Ok::<_, GraphError>((
                    path.clone(),
                    SourceKind::from_path(path),
                    derived.get(path).cloned().flatten(),
                    parse_module(path, source, target_version),
                ))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        let mut modules = BTreeMap::<String, RawModule>::new();
        let mut files = Vec::new();
        let mut syntax_errors = Vec::new();
        for (path, source_kind, module_identity, parsed) in parsed {
            files.push(GraphFile {
                path: path.clone(),
                module: module_identity.as_ref().map(|(name, _)| name.clone()),
                source_kind,
                diagnostic: source_kind == SourceKind::Python && diagnostic_paths.contains(&path),
                source: None,
                comments: Vec::new(),
                nodes: Vec::new(),
            });
            match parsed {
                Ok(parsed) => {
                    let file = files.last_mut().expect("graph file was inserted");
                    file.source = Some(parsed.source().clone());
                    file.comments = parsed.comments().to_vec();
                    file.nodes = parsed.nodes().to_vec();
                    if let Some((module_name, is_package)) = module_identity {
                        let module = modules
                            .entry(module_name.clone())
                            .or_insert_with(|| RawModule::new(module_name, is_package));
                        module.merge(parsed.facts().clone());
                    }
                }
                Err(errors) => {
                    let file = files.last_mut().expect("graph file was inserted");
                    file.source = Some(errors.source().clone());
                    file.comments = errors.comments().to_vec();
                    file.nodes = errors.nodes().to_vec();
                    syntax_errors.push(errors);
                }
            }
        }

        let resolver = Resolver::new(&modules);
        let mut classes = BTreeMap::new();
        let mut functions = BTreeMap::new();
        let mut diagnostic_classes = Vec::new();
        let mut diagnostic_functions = Vec::new();
        for (module_name, module) in &modules {
            for class in &module.facts.classes {
                index_class(
                    &resolver,
                    module_name,
                    None,
                    class,
                    &mut classes,
                    &mut diagnostic_classes,
                    &diagnostic_paths,
                );
                index_class_functions(
                    &resolver,
                    module_name,
                    None,
                    class,
                    &mut functions,
                    &mut diagnostic_functions,
                    &diagnostic_paths,
                );
            }
            for function in &module.facts.functions {
                index_function(
                    &resolver,
                    module_name,
                    None,
                    function,
                    &mut functions,
                    &mut diagnostic_functions,
                    &diagnostic_paths,
                );
            }
        }

        Ok(GraphBuild {
            graph: Self {
                modules,
                files,
                classes,
                functions,
                diagnostic_classes,
                diagnostic_functions,
            },
            syntax_errors,
        })
    }

    pub fn files(&self) -> &[GraphFile] {
        &self.files
    }

    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub fn class(&self, qualified_name: &str) -> Option<&ClassInfo> {
        self.classes.get(qualified_name)
    }

    pub fn function(&self, qualified_name: &str) -> Option<&FunctionInfo> {
        self.functions.get(qualified_name)
    }

    pub fn diagnostic_classes(&self) -> &[ClassInfo] {
        &self.diagnostic_classes
    }

    pub fn diagnostic_functions(&self) -> &[FunctionInfo] {
        &self.diagnostic_functions
    }

    pub fn closed_type_sites(&self) -> impl Iterator<Item = (&str, &ClosedTypeSiteFact)> {
        self.modules.iter().flat_map(|(module, raw)| {
            raw.facts
                .closed_type_sites
                .iter()
                .map(move |site| (module.as_str(), site))
        })
    }

    pub fn constructor_fallbacks(&self) -> impl Iterator<Item = &ConstructorFallbackFact> {
        self.modules
            .values()
            .flat_map(|module| &module.facts.constructor_fallbacks)
    }

    pub fn site_type(
        &self,
        module: &str,
        scope: Option<&str>,
        expression: &SemanticExpr,
    ) -> ResolvedType {
        match expression {
            SemanticExpr::Name(name) => {
                if let Some(resolved) =
                    scope.and_then(|scope| self.scoped_local(module, scope, name))
                {
                    return resolved.clone();
                }
                if self.is_locally_bound(module, scope, name) {
                    return ResolvedType::Unknown;
                }
                Resolver::new(&self.modules).resolve_name_type(module, name, &mut BTreeSet::new())
            }
            SemanticExpr::Attribute { value, name } => {
                self.attribute_type(&self.site_type(module, scope, value), name)
            }
            SemanticExpr::Call { .. } => Resolver::new(&self.modules).expression_type(
                module,
                expression,
                &mut BTreeSet::new(),
            ),
            _ => {
                Resolver::new(&self.modules).resolve_type(module, expression, &mut BTreeSet::new())
            }
        }
    }

    pub fn site_symbol(
        &self,
        module: &str,
        scope: Option<&str>,
        expression: &SemanticExpr,
    ) -> Option<String> {
        let mut root = expression;
        while let SemanticExpr::Attribute { value, .. } = root {
            root = value;
        }
        if let SemanticExpr::Name(name) = root
            && self.is_locally_bound(module, scope, name)
        {
            return None;
        }
        let resolver = Resolver::new(&self.modules);
        let canonical = resolver.canonical_symbol(module, expression, &mut BTreeSet::new())?;
        Some(
            resolver
                .resolve_export(&canonical, &mut BTreeSet::new())
                .unwrap_or(canonical),
        )
    }

    pub fn is_locally_bound(&self, module: &str, scope: Option<&str>, name: &str) -> bool {
        scope.is_some_and(|scope| self.scoped_binding(module, scope, name))
    }

    pub fn builtin_is_unshadowed(&self, module: &str, scope: Option<&str>, name: &str) -> bool {
        !self.is_locally_bound(module, scope, name)
            && self.modules.get(module).is_none_or(|raw| {
                !raw.facts.assignments.iter().any(|item| item.name == name)
                    && !raw.facts.classes.iter().any(|item| item.name == name)
                    && !raw.facts.functions.iter().any(|item| item.name == name)
                    && !raw
                        .facts
                        .imports
                        .iter()
                        .any(|item| import_binds(item, name))
            })
    }

    fn scoped_function(&self, module: &str, scope: &str) -> Option<&FunctionInfo> {
        let qualified = format!("{module}.{scope}");
        self.functions.get(&qualified).or_else(|| {
            self.classes
                .values()
                .flat_map(|class| class.methods.values())
                .find(|function| function.qualified_name == qualified)
        })
    }

    fn scoped_local(&self, module: &str, scope: &str, name: &str) -> Option<&ResolvedType> {
        let mut candidate = Some(scope);
        while let Some(scope) = candidate {
            if let Some(function) = self.scoped_function(module, scope) {
                if function.global_names.contains(name) {
                    return None;
                }
                if !function.nonlocal_names.contains(name)
                    && let Some(resolved) = function.locals.get(name)
                {
                    return Some(resolved);
                }
            }
            candidate = scope.rsplit_once('.').map(|(parent, _)| parent);
        }
        None
    }

    fn scoped_binding(&self, module: &str, scope: &str, name: &str) -> bool {
        let mut candidate = Some(scope);
        while let Some(scope) = candidate {
            if let Some(function) = self.scoped_function(module, scope) {
                if function.global_names.contains(name) {
                    return false;
                }
                if !function.nonlocal_names.contains(name) && function.bound_names.contains(name) {
                    return true;
                }
            }
            candidate = scope.rsplit_once('.').map(|(parent, _)| parent);
        }
        false
    }

    pub fn name_type(&self, module: &str, name: &str) -> ResolvedType {
        Resolver::new(&self.modules).resolve_name_type(module, name, &mut BTreeSet::new())
    }

    pub fn model_kind(&self, class: &str) -> ModelKind {
        self.model_kind_inner(class, &mut BTreeSet::new())
    }

    fn model_kind_inner(&self, class: &str, visiting: &mut BTreeSet<String>) -> ModelKind {
        if !visiting.insert(class.to_owned()) {
            return ModelKind::Unknown;
        }
        let Some(info) = self.classes.get(class) else {
            visiting.remove(class);
            return ModelKind::Unknown;
        };
        let mut unknown = false;
        for base in &info.bases {
            let kind = match base.as_str() {
                "pydantic.RootModel" => ModelKind::RootModel,
                "pydantic.BaseModel" => ModelKind::BaseModel,
                _ if self.classes.contains_key(base) => self.model_kind_inner(base, visiting),
                _ => ModelKind::Unknown,
            };
            match kind {
                ModelKind::RootModel => {
                    visiting.remove(class);
                    return ModelKind::RootModel;
                }
                ModelKind::BaseModel => {
                    visiting.remove(class);
                    return ModelKind::BaseModel;
                }
                ModelKind::Unknown => unknown = true,
                ModelKind::NotModel => {}
            }
        }
        visiting.remove(class);
        if unknown {
            ModelKind::Unknown
        } else {
            ModelKind::NotModel
        }
    }

    pub fn enum_kind(&self, class: &str) -> EnumKind {
        self.enum_kind_inner(class, &mut BTreeSet::new())
    }

    fn enum_kind_inner(&self, class: &str, visiting: &mut BTreeSet<String>) -> EnumKind {
        if !visiting.insert(class.to_owned()) {
            return EnumKind::Unknown;
        }
        let Some(info) = self.classes.get(class) else {
            visiting.remove(class);
            return EnumKind::Unknown;
        };
        let mut unknown = false;
        for base in &info.bases {
            let kind = match base.as_str() {
                "enum.Flag" | "enum.IntFlag" => EnumKind::Flag,
                "enum.StrEnum" => EnumKind::StrEnum,
                "enum.IntEnum" => EnumKind::IntEnum,
                "enum.Enum" => EnumKind::Enum,
                _ if self.classes.contains_key(base) => self.enum_kind_inner(base, visiting),
                _ => EnumKind::Unknown,
            };
            match kind {
                EnumKind::Flag => {
                    visiting.remove(class);
                    return EnumKind::Flag;
                }
                EnumKind::StrEnum | EnumKind::IntEnum | EnumKind::Enum => {
                    visiting.remove(class);
                    return kind;
                }
                EnumKind::Unknown => unknown = true,
                EnumKind::NotEnum => {}
            }
        }
        visiting.remove(class);
        if unknown {
            EnumKind::Unknown
        } else {
            EnumKind::NotEnum
        }
    }

    pub fn is_subclass(&self, child: &str, base: &str) -> Option<bool> {
        self.is_subclass_inner(child, base, &mut BTreeSet::new())
    }

    fn is_subclass_inner(
        &self,
        child: &str,
        base: &str,
        visiting: &mut BTreeSet<String>,
    ) -> Option<bool> {
        if child == base {
            return Some(true);
        }
        if !visiting.insert(child.to_owned()) {
            return None;
        }
        let info = self.classes.get(child)?;
        let mut unknown = false;
        for direct in &info.bases {
            if direct == base {
                visiting.remove(child);
                return Some(true);
            }
            if self.classes.contains_key(direct) {
                match self.is_subclass_inner(direct, base, visiting) {
                    Some(true) => {
                        visiting.remove(child);
                        return Some(true);
                    }
                    Some(false) => {}
                    None => unknown = true,
                }
            } else if !is_known_external(direct) {
                unknown = true;
            }
        }
        visiting.remove(child);
        if unknown { None } else { Some(false) }
    }

    pub fn declared_model_fields(&self, class: &str) -> Vec<&FieldInfo> {
        if self.model_kind(class) != ModelKind::BaseModel {
            return Vec::new();
        }
        self.classes.get(class).map_or_else(Vec::new, |class| {
            class
                .fields
                .iter()
                .filter(|field| {
                    !field.name.starts_with('_')
                        && !matches!(
                            &field.annotation,
                            ResolvedType::Applied { origin, .. } if origin == "typing.ClassVar"
                        )
                })
                .collect()
        })
    }

    pub fn top_level_admits_none(resolved: &ResolvedType) -> bool {
        matches!(resolved, ResolvedType::None)
            || matches!(resolved, ResolvedType::Union(members) if members.contains(&ResolvedType::None))
    }

    pub fn attribute_type(&self, resolved: &ResolvedType, attribute: &str) -> ResolvedType {
        match resolved {
            ResolvedType::Class(class) => {
                self.class_attribute_type(class, attribute, &mut BTreeSet::new())
            }
            ResolvedType::Union(members) => {
                let mut resolved_members = Vec::new();
                for member in members {
                    let value = self.attribute_type(member, attribute);
                    if value == ResolvedType::Unknown {
                        return ResolvedType::Unknown;
                    }
                    push_unique(&mut resolved_members, value);
                }
                union_type(resolved_members)
            }
            _ => ResolvedType::Unknown,
        }
    }

    fn class_attribute_type(
        &self,
        class: &str,
        attribute: &str,
        visiting: &mut BTreeSet<String>,
    ) -> ResolvedType {
        if !visiting.insert(class.to_owned()) {
            return ResolvedType::Unknown;
        }
        let resolved = (|| {
            let info = self.classes.get(class)?;
            if let Some(field) = info
                .fields
                .iter()
                .rev()
                .find(|field| field.name == attribute)
            {
                return Some(field.annotation.clone());
            }
            for base in &info.bases {
                if self.classes.contains_key(base) {
                    let value = self.class_attribute_type(base, attribute, visiting);
                    if value != ResolvedType::Unknown {
                        return Some(value);
                    }
                }
            }
            None
        })()
        .unwrap_or(ResolvedType::Unknown);
        visiting.remove(class);
        resolved
    }

    pub fn closed_domain(&self, resolved: &ResolvedType) -> Option<ClosedDomain> {
        match resolved {
            ResolvedType::Union(members) => self.closed_class_union(members),
            ResolvedType::Literal(values) => literal_domain(values),
            ResolvedType::Class(class) => self.closed_enum(class),
            _ => None,
        }
    }

    fn closed_class_union(&self, members: &[ResolvedType]) -> Option<ClosedDomain> {
        let mut closed = Vec::new();
        let mut classes = Vec::new();
        for member in members {
            match member {
                ResolvedType::Class(class) => {
                    push_unique(&mut classes, class.clone());
                    push_unique(&mut closed, ClosedMember::Class(class.clone()));
                }
                ResolvedType::None => push_unique(&mut closed, ClosedMember::None),
                _ => return None,
            }
        }
        if classes.len() < 2 {
            return None;
        }
        for (index, class) in classes.iter().enumerate() {
            for other in &classes[index + 1..] {
                if self.is_subclass(class, other) != Some(false)
                    || self.is_subclass(other, class) != Some(false)
                {
                    return None;
                }
            }
        }
        Some(ClosedDomain::ClassUnion(closed))
    }

    fn closed_enum(&self, class: &str) -> Option<ClosedDomain> {
        if !matches!(
            self.enum_kind(class),
            EnumKind::Enum | EnumKind::IntEnum | EnumKind::StrEnum
        ) {
            return None;
        }
        let (module_name, local_name) = split_qualified_class(class, &self.modules)?;
        let module = self.modules.get(module_name)?;
        let fact = find_class_fact(&module.facts.classes, local_name)?;
        let mut members = Vec::new();
        let mut literal_values = BTreeMap::<LiteralValue, String>::new();
        for assignment in &fact.members {
            if assignment.name.starts_with('_') {
                continue;
            }
            if let Some(SemanticExpr::Name(alias)) = &assignment.value
                && members.iter().any(|member| member == alias)
            {
                continue;
            }
            let Some(assignment_value) = assignment.value.as_ref() else {
                continue;
            };
            if let Some(value) = literal_value(assignment_value) {
                if literal_values.contains_key(&value) {
                    continue;
                }
                literal_values.insert(value, assignment.name.clone());
            }
            push_unique(&mut members, assignment.name.clone());
        }
        (!members.is_empty()).then(|| ClosedDomain::Enum {
            class: class.to_owned(),
            members,
        })
    }

    pub fn common_discriminator(&self, domain: &ClosedDomain) -> Option<Discriminator> {
        let ClosedDomain::ClassUnion(members) = domain else {
            return None;
        };
        let classes = members
            .iter()
            .filter_map(|member| match member {
                ClosedMember::Class(class) => Some(class),
                ClosedMember::None => None,
            })
            .collect::<Vec<_>>();
        let first = self.classes.get(*classes.first()?)?;
        for field in &first.fields {
            let ResolvedType::Literal(first_values) = &field.annotation else {
                continue;
            };
            let [first_value] = first_values.as_slice() else {
                continue;
            };
            let mut values = BTreeMap::from([(first.qualified_name.clone(), first_value.clone())]);
            let mut seen = vec![first_value.clone()];
            let mut valid = true;
            for class in &classes[1..] {
                let Some(info) = self.classes.get(*class) else {
                    valid = false;
                    break;
                };
                let Some(candidate) = info.fields.iter().find(|item| item.name == field.name)
                else {
                    valid = false;
                    break;
                };
                let ResolvedType::Literal(candidate_values) = &candidate.annotation else {
                    valid = false;
                    break;
                };
                let [candidate_value] = candidate_values.as_slice() else {
                    valid = false;
                    break;
                };
                if seen
                    .iter()
                    .any(|value| value == candidate_value || runtime_equal(value, candidate_value))
                {
                    valid = false;
                    break;
                }
                seen.push(candidate_value.clone());
                values.insert(info.qualified_name.clone(), candidate_value.clone());
            }
            if valid {
                return Some(Discriminator {
                    field: field.name.clone(),
                    values,
                });
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
struct RawModule {
    name: String,
    is_package: bool,
    facts: ModuleFacts,
}

impl RawModule {
    fn new(name: String, is_package: bool) -> Self {
        Self {
            name,
            is_package,
            facts: ModuleFacts {
                imports: Vec::new(),
                classes: Vec::new(),
                functions: Vec::new(),
                assignments: Vec::new(),
                closed_type_sites: Vec::new(),
                constructor_fallbacks: Vec::new(),
            },
        }
    }

    fn merge(&mut self, facts: ModuleFacts) {
        self.facts.imports.extend(facts.imports);
        self.facts.classes.extend(facts.classes);
        self.facts.functions.extend(facts.functions);
        self.facts.assignments.extend(facts.assignments);
        self.facts.closed_type_sites.extend(facts.closed_type_sites);
        self.facts
            .constructor_fallbacks
            .extend(facts.constructor_fallbacks);
    }
}

#[derive(Default)]
struct ModuleIdentity {
    implementations: Vec<PathBuf>,
    stubs: Vec<PathBuf>,
    is_package: bool,
}

struct Resolver<'a> {
    modules: &'a BTreeMap<String, RawModule>,
}

impl<'a> Resolver<'a> {
    fn new(modules: &'a BTreeMap<String, RawModule>) -> Self {
        Self { modules }
    }

    fn resolve_type(
        &self,
        module: &str,
        expression: &SemanticExpr,
        visiting: &mut BTreeSet<String>,
    ) -> ResolvedType {
        match expression {
            SemanticExpr::None => ResolvedType::None,
            SemanticExpr::Forward { expression, .. } => expression
                .as_deref()
                .map_or(ResolvedType::Unknown, |expression| {
                    self.resolve_type(module, expression, visiting)
                }),
            SemanticExpr::Union { left, right } => union_type(vec![
                self.resolve_type(module, left, visiting),
                self.resolve_type(module, right, visiting),
            ]),
            SemanticExpr::Name(name) => self.resolve_name_type(module, name, visiting),
            SemanticExpr::Attribute { .. } => self
                .canonical_symbol(module, expression, visiting)
                .map_or(ResolvedType::Unknown, |canonical| {
                    self.type_from_canonical(&canonical, visiting)
                }),
            SemanticExpr::Subscript { value, arguments } => {
                let Some(origin) = self.canonical_symbol(module, value, visiting) else {
                    return ResolvedType::Unknown;
                };
                match origin.as_str() {
                    "typing.Annotated" => {
                        arguments.first().map_or(ResolvedType::Unknown, |value| {
                            self.resolve_type(module, value, visiting)
                        })
                    }
                    "typing.Union" => union_type(
                        arguments
                            .iter()
                            .map(|value| self.resolve_type(module, value, visiting))
                            .collect(),
                    ),
                    "typing.Optional" => arguments.first().map_or(ResolvedType::Unknown, |value| {
                        union_type(vec![
                            self.resolve_type(module, value, visiting),
                            ResolvedType::None,
                        ])
                    }),
                    "typing.Literal" => {
                        let values = arguments
                            .iter()
                            .filter_map(literal_value)
                            .collect::<Vec<_>>();
                        if values.len() == arguments.len() {
                            ResolvedType::Literal(values)
                        } else {
                            ResolvedType::Unknown
                        }
                    }
                    _ => ResolvedType::Applied {
                        origin,
                        arguments: arguments
                            .iter()
                            .map(|value| self.resolve_type(module, value, visiting))
                            .collect(),
                    },
                }
            }
            _ => ResolvedType::Unknown,
        }
    }

    fn type_from_canonical(
        &self,
        canonical: &str,
        visiting: &mut BTreeSet<String>,
    ) -> ResolvedType {
        match canonical {
            "None" | "builtins.None" => ResolvedType::None,
            "typing.Any" => ResolvedType::Any,
            "builtins.object" => ResolvedType::Object,
            _ => {
                let resolved_canonical = self
                    .resolve_export(canonical, visiting)
                    .unwrap_or_else(|| canonical.to_owned());
                if self.class_exists(&resolved_canonical) {
                    return ResolvedType::Class(resolved_canonical);
                }
                let canonical = resolved_canonical.as_str();
                if let Some((module, assignment)) = self.assignment(canonical) {
                    let key = format!("type:{canonical}");
                    if !visiting.insert(key.clone()) {
                        return ResolvedType::Unknown;
                    }
                    let resolved = assignment
                        .annotation
                        .as_ref()
                        .or(assignment.value.as_ref())
                        .map_or(ResolvedType::Unknown, |expression| {
                            self.resolve_type(module, expression, visiting)
                        });
                    visiting.remove(&key);
                    return resolved;
                }
                if is_known_named(canonical) {
                    ResolvedType::Named(canonical.to_owned())
                } else {
                    ResolvedType::Unknown
                }
            }
        }
    }

    fn resolve_name_type(
        &self,
        module: &str,
        name: &str,
        visiting: &mut BTreeSet<String>,
    ) -> ResolvedType {
        let canonical = format!("{module}.{name}");
        if let Some((_, assignment)) = self.assignment(&canonical) {
            if let Some(annotation) = &assignment.annotation {
                return self.resolve_type(module, annotation, visiting);
            }
            if let Some(value) = &assignment.value {
                return self.expression_type(module, value, visiting);
            }
        }
        self.canonical_name(module, name, visiting)
            .map_or(ResolvedType::Unknown, |canonical| {
                self.type_from_canonical(&canonical, visiting)
            })
    }

    fn expression_type(
        &self,
        module: &str,
        expression: &SemanticExpr,
        visiting: &mut BTreeSet<String>,
    ) -> ResolvedType {
        match expression {
            SemanticExpr::Name(name) => self.resolve_name_type(module, name, visiting),
            SemanticExpr::Call { function, .. } => {
                let Some(canonical) = self.canonical_symbol(module, function, visiting) else {
                    return ResolvedType::Unknown;
                };
                let canonical = self
                    .resolve_export(&canonical, visiting)
                    .unwrap_or(canonical);
                if self.class_exists(&canonical) {
                    return ResolvedType::Class(canonical);
                }
                self.function(canonical.as_str()).map_or(
                    ResolvedType::Unknown,
                    |(function_module, function)| {
                        function
                            .returns
                            .as_ref()
                            .map_or(ResolvedType::Unknown, |returns| {
                                self.resolve_type(function_module, returns, visiting)
                            })
                    },
                )
            }
            SemanticExpr::Attribute { value, name } => {
                let base = self.expression_type(module, value, visiting);
                let ResolvedType::Class(class) = base else {
                    return ResolvedType::Unknown;
                };
                self.raw_class_field(&class, name).map_or(
                    ResolvedType::Unknown,
                    |(field_module, field)| {
                        field
                            .annotation
                            .as_ref()
                            .map_or(ResolvedType::Unknown, |annotation| {
                                self.resolve_type(field_module, annotation, visiting)
                            })
                    },
                )
            }
            _ => self.resolve_type(module, expression, visiting),
        }
    }

    fn canonical_symbol(
        &self,
        module: &str,
        expression: &SemanticExpr,
        visiting: &mut BTreeSet<String>,
    ) -> Option<String> {
        match expression {
            SemanticExpr::Name(name) => self.canonical_name(module, name, visiting),
            SemanticExpr::Attribute { value, name } => {
                let base = self.canonical_symbol(module, value, visiting)?;
                Some(format!("{base}.{name}"))
            }
            _ => None,
        }
    }

    fn canonical_base(
        &self,
        module: &str,
        expression: &SemanticExpr,
        visiting: &mut BTreeSet<String>,
    ) -> Option<String> {
        match expression {
            SemanticExpr::Subscript { value, .. } => self.canonical_symbol(module, value, visiting),
            _ => self.canonical_symbol(module, expression, visiting),
        }
    }

    fn canonical_name(
        &self,
        module: &str,
        name: &str,
        visiting: &mut BTreeSet<String>,
    ) -> Option<String> {
        let raw = self.modules.get(module)?;
        if raw
            .facts
            .classes
            .iter()
            .rev()
            .any(|class| class.name == name)
            || raw
                .facts
                .functions
                .iter()
                .rev()
                .any(|function| function.name == name)
        {
            return Some(format!("{module}.{name}"));
        }
        if let Some(assignment) = raw
            .facts
            .assignments
            .iter()
            .rev()
            .find(|assignment| assignment.name == name)
            && let Some(value) = &assignment.value
        {
            let key = format!("symbol:{module}.{name}");
            if !visiting.insert(key.clone()) {
                return None;
            }
            let canonical = self.canonical_symbol(module, value, visiting);
            visiting.remove(&key);
            if canonical.is_some() {
                return canonical;
            }
        }
        for import in raw.facts.imports.iter().rev() {
            match import {
                ImportFact::Import {
                    module: imported,
                    alias,
                    ..
                } => {
                    let binding = alias
                        .as_deref()
                        .unwrap_or_else(|| imported.split('.').next().unwrap_or(imported));
                    if binding == name {
                        return Some(if alias.is_some() {
                            imported.clone()
                        } else {
                            imported.split('.').next().unwrap_or(imported).to_owned()
                        });
                    }
                }
                ImportFact::From {
                    module: imported,
                    level,
                    name: imported_name,
                    alias,
                    ..
                } if alias.as_deref().unwrap_or(imported_name) == name => {
                    let base = absolute_import(raw, imported.as_deref(), *level)?;
                    return Some(if base.is_empty() {
                        imported_name.clone()
                    } else {
                        format!("{base}.{imported_name}")
                    });
                }
                _ => {}
            }
        }
        builtin_name(name)
    }

    fn resolve_export(&self, canonical: &str, visiting: &mut BTreeSet<String>) -> Option<String> {
        if self.class_exists(canonical) || self.function(canonical).is_some() {
            return Some(canonical.to_owned());
        }
        let (module, local) = split_canonical(canonical, self.modules)?;
        let key = format!("export:{canonical}");
        if !visiting.insert(key.clone()) {
            return None;
        }
        let resolved = self.canonical_name(module, local, visiting);
        visiting.remove(&key);
        match resolved {
            Some(resolved) if resolved != canonical => {
                self.resolve_export(&resolved, visiting).or(Some(resolved))
            }
            _ => None,
        }
    }

    fn class_exists(&self, canonical: &str) -> bool {
        self.modules.iter().any(|(module, raw)| {
            canonical
                .strip_prefix(&format!("{module}."))
                .is_some_and(|local| find_class_fact(&raw.facts.classes, local).is_some())
        })
    }

    fn assignment(&self, canonical: &str) -> Option<(&str, &AssignmentFact)> {
        let (module, local) = split_canonical(canonical, self.modules)?;
        self.modules.get(module).and_then(|raw| {
            raw.facts
                .assignments
                .iter()
                .rev()
                .find(|assignment| assignment.name == local)
                .map(|assignment| (module, assignment))
        })
    }

    fn function(&self, canonical: &str) -> Option<(&str, &FunctionFact)> {
        let (module, local) = split_canonical(canonical, self.modules)?;
        self.modules.get(module).and_then(|raw| {
            raw.facts
                .functions
                .iter()
                .rev()
                .find(|function| function.name == local)
                .map(|function| (module, function))
        })
    }

    fn raw_class_field(&self, class: &str, name: &str) -> Option<(&str, &AssignmentFact)> {
        let (module, local) = split_qualified_class(class, self.modules)?;
        let fact = find_class_fact(&self.modules.get(module)?.facts.classes, local)?;
        fact.fields
            .iter()
            .rev()
            .find(|field| field.name == name)
            .map(|field| (module, field))
    }
}

fn index_class(
    resolver: &Resolver<'_>,
    module: &str,
    parent: Option<&str>,
    class: &ClassFact,
    classes: &mut BTreeMap<String, ClassInfo>,
    diagnostic_classes: &mut Vec<ClassInfo>,
    diagnostic_paths: &BTreeSet<PathBuf>,
) {
    let local_name = parent.map_or_else(
        || class.name.clone(),
        |parent| format!("{parent}.{}", class.name),
    );
    let qualified_name = format!("{module}.{local_name}");
    let bases = class
        .bases
        .iter()
        .map(|base| {
            let canonical = resolver
                .canonical_base(module, base, &mut BTreeSet::new())
                .unwrap_or_else(|| "<unknown>".to_owned());
            resolver
                .resolve_export(&canonical, &mut BTreeSet::new())
                .unwrap_or(canonical)
        })
        .collect();
    let fields = class
        .fields
        .iter()
        .map(|field| resolve_field(resolver, module, field))
        .collect();
    let methods = class
        .methods
        .iter()
        .map(|function| {
            let info = resolve_function(resolver, module, Some(&local_name), function);
            (function.name.clone(), info)
        })
        .collect();
    let info = ClassInfo {
        qualified_name: qualified_name.clone(),
        module: module.to_owned(),
        name: class.name.clone(),
        path: class.path.clone(),
        range: class.range,
        bases,
        fields,
        methods,
    };
    if SourceKind::from_path(&class.path) == SourceKind::Python
        && diagnostic_paths.contains(&class.path)
    {
        diagnostic_classes.push(info.clone());
    }
    classes.insert(qualified_name, info);
    for nested in &class.nested_classes {
        index_class(
            resolver,
            module,
            Some(&local_name),
            nested,
            classes,
            diagnostic_classes,
            diagnostic_paths,
        );
    }
}

fn index_class_functions(
    resolver: &Resolver<'_>,
    module: &str,
    parent: Option<&str>,
    class: &ClassFact,
    functions: &mut BTreeMap<String, FunctionInfo>,
    diagnostic_functions: &mut Vec<FunctionInfo>,
    diagnostic_paths: &BTreeSet<PathBuf>,
) {
    let local_name = parent.map_or_else(
        || class.name.clone(),
        |parent| format!("{parent}.{}", class.name),
    );
    for function in &class.methods {
        index_function(
            resolver,
            module,
            Some(&local_name),
            function,
            functions,
            diagnostic_functions,
            diagnostic_paths,
        );
    }
    for nested in &class.nested_classes {
        index_class_functions(
            resolver,
            module,
            Some(&local_name),
            nested,
            functions,
            diagnostic_functions,
            diagnostic_paths,
        );
    }
}

fn index_function(
    resolver: &Resolver<'_>,
    module: &str,
    parent: Option<&str>,
    function: &FunctionFact,
    functions: &mut BTreeMap<String, FunctionInfo>,
    diagnostic_functions: &mut Vec<FunctionInfo>,
    diagnostic_paths: &BTreeSet<PathBuf>,
) {
    let info = resolve_function(resolver, module, parent, function);
    if SourceKind::from_path(&function.path) == SourceKind::Python
        && diagnostic_paths.contains(&function.path)
    {
        diagnostic_functions.push(info.clone());
    }
    let local_name = info
        .qualified_name
        .strip_prefix(&format!("{module}."))
        .expect("qualified function belongs to module")
        .to_owned();
    functions.insert(info.qualified_name.clone(), info);
    for nested in &function.nested_functions {
        index_function(
            resolver,
            module,
            Some(&local_name),
            nested,
            functions,
            diagnostic_functions,
            diagnostic_paths,
        );
    }
}

fn resolve_field(resolver: &Resolver<'_>, module: &str, field: &AssignmentFact) -> FieldInfo {
    let annotation = field
        .annotation
        .as_ref()
        .map_or(ResolvedType::Unknown, |annotation| {
            resolver.resolve_type(module, annotation, &mut BTreeSet::new())
        });
    let defaulted = field
        .value
        .as_ref()
        .is_some_and(|value| field_value_is_defaulted(resolver, module, value))
        || annotated_field_default(resolver, module, field.annotation.as_ref());
    FieldInfo {
        name: field.name.clone(),
        path: field.path.clone(),
        range: field.range,
        annotation,
        defaulted,
    }
}

fn field_value_is_defaulted(resolver: &Resolver<'_>, module: &str, value: &SemanticExpr) -> bool {
    let SemanticExpr::Call {
        function,
        arguments,
        keywords,
    } = value
    else {
        return true;
    };
    if resolver
        .canonical_symbol(module, function, &mut BTreeSet::new())
        .as_deref()
        != Some("pydantic.Field")
    {
        return true;
    }
    arguments
        .first()
        .is_some_and(|argument| !matches!(argument, SemanticExpr::Ellipsis))
        || keywords.iter().any(|(name, value)| {
            matches!(name.as_deref(), Some("default_factory"))
                || (matches!(name.as_deref(), Some("default"))
                    && !matches!(value, SemanticExpr::Ellipsis))
        })
}

fn annotated_field_default(
    resolver: &Resolver<'_>,
    module: &str,
    annotation: Option<&SemanticExpr>,
) -> bool {
    let Some(SemanticExpr::Subscript { value, arguments }) = annotation else {
        return false;
    };
    if resolver
        .canonical_symbol(module, value, &mut BTreeSet::new())
        .as_deref()
        != Some("typing.Annotated")
    {
        return false;
    }
    arguments.iter().skip(1).any(|metadata| {
        matches!(metadata, SemanticExpr::Call { function, .. }
                if resolver.canonical_symbol(module, function, &mut BTreeSet::new()).as_deref()
                    == Some("pydantic.Field"))
            && field_value_is_defaulted(resolver, module, metadata)
    })
}

fn resolve_function(
    resolver: &Resolver<'_>,
    module: &str,
    parent: Option<&str>,
    function: &FunctionFact,
) -> FunctionInfo {
    let local_name = parent.map_or_else(
        || function.name.clone(),
        |parent| format!("{parent}.{}", function.name),
    );
    let parameters = function
        .parameters
        .iter()
        .map(|parameter| ParameterInfo {
            name: parameter.name.clone(),
            range: parameter.range,
            kind: parameter.kind,
            annotation: parameter
                .annotation
                .as_ref()
                .map_or(ResolvedType::Unknown, |annotation| {
                    resolver.resolve_type(module, annotation, &mut BTreeSet::new())
                }),
            defaulted: parameter.default.is_some(),
        })
        .collect::<Vec<_>>();
    let mut locals = parameters
        .iter()
        .map(|parameter| (parameter.name.clone(), parameter.annotation.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut bound_names = function
        .bound_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let global_names = function
        .global_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let nonlocal_names = function
        .nonlocal_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    bound_names.retain(|name| !global_names.contains(name) && !nonlocal_names.contains(name));
    bound_names.extend(parameters.iter().map(|parameter| parameter.name.clone()));
    for assignment in &function.assignments {
        if global_names.contains(&assignment.name) || nonlocal_names.contains(&assignment.name) {
            continue;
        }
        let resolved = assignment.annotation.as_ref().map_or_else(
            || match assignment.value.as_ref() {
                Some(SemanticExpr::Name(name)) if locals.contains_key(name) => locals[name].clone(),
                Some(value) => resolver.expression_type(module, value, &mut BTreeSet::new()),
                None => ResolvedType::Unknown,
            },
            |annotation| resolver.resolve_type(module, annotation, &mut BTreeSet::new()),
        );
        locals.insert(assignment.name.clone(), resolved);
        bound_names.insert(assignment.name.clone());
    }
    FunctionInfo {
        qualified_name: format!("{module}.{local_name}"),
        path: function.path.clone(),
        range: function.range,
        parameters,
        returns: function
            .returns
            .as_ref()
            .map_or(ResolvedType::Unknown, |returns| {
                resolver.resolve_type(module, returns, &mut BTreeSet::new())
            }),
        locals,
        bound_names,
        global_names,
        nonlocal_names,
    }
}

fn normalized_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = roots
        .iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    roots
}

fn derive_module(path: &Path, roots: &[PathBuf]) -> Result<Option<(String, bool)>, GraphError> {
    let mut candidates = roots
        .iter()
        .filter_map(|root| {
            let relative = path.strip_prefix(root).ok()?;
            Some((root.components().count(), relative))
        })
        .collect::<Vec<_>>();
    let Some(deepest) = candidates.iter().map(|(depth, _)| *depth).max() else {
        return Ok(None);
    };
    candidates.retain(|(depth, _)| *depth == deepest);
    let mut names = BTreeSet::new();
    let mut package = false;
    for (_, relative) in candidates {
        let mut components = relative
            .components()
            .map(|component| component.as_os_str().to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>();
        let Some(mut components) = components.take() else {
            return Ok(None);
        };
        let Some(file) = components.pop() else {
            continue;
        };
        let stem = Path::new(&file)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if stem == "__init__" {
            package = true;
        } else if !stem.is_empty() {
            components.push(stem.to_owned());
        }
        if !components.is_empty() {
            names.insert(components.join("."));
        }
    }
    if names.len() > 1 {
        return Err(GraphError::AmbiguousModule {
            path: path.to_owned(),
            names: names.into_iter().collect(),
        });
    }
    Ok(names.into_iter().next().map(|name| (name, package)))
}

fn python_version(version: TargetVersion) -> PythonVersion {
    match version {
        TargetVersion::Py310 => PythonVersion::Py310,
        TargetVersion::Py311 => PythonVersion::Py311,
        TargetVersion::Py312 => PythonVersion::Py312,
        TargetVersion::Py313 => PythonVersion::Py313,
        TargetVersion::Py314 => PythonVersion::Py314,
        TargetVersion::Py315 => PythonVersion::Py315,
    }
}

fn absolute_import(module: &RawModule, imported: Option<&str>, level: u32) -> Option<String> {
    if level == 0 {
        return imported.map(str::to_owned).or(Some(String::new()));
    }
    let mut package = if module.is_package {
        module.name.split('.').collect::<Vec<_>>()
    } else {
        let mut parts = module.name.split('.').collect::<Vec<_>>();
        parts.pop();
        parts
    };
    if package.is_empty() {
        return None;
    }
    for _ in 1..level {
        package.pop()?;
    }
    if package.is_empty() {
        return None;
    }
    if let Some(imported) = imported {
        package.extend(imported.split('.'));
    }
    Some(package.join("."))
}

fn split_canonical<'canonical, 'modules>(
    canonical: &'canonical str,
    modules: &'modules BTreeMap<String, RawModule>,
) -> Option<(&'modules str, &'canonical str)> {
    modules
        .keys()
        .filter_map(|module| {
            canonical
                .strip_prefix(module)
                .and_then(|suffix| suffix.strip_prefix('.'))
                .map(|local| (module.as_str(), local))
        })
        .max_by_key(|(module, _)| module.len())
}

fn split_qualified_class<'canonical, 'modules>(
    canonical: &'canonical str,
    modules: &'modules BTreeMap<String, RawModule>,
) -> Option<(&'modules str, &'canonical str)> {
    split_canonical(canonical, modules)
}

fn find_class_fact<'a>(classes: &'a [ClassFact], local: &str) -> Option<&'a ClassFact> {
    let (head, tail) = local.split_once('.').unwrap_or((local, ""));
    let class = classes.iter().rev().find(|class| class.name == head)?;
    if tail.is_empty() {
        Some(class)
    } else {
        find_class_fact(&class.nested_classes, tail)
    }
}

fn import_binds(import: &ImportFact, name: &str) -> bool {
    match import {
        ImportFact::Import { module, alias, .. } => {
            alias
                .as_deref()
                .unwrap_or_else(|| module.split('.').next().unwrap_or(module))
                == name
        }
        ImportFact::From {
            name: imported,
            alias,
            ..
        } => alias.as_deref().unwrap_or(imported) == name,
        ImportFact::Wildcard { .. } => false,
    }
}

fn builtin_name(name: &str) -> Option<String> {
    match name {
        "object" | "str" | "bytes" | "int" | "float" | "bool" | "list" | "dict" | "set"
        | "tuple" | "type" => Some(format!("builtins.{name}")),
        _ => None,
    }
}

fn is_known_named(canonical: &str) -> bool {
    canonical.starts_with("builtins.")
        || canonical.starts_with("typing.")
        || canonical.starts_with("collections.abc.")
}

fn is_known_external(canonical: &str) -> bool {
    canonical.starts_with("pydantic.")
        || canonical.starts_with("enum.")
        || is_known_named(canonical)
}

fn union_type(types: Vec<ResolvedType>) -> ResolvedType {
    let mut members = Vec::new();
    for resolved in types {
        match resolved {
            ResolvedType::Union(nested) => {
                for member in nested {
                    push_unique(&mut members, member);
                }
            }
            other => push_unique(&mut members, other),
        }
    }
    if members.len() == 1 {
        members.pop().expect("single union member exists")
    } else {
        ResolvedType::Union(members)
    }
}

pub(crate) fn literal_value(expression: &SemanticExpr) -> Option<LiteralValue> {
    match expression {
        SemanticExpr::String(value) => Some(LiteralValue::String(value.clone())),
        SemanticExpr::Bytes(value) => Some(LiteralValue::Bytes(value.clone())),
        SemanticExpr::Forward { value, .. } => Some(LiteralValue::String(value.clone())),
        SemanticExpr::Number(value) => Some(LiteralValue::Number(value.replace('_', ""))),
        SemanticExpr::Boolean(value) => Some(LiteralValue::Boolean(*value)),
        SemanticExpr::None => Some(LiteralValue::None),
        _ => None,
    }
}

fn literal_domain(values: &[LiteralValue]) -> Option<ClosedDomain> {
    let mut unique = Vec::new();
    for value in values {
        if unique
            .iter()
            .any(|existing| existing == value || typing_equal(existing, value))
        {
            continue;
        }
        if unique.iter().any(|existing| runtime_equal(existing, value)) {
            return None;
        }
        unique.push(value.clone());
    }
    (!unique.is_empty()).then_some(ClosedDomain::Literal(unique))
}

pub(crate) fn typing_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
    let (LiteralValue::Number(left), LiteralValue::Number(right)) = (left, right) else {
        return false;
    };
    match (parse_numeric(left), parse_numeric(right)) {
        (Some(Numeric::Integer(left)), Some(Numeric::Integer(right))) => left == right,
        (Some(Numeric::Float(left)), Some(Numeric::Float(right))) => left == right,
        _ => false,
    }
}

fn runtime_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
    match (left, right) {
        (LiteralValue::Boolean(boolean), LiteralValue::Number(number))
        | (LiteralValue::Number(number), LiteralValue::Boolean(boolean)) => {
            numeric_equal(number, if *boolean { "1" } else { "0" })
        }
        (LiteralValue::Number(left), LiteralValue::Number(right)) => numeric_equal(left, right),
        _ => false,
    }
}

fn numeric_equal(left: &str, right: &str) -> bool {
    match (parse_numeric(left), parse_numeric(right)) {
        (Some(Numeric::Integer(left)), Some(Numeric::Integer(right))) => left == right,
        (Some(Numeric::Float(left)), Some(Numeric::Float(right))) => left == right,
        (Some(Numeric::Integer(integer)), Some(Numeric::Float(float)))
        | (Some(Numeric::Float(float)), Some(Numeric::Integer(integer))) => {
            float.is_finite()
                && float.fract() == 0.0
                && BigInt::from_f64(float).is_some_and(|float| float == integer)
        }
        _ => false,
    }
}

enum Numeric {
    Integer(BigInt),
    Float(f64),
}

fn parse_numeric(value: &str) -> Option<Numeric> {
    let value = value.replace('_', "");
    if value.contains(['.', 'e', 'E']) {
        return value.parse().ok().map(Numeric::Float);
    }
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value.as_str()), |value| (true, value));
    let unsigned = unsigned.strip_prefix('+').unwrap_or(unsigned);
    let (radix, digits) = if let Some(digits) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, digits)
    } else if let Some(digits) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        (8, digits)
    } else if let Some(digits) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        (2, digits)
    } else {
        (10, unsigned)
    };
    let mut integer = BigInt::parse_bytes(digits.as_bytes(), radix)?;
    if negative {
        integer = -integer;
    }
    Some(Numeric::Integer(integer))
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn valid_source_pair(identity: &ModuleIdentity) -> bool {
    let ([implementation], [stub]) = (
        identity.implementations.as_slice(),
        identity.stubs.as_slice(),
    ) else {
        return true;
    };
    implementation.with_extension("pyi") == *stub
}

#[derive(Debug)]
pub enum GraphError {
    ReadSource {
        path: PathBuf,
        source: std::io::Error,
    },
    DuplicateModule {
        module: String,
        paths: Vec<PathBuf>,
    },
    AmbiguousModule {
        path: PathBuf,
        names: Vec<String>,
    },
}

impl Display for GraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadSource { path, source } => {
                write!(
                    formatter,
                    "failed to read Python source {}: {source}",
                    path.display()
                )
            }
            Self::DuplicateModule { module, paths } => write!(
                formatter,
                "duplicate importable module `{module}`: {}",
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::AmbiguousModule { path, names } => write!(
                formatter,
                "ambiguous importable module for {}: {}",
                path.display(),
                names.join(", ")
            ),
        }
    }
}

impl Error for GraphError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use crate::project::Project;

    use super::{
        ClosedDomain, ClosedMember, EnumKind, GraphError, LiteralValue, ModelKind, ProjectGraph,
        ResolvedType,
    };

    fn write(path: &Path, source: &str) {
        fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("fixture directory");
        fs::write(path, source).expect("fixture should write");
    }

    fn build(files: &[(&str, &str)]) -> (TempDir, super::GraphBuild) {
        let directory = TempDir::new().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n",
        );
        for (path, source) in files {
            write(&directory.path().join(path), source);
        }
        let project = Project::resolve(directory.path(), None).expect("project should resolve");
        let discovered = project
            .discover_files(directory.path(), &[])
            .expect("files should resolve");
        let graph = ProjectGraph::build(&project, &discovered).expect("graph should build");
        (directory, graph)
    }

    #[test]
    fn derives_modules_from_deepest_roots_and_indexes_stubs() {
        let (_directory, build) = build(&[
            ("flat.py", "value: int\n"),
            ("src/pkg/__init__.py", "from .model import Model\n"),
            ("src/pkg/model.py", "class Model:\n    value: str\n"),
            ("src/contracts.pyi", "def load() -> pkg.model.Model: ...\n"),
            ("src/shadow.py", "class Shared:\n    runtime: str\n"),
            ("src/shadow.pyi", "class Shared:\n    interface: int\n"),
        ]);

        assert!(build.syntax_errors.is_empty());
        assert_eq!(
            build.graph.module_names().collect::<Vec<_>>(),
            ["contracts", "flat", "pkg", "pkg.model", "shadow"]
        );
        let stub = build
            .graph
            .files()
            .iter()
            .find(|file| file.path.ends_with("contracts.pyi"))
            .expect("stub should be indexed");
        assert!(!stub.diagnostic);
        assert_eq!(stub.module.as_deref(), Some("contracts"));
        assert!(
            build
                .graph
                .class("shadow.Shared")
                .expect("stub declaration should be authoritative")
                .path
                .ends_with("shadow.pyi")
        );
        assert!(
            build
                .graph
                .diagnostic_classes()
                .iter()
                .any(|class| class.path.ends_with("shadow.py"))
        );
        assert!(
            build
                .graph
                .diagnostic_classes()
                .iter()
                .all(|class| class.path.extension().unwrap() == "py")
        );
    }

    #[test]
    fn resolves_cross_file_models_aliases_returns_attributes_and_discriminators() {
        let (_directory, build) = build(&[
            (
                "src/domain/models.py",
                r#"import pydantic as p
from typing import Annotated, ClassVar, Literal

class DomainModel(p.BaseModel):
    ignored: ClassVar[str]

class Draft(DomainModel):
    kind: Literal["draft state"]
    payload: str

class Published(DomainModel):
    kind: Literal["published!"]
    payload: str

class SpecialDraft(Draft):
    extra: int

class Root(p.RootModel):
    root: str

class GenericRoot(p.RootModel[str]):
    pass

State = Draft | Published
Overlap = DomainModel | Draft

def make(value: Draft) -> "Draft":
    return value
"#,
            ),
            (
                "src/app.py",
                r#"from domain.models import Draft, State, make

state: State
same = state
created = make(Draft(kind="draft", payload="x"))
"#,
            ),
        ]);
        let graph = build.graph;

        assert_eq!(
            graph.model_kind("domain.models.Draft"),
            ModelKind::BaseModel
        );
        assert_eq!(
            graph.model_kind("domain.models.SpecialDraft"),
            ModelKind::BaseModel
        );
        assert_eq!(graph.model_kind("domain.models.Root"), ModelKind::RootModel);
        assert_eq!(
            graph.model_kind("domain.models.GenericRoot"),
            ModelKind::RootModel
        );
        assert_eq!(
            graph.name_type("app", "state"),
            ResolvedType::Union(vec![
                ResolvedType::Class("domain.models.Draft".to_owned()),
                ResolvedType::Class("domain.models.Published".to_owned()),
            ])
        );
        assert_eq!(
            graph.name_type("app", "same"),
            graph.name_type("app", "state")
        );
        assert_eq!(
            graph.name_type("app", "created"),
            ResolvedType::Class("domain.models.Draft".to_owned())
        );
        assert_eq!(
            graph.attribute_type(
                &ResolvedType::Class("domain.models.SpecialDraft".to_owned()),
                "payload"
            ),
            ResolvedType::Named("builtins.str".to_owned())
        );
        let fields = graph.declared_model_fields("domain.models.Draft");
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["kind", "payload"]
        );
        let domain = graph
            .closed_domain(&graph.name_type("app", "state"))
            .expect("class union should be closed");
        assert_eq!(
            domain,
            ClosedDomain::ClassUnion(vec![
                ClosedMember::Class("domain.models.Draft".to_owned()),
                ClosedMember::Class("domain.models.Published".to_owned()),
            ])
        );
        assert_eq!(
            graph
                .common_discriminator(&domain)
                .expect("tag should discriminate")
                .field,
            "kind"
        );
        assert!(
            graph
                .closed_domain(&graph.name_type("domain.models", "Overlap"))
                .is_none()
        );
    }

    #[test]
    fn resolves_nullable_forms_and_field_defaults_conservatively() {
        let (_directory, build) = build(&[(
            "models.py",
            r#"from pydantic import BaseModel, Field
from typing import Annotated, Optional

class Model(BaseModel):
    direct: str | None
    optional: Optional[str]
    nested: list[str | None]
    required: str = Field(...)
    factory: str = Field(default_factory=str)
    metadata: Annotated[str, Field(default="x")]
"#,
        )]);
        let graph = build.graph;
        let fields = graph
            .declared_model_fields("models.Model")
            .into_iter()
            .map(|field| (field.name.as_str(), field))
            .collect::<BTreeMap<_, _>>();

        assert!(ProjectGraph::top_level_admits_none(
            &fields["direct"].annotation
        ));
        assert!(ProjectGraph::top_level_admits_none(
            &fields["optional"].annotation
        ));
        assert!(!ProjectGraph::top_level_admits_none(
            &fields["nested"].annotation
        ));
        assert!(!fields["required"].defaulted);
        assert!(fields["factory"].defaulted);
        assert!(fields["metadata"].defaulted);
    }

    #[test]
    fn recognizes_enums_flags_aliases_and_runtime_equal_literals() {
        let (_directory, build) = build(&[(
            "types.py",
            r#"from enum import Enum, Flag
from typing import Literal

class Status(Enum):
    OPEN = "open"
    OPEN_ALIAS = OPEN
    CLOSED = "closed"

class Options(Flag):
    A = 1
    B = 2

Safe = Literal[1, 2]
Unsafe = Literal[1, True]
"#,
        )]);
        let graph = build.graph;

        assert_eq!(graph.enum_kind("types.Status"), EnumKind::Enum);
        assert_eq!(graph.enum_kind("types.Options"), EnumKind::Flag);
        assert_eq!(
            graph.closed_domain(&ResolvedType::Class("types.Status".to_owned())),
            Some(ClosedDomain::Enum {
                class: "types.Status".to_owned(),
                members: vec!["OPEN".to_owned(), "CLOSED".to_owned()]
            })
        );
        assert_eq!(
            graph.closed_domain(&graph.name_type("types", "Safe")),
            Some(ClosedDomain::Literal(vec![
                LiteralValue::Number("1".to_owned()),
                LiteralValue::Number("2".to_owned())
            ]))
        );
        assert!(
            graph
                .closed_domain(&graph.name_type("types", "Unsafe"))
                .is_none()
        );
        assert!(
            graph
                .closed_domain(&ResolvedType::Class("types.Options".to_owned()))
                .is_none()
        );
    }

    #[test]
    fn wildcard_and_dynamic_bases_do_not_create_speculative_facts() {
        let (_directory, build) = build(&[(
            "unknown.py",
            "from external import *\n\nclass Maybe(Base):\n    value: str\n",
        )]);

        assert_eq!(build.graph.model_kind("unknown.Maybe"), ModelKind::Unknown);
        assert_eq!(
            build.graph.name_type("unknown", "Base"),
            ResolvedType::Unknown
        );
    }

    #[test]
    fn resolves_package_reexports_and_function_local_declared_flow() {
        let (_directory, build) = build(&[
            ("src/pkg/model.py", "class User:\n    name: str\n"),
            (
                "src/pkg/factory.py",
                "from .model import User\n\ndef make(value: User) -> User:\n    copy = value\n    declared: User\n    return copy\n",
            ),
            (
                "src/pkg/__init__.py",
                "from .model import User\nfrom .factory import make\n",
            ),
            (
                "src/consumer.py",
                "from pkg import User, make\nitem: User\ncreated = make(item)\n",
            ),
        ]);
        let graph = build.graph;

        assert_eq!(
            graph.name_type("consumer", "item"),
            ResolvedType::Class("pkg.model.User".to_owned())
        );
        assert_eq!(
            graph.name_type("consumer", "created"),
            graph.name_type("consumer", "item")
        );
        let function = graph
            .function("pkg.factory.make")
            .expect("function should be indexed");
        assert_eq!(
            function.locals["copy"],
            ResolvedType::Class("pkg.model.User".to_owned())
        );
        assert_eq!(function.locals["declared"], function.locals["copy"]);
    }

    #[test]
    fn supports_signed_and_bytes_literals() {
        let (_directory, build) = build(&[(
            "types.py",
            "from typing import Literal\nValues = Literal[-1, +2, b\"raw\"]\nPrecise = Literal[9007199254740992, 9007199254740993]\nEquivalent = Literal[0x1, 1]\nIntFloat = Literal[1, 1.0]\n",
        )]);

        assert_eq!(
            build
                .graph
                .closed_domain(&build.graph.name_type("types", "Values")),
            Some(ClosedDomain::Literal(vec![
                LiteralValue::Number("-1".to_owned()),
                LiteralValue::Number("+2".to_owned()),
                LiteralValue::Bytes("b\"raw\"".to_owned()),
            ]))
        );
        assert!(
            build
                .graph
                .closed_domain(&build.graph.name_type("types", "Precise"))
                .is_some()
        );
        assert_eq!(
            build
                .graph
                .closed_domain(&build.graph.name_type("types", "Equivalent")),
            Some(ClosedDomain::Literal(vec![LiteralValue::Number(
                "0x1".to_owned()
            )]))
        );
        assert!(
            build
                .graph
                .closed_domain(&build.graph.name_type("types", "IntFloat"))
                .is_none()
        );
    }

    #[test]
    fn inherited_attribute_lookup_handles_multiple_branches() {
        let (_directory, build) = build(&[(
            "hierarchy.py",
            "class LeftRoot:\n    pass\n\nclass RightRoot:\n    value: str\n\nclass Left(LeftRoot):\n    pass\n\nclass Right(LeftRoot, RightRoot):\n    pass\n\nclass Diamond(Left, Right):\n    pass\n",
        )]);

        assert_eq!(
            build.graph.attribute_type(
                &ResolvedType::Class("hierarchy.Diamond".to_owned()),
                "value"
            ),
            ResolvedType::Named("builtins.str".to_owned())
        );
    }

    #[test]
    fn rejects_non_colocated_implementation_stub_collisions() {
        let directory = TempDir::new().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n",
        );
        write(&directory.path().join("pkg.py"), "value = 1\n");
        write(&directory.path().join("pkg/__init__.pyi"), "value: int\n");
        let project = Project::resolve(directory.path(), None).expect("project should resolve");
        let discovered = project
            .discover_files(directory.path(), &[])
            .expect("files should resolve");

        let error = ProjectGraph::build(&project, &discovered)
            .expect_err("non-colocated source pair should collide");

        assert!(matches!(error, GraphError::DuplicateModule { .. }));
    }

    #[test]
    fn duplicate_modules_across_workspace_members_are_actionable() {
        let directory = TempDir::new().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n\n[tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
        );
        for member in ["a", "b"] {
            write(
                &directory
                    .path()
                    .join(format!("packages/{member}/pyproject.toml")),
                &format!("[project]\nname = \"{member}\"\nversion = \"0.1.0\"\n"),
            );
            write(
                &directory
                    .path()
                    .join(format!("packages/{member}/src/domain/model.py")),
                "class Model: pass\n",
            );
        }
        let project = Project::resolve(directory.path(), None).expect("workspace should resolve");
        let discovered = project
            .discover_files(directory.path(), &[])
            .expect("files should resolve");

        let error =
            ProjectGraph::build(&project, &discovered).expect_err("duplicate module should fail");

        let GraphError::DuplicateModule { module, paths } = error else {
            panic!("unexpected graph error: {error}");
        };
        assert_eq!(module, "domain.model");
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn path_limited_workspace_build_indexes_supporting_member_files() {
        let directory = TempDir::new().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n\n[tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
        );
        for member in ["domain", "app"] {
            write(
                &directory
                    .path()
                    .join(format!("packages/{member}/pyproject.toml")),
                &format!("[project]\nname = \"{member}\"\nversion = \"0.1.0\"\n"),
            );
        }
        write(
            &directory.path().join("packages/domain/src/domain/model.py"),
            "class Model: pass\n",
        );
        write(
            &directory.path().join("packages/app/src/app/main.py"),
            "from domain.model import Model\nvalue: Model\n",
        );
        let app = directory.path().join("packages/app");
        let project = Project::resolve(&app, None).expect("workspace should resolve");
        let discovered = project
            .discover_files(&app, &[PathBuf::from("src")])
            .expect("files should resolve");
        let build = ProjectGraph::build(&project, &discovered).expect("graph should build");

        assert_eq!(
            build.graph.name_type("app.main", "value"),
            ResolvedType::Class("domain.model.Model".to_owned())
        );
        assert_eq!(
            build
                .graph
                .files()
                .iter()
                .filter(|file| file.diagnostic)
                .count(),
            1
        );
        assert_eq!(build.graph.files().len(), 2);
        assert!(build.graph.diagnostic_classes().is_empty());
    }

    #[test]
    fn build_order_is_deterministic_and_syntax_errors_are_retained() {
        let directory = TempDir::new().expect("temporary directory");
        write(
            &directory.path().join("pyproject.toml"),
            "[tool.ddd-lint]\n",
        );
        write(&directory.path().join("b.py"), "class B: pass\n");
        write(&directory.path().join("a.py"), "def broken(:\n");
        let project = Project::resolve(directory.path(), None).expect("project should resolve");
        let mut discovered = project
            .discover_files(directory.path(), &[])
            .expect("files should resolve");
        discovered.supporting.reverse();

        let build = ProjectGraph::build(&project, &discovered).expect("graph should build");

        assert_eq!(build.syntax_errors.len(), 1);
        assert_eq!(
            build
                .graph
                .files()
                .iter()
                .map(|file| file.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            ["a.py", "b.py"]
        );
        assert!(build.into_rule_graph().is_err());
    }
}
