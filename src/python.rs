use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::source_order::SourceOrderVisitor;
use ruff_python_ast::{
    AnyNodeRef, BoolOp, CmpOp, ExceptHandler, Expr, ModModule, NodeKind, Number, Operator, Pattern,
    PySourceType, PythonVersion as RuffPythonVersion, Singleton, Stmt, UnaryOp,
};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::{Ranged, TextRange as RuffTextRange};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceKind {
    Python,
    Stub,
}

impl SourceKind {
    pub fn from_path(path: &Path) -> Self {
        if path.extension().is_some_and(|extension| extension == "pyi") {
            Self::Stub
        } else {
            Self::Python
        }
    }

    const fn to_ruff(self) -> PySourceType {
        match self {
            Self::Python => PySourceType::Python,
            Self::Stub => PySourceType::Stub,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PythonVersion {
    Py310,
    Py311,
    Py312,
    Py313,
    Py314,
    Py315,
}

impl PythonVersion {
    const fn to_ruff(self) -> RuffPythonVersion {
        match self {
            Self::Py310 => RuffPythonVersion::PY310,
            Self::Py311 => RuffPythonVersion::PY311,
            Self::Py312 => RuffPythonVersion::PY312,
            Self::Py313 => RuffPythonVersion::PY313,
            Self::Py314 => RuffPythonVersion::PY314,
            Self::Py315 => RuffPythonVersion::PY315,
        }
    }
}

impl Display for PythonVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Py310 => "3.10",
            Self::Py311 => "3.11",
            Self::Py312 => "3.12",
            Self::Py313 => "3.13",
            Self::Py314 => "3.14",
            Self::Py315 => "3.15",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn start(self) -> usize {
        self.start
    }

    pub const fn end(self) -> usize {
        self.end
    }

    pub const fn len(self) -> usize {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    fn from_ruff(range: RuffTextRange) -> Self {
        Self {
            start: range.start().to_usize(),
            end: range.end().to_usize(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceRange {
    pub start: SourceLocation,
    pub end: SourceLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    path: PathBuf,
    text: String,
    line_starts: Vec<usize>,
    multibyte_prefix: Vec<(usize, usize)>,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts =
            Vec::with_capacity(text.bytes().filter(|byte| *byte == b'\n').count() + 1);
        line_starts.push(0);
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        let mut extra_bytes = 0;
        let multibyte_prefix = text
            .char_indices()
            .filter_map(|(offset, character)| {
                let width = character.len_utf8();
                if width == 1 {
                    None
                } else {
                    extra_bytes += width - 1;
                    Some((offset, extra_bytes))
                }
            })
            .collect();

        Self {
            path: path.into(),
            text,
            line_starts,
            multibyte_prefix,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn source_kind(&self) -> SourceKind {
        SourceKind::from_path(&self.path)
    }

    pub fn slice(&self, range: ByteRange) -> Option<&str> {
        self.text.get(range.start..range.end)
    }

    pub fn location(&self, offset: usize) -> Option<SourceLocation> {
        if offset > self.text.len() || !self.text.is_char_boundary(offset) {
            return None;
        }

        let line_index = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.line_starts[line_index];
        let column = offset - line_start + 1 - self.extra_bytes_between(line_start, offset);

        Some(SourceLocation {
            line: line_index + 1,
            column,
        })
    }

    pub fn source_range(&self, range: ByteRange) -> Option<SourceRange> {
        Some(SourceRange {
            start: self.location(range.start)?,
            end: self.location(range.end)?,
        })
    }

    fn extra_bytes_between(&self, start: usize, end: usize) -> usize {
        self.extra_bytes_before(end) - self.extra_bytes_before(start)
    }

    fn extra_bytes_before(&self, offset: usize) -> usize {
        let index = self
            .multibyte_prefix
            .partition_point(|(character_start, _)| *character_start < offset);
        index
            .checked_sub(1)
            .map_or(0, |index| self.multibyte_prefix[index].1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comment {
    pub range: ByteRange,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SyntaxKind {
    Module,
    FunctionDefinition,
    ClassDefinition,
    ReturnStatement,
    TypeAliasStatement,
    Assignment,
    AnnotatedAssignment,
    IfStatement,
    MatchStatement,
    ImportStatement,
    ImportFromStatement,
    ExpressionStatement,
    OtherStatement,
    BooleanOperation,
    NamedExpression,
    BinaryOperation,
    UnaryOperation,
    ConditionalExpression,
    Comparison,
    Call,
    StringLiteral,
    BytesLiteral,
    NumberLiteral,
    BooleanLiteral,
    NoneLiteral,
    EllipsisLiteral,
    Attribute,
    Subscript,
    Name,
    List,
    Tuple,
    OtherExpression,
    ValuePattern,
    SingletonPattern,
    SequencePattern,
    MappingPattern,
    ClassPattern,
    StarPattern,
    AsPattern,
    OrPattern,
    Parameter,
    ParameterWithDefault,
    Arguments,
    Parameters,
    Keyword,
    Alias,
    MatchCase,
    Decorator,
    ElifElseClause,
    TypeParameters,
    Identifier,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SyntaxNode {
    pub kind: SyntaxKind,
    pub range: ByteRange,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct ParsedModule {
    source: SourceFile,
    source_kind: SourceKind,
    target_version: PythonVersion,
    comments: Vec<Comment>,
    nodes: Vec<SyntaxNode>,
    facts: ModuleFacts,
    _syntax: ModModule,
}

impl ParsedModule {
    pub fn source(&self) -> &SourceFile {
        &self.source
    }

    pub const fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    pub const fn target_version(&self) -> PythonVersion {
        self.target_version
    }

    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    pub fn facts(&self) -> &ModuleFacts {
        &self.facts
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModuleFacts {
    pub imports: Vec<ImportFact>,
    pub classes: Vec<ClassFact>,
    pub functions: Vec<FunctionFact>,
    pub assignments: Vec<AssignmentFact>,
    pub closed_type_sites: Vec<ClosedTypeSiteFact>,
    pub constructor_fallbacks: Vec<ConstructorFallbackFact>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstructorFallbackFact {
    pub path: PathBuf,
    pub range: ByteRange,
    pub parameter: String,
    pub kind: ConstructorFallbackKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstructorFallbackKind {
    DirectDefault,
    OrExpression,
    ConditionalExpression,
    NoneAssignment,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClosedTypeSiteFact {
    Match(MatchFact),
    Conditional(ConditionalFact),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchFact {
    pub path: PathBuf,
    pub scope: Option<String>,
    pub range: ByteRange,
    pub subject_range: ByteRange,
    pub subject: SemanticExpr,
    pub cases: Vec<MatchCaseFact>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchCaseFact {
    pub pattern: PatternFact,
    pub guarded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternFact {
    Value {
        range: ByteRange,
        value: SemanticExpr,
    },
    Singleton {
        range: ByteRange,
        value: SingletonFact,
    },
    Class {
        range: ByteRange,
        class: SemanticExpr,
    },
    As {
        range: ByteRange,
        pattern: Box<Self>,
    },
    Or {
        range: ByteRange,
        patterns: Vec<Self>,
    },
    CatchAll {
        range: ByteRange,
    },
    Other {
        range: ByteRange,
    },
}

impl PatternFact {
    pub const fn range(&self) -> ByteRange {
        match self {
            Self::Value { range, .. }
            | Self::Singleton { range, .. }
            | Self::Class { range, .. }
            | Self::As { range, .. }
            | Self::Or { range, .. }
            | Self::CatchAll { range }
            | Self::Other { range } => *range,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingletonFact {
    None,
    True,
    False,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConditionalFact {
    pub path: PathBuf,
    pub scope: Option<String>,
    pub range: ByteRange,
    pub test: DirectTestFact,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DirectTestFact {
    IsInstance {
        value: SemanticExpr,
        member: SemanticExpr,
    },
    Compare {
        left: SemanticExpr,
        operator: ComparisonFact,
        right: SemanticExpr,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComparisonFact {
    Equal,
    NotEqual,
    Is,
    IsNot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportFact {
    Import {
        module: String,
        alias: Option<String>,
        range: ByteRange,
    },
    From {
        module: Option<String>,
        level: u32,
        name: String,
        alias: Option<String>,
        range: ByteRange,
    },
    Wildcard {
        module: Option<String>,
        level: u32,
        range: ByteRange,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassFact {
    pub name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub bases: Vec<SemanticExpr>,
    pub decorators: Vec<SemanticExpr>,
    pub fields: Vec<AssignmentFact>,
    pub members: Vec<AssignmentFact>,
    pub methods: Vec<FunctionFact>,
    pub nested_classes: Vec<ClassFact>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionFact {
    pub name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub decorators: Vec<SemanticExpr>,
    pub parameters: Vec<ParameterFact>,
    pub returns: Option<SemanticExpr>,
    pub assignments: Vec<AssignmentFact>,
    pub nested_functions: Vec<FunctionFact>,
    pub bound_names: Vec<String>,
    pub global_names: Vec<String>,
    pub nonlocal_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    PositionalOnly,
    Positional,
    VariadicPositional,
    KeywordOnly,
    VariadicKeyword,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterFact {
    pub name: String,
    pub range: ByteRange,
    pub kind: ParameterKind,
    pub annotation: Option<SemanticExpr>,
    pub default: Option<SemanticExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssignmentFact {
    pub name: String,
    pub path: PathBuf,
    pub range: ByteRange,
    pub annotation: Option<SemanticExpr>,
    pub value: Option<SemanticExpr>,
    pub type_alias: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SemanticExpr {
    Name(String),
    Attribute {
        value: Box<Self>,
        name: String,
    },
    Subscript {
        value: Box<Self>,
        arguments: Vec<Self>,
    },
    Union {
        left: Box<Self>,
        right: Box<Self>,
    },
    Call {
        function: Box<Self>,
        arguments: Vec<Self>,
        keywords: Vec<(Option<String>, Self)>,
    },
    Tuple(Vec<Self>),
    List(Vec<Self>),
    String(String),
    Bytes(String),
    Forward {
        value: String,
        expression: Option<Box<Self>>,
    },
    Number(String),
    Boolean(bool),
    None,
    Ellipsis,
    Unknown,
}

pub fn parse_module(
    path: impl Into<PathBuf>,
    source: impl Into<String>,
    target_version: PythonVersion,
) -> Result<ParsedModule, ParseErrors> {
    let source = SourceFile::new(path, source);
    let source_kind = source.source_kind();
    let options =
        ParseOptions::from(source_kind.to_ruff()).with_target_version(target_version.to_ruff());
    let parsed = parse_unchecked(source.text(), options);

    let mut errors = parsed
        .errors()
        .iter()
        .map(|error| SyntaxError {
            kind: SyntaxErrorKind::InvalidSyntax,
            message: "invalid Python syntax".to_owned(),
            detail: error.error.to_string(),
            range: ByteRange::from_ruff(error.location),
        })
        .chain(
            parsed
                .unsupported_syntax_errors()
                .iter()
                .map(|error| SyntaxError {
                    kind: SyntaxErrorKind::UnsupportedSyntax,
                    message: format!("syntax is not supported on Python {target_version}"),
                    detail: error.to_string(),
                    range: ByteRange::from_ruff(error.range),
                }),
        )
        .collect::<Vec<_>>();

    let comments = parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .map(|token| {
            let range = ByteRange::from_ruff(token.range());
            Comment {
                range,
                text: source.slice(range).unwrap_or_default().to_owned(),
            }
        })
        .collect();
    let module = parsed
        .try_into_module()
        .expect("module parse options must produce a module")
        .into_syntax();
    let nodes = NodeCollector::collect(&module);

    if !errors.is_empty() {
        errors.sort_by_key(|error| (error.range.start, error.range.end));
        return Err(ParseErrors {
            source: Box::new(source),
            comments,
            nodes,
            errors,
        });
    }
    let facts = extract_module_facts(&module, source.path(), source.text(), target_version);

    Ok(ParsedModule {
        source,
        source_kind,
        target_version,
        comments,
        nodes,
        facts,
        _syntax: module,
    })
}

fn extract_module_facts(
    module: &ModModule,
    path: &Path,
    source: &str,
    target_version: PythonVersion,
) -> ModuleFacts {
    let mut imports = Vec::new();
    let mut classes = Vec::new();
    let mut functions = Vec::new();
    let mut assignments = Vec::new();
    extract_module_suite(
        &module.body,
        path,
        source,
        target_version,
        &mut imports,
        &mut classes,
        &mut functions,
        &mut assignments,
    );
    ModuleFacts {
        imports,
        classes,
        functions,
        assignments,
        closed_type_sites: ClosedTypeCollector::collect(module, path, source, target_version),
        constructor_fallbacks: ConstructorFallbackCollector::collect(module, path),
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_module_suite(
    statements: &[Stmt],
    path: &Path,
    source: &str,
    target_version: PythonVersion,
    imports: &mut Vec<ImportFact>,
    classes: &mut Vec<ClassFact>,
    functions: &mut Vec<FunctionFact>,
    assignments: &mut Vec<AssignmentFact>,
) {
    for statement in statements {
        match statement {
            Stmt::Import(import) => {
                imports.extend(import.names.iter().map(|alias| ImportFact::Import {
                    module: alias.name.to_string(),
                    alias: alias.asname.as_ref().map(ToString::to_string),
                    range: ByteRange::from_ruff(alias.range),
                }));
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    if alias.name.as_str() == "*" {
                        imports.push(ImportFact::Wildcard {
                            module: import.module.as_ref().map(ToString::to_string),
                            level: import.level,
                            range: ByteRange::from_ruff(alias.range),
                        });
                    } else {
                        imports.push(ImportFact::From {
                            module: import.module.as_ref().map(ToString::to_string),
                            level: import.level,
                            name: alias.name.to_string(),
                            alias: alias.asname.as_ref().map(ToString::to_string),
                            range: ByteRange::from_ruff(alias.range),
                        });
                    }
                }
            }
            Stmt::ClassDef(class) => {
                classes.push(extract_class(class, path, source, target_version));
            }
            Stmt::FunctionDef(function) => {
                functions.push(extract_function(function, path, source, target_version));
            }
            _ => assignments.extend(extract_assignments(statement, path, source, target_version)),
        }
        if !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            for_each_child_suite(statement, |suite| {
                extract_module_suite(
                    suite,
                    path,
                    source,
                    target_version,
                    imports,
                    classes,
                    functions,
                    assignments,
                );
            });
        }
    }
}

fn extract_class(
    class: &ruff_python_ast::StmtClassDef,
    path: &Path,
    source: &str,
    target_version: PythonVersion,
) -> ClassFact {
    let mut fields = Vec::new();
    let mut members = Vec::new();
    let mut methods = Vec::new();
    let mut nested_classes = Vec::new();
    extract_class_suite(
        &class.body,
        path,
        source,
        target_version,
        &mut fields,
        &mut members,
        &mut methods,
        &mut nested_classes,
    );
    ClassFact {
        name: class.name.to_string(),
        path: path.to_owned(),
        range: ByteRange::from_ruff(class.range),
        bases: class
            .arguments
            .iter()
            .flat_map(|arguments| &arguments.args)
            .map(|expression| semantic_expr(expression, source, target_version, false))
            .collect(),
        decorators: class
            .decorator_list
            .iter()
            .map(|decorator| semantic_expr(&decorator.expression, source, target_version, false))
            .collect(),
        fields,
        members,
        methods,
        nested_classes,
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_class_suite(
    statements: &[Stmt],
    path: &Path,
    source: &str,
    target_version: PythonVersion,
    fields: &mut Vec<AssignmentFact>,
    members: &mut Vec<AssignmentFact>,
    methods: &mut Vec<FunctionFact>,
    nested_classes: &mut Vec<ClassFact>,
) {
    for statement in statements {
        match statement {
            Stmt::FunctionDef(function) => {
                methods.push(extract_function(function, path, source, target_version));
            }
            Stmt::ClassDef(class) => {
                nested_classes.push(extract_class(class, path, source, target_version));
            }
            _ => {
                let assignments = extract_assignments(statement, path, source, target_version);
                fields.extend(
                    assignments
                        .iter()
                        .filter(|assignment| assignment.annotation.is_some())
                        .cloned(),
                );
                members.extend(assignments);
            }
        }
        if !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            for_each_child_suite(statement, |suite| {
                extract_class_suite(
                    suite,
                    path,
                    source,
                    target_version,
                    fields,
                    members,
                    methods,
                    nested_classes,
                );
            });
        }
    }
}

fn extract_function(
    function: &ruff_python_ast::StmtFunctionDef,
    path: &Path,
    source: &str,
    target_version: PythonVersion,
) -> FunctionFact {
    let parameters = function
        .parameters
        .posonlyargs
        .iter()
        .map(|parameter| {
            extract_parameter_with_default(
                parameter,
                ParameterKind::PositionalOnly,
                source,
                target_version,
            )
        })
        .chain(function.parameters.args.iter().map(|parameter| {
            extract_parameter_with_default(
                parameter,
                ParameterKind::Positional,
                source,
                target_version,
            )
        }))
        .chain(function.parameters.vararg.iter().map(|parameter| {
            extract_parameter(
                parameter,
                ParameterKind::VariadicPositional,
                source,
                target_version,
            )
        }))
        .chain(function.parameters.kwonlyargs.iter().map(|parameter| {
            extract_parameter_with_default(
                parameter,
                ParameterKind::KeywordOnly,
                source,
                target_version,
            )
        }))
        .chain(function.parameters.kwarg.iter().map(|parameter| {
            extract_parameter(
                parameter,
                ParameterKind::VariadicKeyword,
                source,
                target_version,
            )
        }))
        .collect();
    let assignments = function
        .body
        .iter()
        .flat_map(|statement| extract_assignments(statement, path, source, target_version))
        .collect();
    let mut nested_functions = Vec::new();
    collect_nested_functions(
        &function.body,
        path,
        source,
        target_version,
        &mut nested_functions,
    );
    let mut bound_names = Vec::new();
    collect_bound_names(&function.body, &mut bound_names);
    let mut global_names = Vec::new();
    let mut nonlocal_names = Vec::new();
    collect_scope_declarations(&function.body, &mut global_names, &mut nonlocal_names);
    FunctionFact {
        name: function.name.to_string(),
        path: path.to_owned(),
        range: ByteRange::from_ruff(function.range),
        decorators: function
            .decorator_list
            .iter()
            .map(|decorator| semantic_expr(&decorator.expression, source, target_version, false))
            .collect(),
        parameters,
        returns: function
            .returns
            .as_deref()
            .map(|expression| semantic_expr(expression, source, target_version, true)),
        assignments,
        nested_functions,
        bound_names,
        global_names,
        nonlocal_names,
    }
}

fn collect_scope_declarations(
    statements: &[Stmt],
    global_names: &mut Vec<String>,
    nonlocal_names: &mut Vec<String>,
) {
    for statement in statements {
        match statement {
            Stmt::Global(statement) => {
                global_names.extend(statement.names.iter().map(ToString::to_string));
            }
            Stmt::Nonlocal(statement) => {
                nonlocal_names.extend(statement.names.iter().map(ToString::to_string));
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => continue,
            _ => {}
        }
        for_each_child_suite(statement, |suite| {
            collect_scope_declarations(suite, global_names, nonlocal_names);
        });
    }
}

fn collect_nested_functions(
    statements: &[Stmt],
    path: &Path,
    source: &str,
    target_version: PythonVersion,
    output: &mut Vec<FunctionFact>,
) {
    for statement in statements {
        match statement {
            Stmt::FunctionDef(function) => {
                output.push(extract_function(function, path, source, target_version));
            }
            Stmt::ClassDef(_) => {}
            _ => for_each_child_suite(statement, |suite| {
                collect_nested_functions(suite, path, source, target_version, output);
            }),
        }
    }
}

fn collect_bound_names(statements: &[Stmt], output: &mut Vec<String>) {
    for statement in statements {
        output.extend(statement_bound_names(statement));
        match statement {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => for_each_child_suite(statement, |suite| collect_bound_names(suite, output)),
        }
        if let Stmt::Try(statement) = statement {
            for handler in &statement.handlers {
                let ExceptHandler::ExceptHandler(handler) = handler;
                if let Some(name) = &handler.name {
                    output.push(name.to_string());
                }
            }
        }
    }
}

fn for_each_child_suite(statement: &Stmt, mut visit: impl FnMut(&[Stmt])) {
    match statement {
        Stmt::For(statement) => {
            visit(&statement.body);
            visit(&statement.orelse);
        }
        Stmt::While(statement) => {
            visit(&statement.body);
            visit(&statement.orelse);
        }
        Stmt::If(statement) => {
            visit(&statement.body);
            for clause in &statement.elif_else_clauses {
                visit(&clause.body);
            }
        }
        Stmt::With(statement) => visit(&statement.body),
        Stmt::Match(statement) => {
            for case in &statement.cases {
                visit(&case.body);
            }
        }
        Stmt::Try(statement) => {
            visit(&statement.body);
            for handler in &statement.handlers {
                let ExceptHandler::ExceptHandler(handler) = handler;
                visit(&handler.body);
            }
            visit(&statement.orelse);
            visit(&statement.finalbody);
        }
        _ => {}
    }
}

fn statement_bound_names(statement: &Stmt) -> Vec<String> {
    match statement {
        Stmt::Import(import) => import
            .names
            .iter()
            .map(|alias| {
                alias.asname.as_ref().map_or_else(
                    || alias.name.split('.').next().unwrap_or_default().to_owned(),
                    ToString::to_string,
                )
            })
            .collect(),
        Stmt::ImportFrom(import) => import
            .names
            .iter()
            .filter(|alias| alias.name.as_str() != "*")
            .map(|alias| alias.asname.as_ref().unwrap_or(&alias.name).to_string())
            .collect(),
        Stmt::FunctionDef(function) => vec![function.name.to_string()],
        Stmt::ClassDef(class) => vec![class.name.to_string()],
        Stmt::Assign(assignment) => assignment
            .targets
            .iter()
            .flat_map(expression_bound_names)
            .collect(),
        Stmt::AnnAssign(assignment) => expression_bound_names(&assignment.target),
        Stmt::AugAssign(assignment) => expression_bound_names(&assignment.target),
        Stmt::For(statement) => expression_bound_names(&statement.target),
        Stmt::With(statement) => statement
            .items
            .iter()
            .filter_map(|item| item.optional_vars.as_deref())
            .flat_map(expression_bound_names)
            .collect(),
        _ => Vec::new(),
    }
}

fn expression_bound_names(expression: &Expr) -> Vec<String> {
    match expression {
        Expr::Name(name) => vec![name.id.to_string()],
        Expr::List(list) => list.elts.iter().flat_map(expression_bound_names).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(expression_bound_names).collect(),
        Expr::Starred(starred) => expression_bound_names(&starred.value),
        _ => Vec::new(),
    }
}

fn extract_parameter_with_default(
    parameter: &ruff_python_ast::ParameterWithDefault,
    kind: ParameterKind,
    source: &str,
    target_version: PythonVersion,
) -> ParameterFact {
    ParameterFact {
        name: parameter.name().to_string(),
        range: ByteRange::from_ruff(parameter.range),
        kind,
        annotation: parameter
            .annotation()
            .map(|expression| semantic_expr(expression, source, target_version, true)),
        default: parameter
            .default()
            .map(|expression| semantic_expr(expression, source, target_version, false)),
    }
}

fn extract_parameter(
    parameter: &ruff_python_ast::Parameter,
    kind: ParameterKind,
    source: &str,
    target_version: PythonVersion,
) -> ParameterFact {
    ParameterFact {
        name: parameter.name.to_string(),
        range: ByteRange::from_ruff(parameter.range),
        kind,
        annotation: parameter
            .annotation()
            .map(|expression| semantic_expr(expression, source, target_version, true)),
        default: None,
    }
}

fn extract_assignments(
    statement: &Stmt,
    path: &Path,
    source: &str,
    target_version: PythonVersion,
) -> Vec<AssignmentFact> {
    match statement {
        Stmt::AnnAssign(assignment) => expression_name(&assignment.target)
            .map(|name| {
                vec![AssignmentFact {
                    name,
                    path: path.to_owned(),
                    range: ByteRange::from_ruff(assignment.range),
                    annotation: Some(semantic_expr(
                        &assignment.annotation,
                        source,
                        target_version,
                        true,
                    )),
                    value: assignment
                        .value
                        .as_deref()
                        .map(|expression| semantic_expr(expression, source, target_version, false)),
                    type_alias: false,
                }]
            })
            .unwrap_or_default(),
        Stmt::Assign(assignment) => assignment
            .targets
            .iter()
            .filter_map(expression_name)
            .map(|name| AssignmentFact {
                name,
                path: path.to_owned(),
                range: ByteRange::from_ruff(assignment.range),
                annotation: None,
                value: Some(semantic_expr(
                    &assignment.value,
                    source,
                    target_version,
                    false,
                )),
                type_alias: false,
            })
            .collect(),
        Stmt::TypeAlias(alias) => expression_name(&alias.name)
            .map(|name| {
                vec![AssignmentFact {
                    name,
                    path: path.to_owned(),
                    range: ByteRange::from_ruff(alias.range),
                    annotation: None,
                    value: Some(semantic_expr(&alias.value, source, target_version, true)),
                    type_alias: true,
                }]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn expression_name(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Name(name) => Some(name.id.to_string()),
        _ => None,
    }
}

fn semantic_expr(
    expression: &Expr,
    source: &str,
    target_version: PythonVersion,
    annotation: bool,
) -> SemanticExpr {
    match expression {
        Expr::Name(name) => SemanticExpr::Name(name.id.to_string()),
        Expr::Attribute(attribute) => SemanticExpr::Attribute {
            value: Box::new(semantic_expr(
                &attribute.value,
                source,
                target_version,
                false,
            )),
            name: attribute.attr.to_string(),
        },
        Expr::Subscript(subscript) => SemanticExpr::Subscript {
            value: Box::new(semantic_expr(
                &subscript.value,
                source,
                target_version,
                false,
            )),
            arguments: match subscript.slice.as_ref() {
                Expr::Tuple(tuple) => tuple
                    .elts
                    .iter()
                    .map(|argument| semantic_expr(argument, source, target_version, true))
                    .collect(),
                argument => vec![semantic_expr(argument, source, target_version, true)],
            },
        },
        Expr::BinOp(binary) if binary.op == Operator::BitOr => SemanticExpr::Union {
            left: Box::new(semantic_expr(&binary.left, source, target_version, true)),
            right: Box::new(semantic_expr(&binary.right, source, target_version, true)),
        },
        Expr::Call(call) => SemanticExpr::Call {
            function: Box::new(semantic_expr(&call.func, source, target_version, false)),
            arguments: call
                .arguments
                .args
                .iter()
                .map(|argument| semantic_expr(argument, source, target_version, false))
                .collect(),
            keywords: call
                .arguments
                .keywords
                .iter()
                .map(|keyword| {
                    (
                        keyword.arg.as_ref().map(ToString::to_string),
                        semantic_expr(&keyword.value, source, target_version, false),
                    )
                })
                .collect(),
        },
        Expr::Tuple(tuple) => SemanticExpr::Tuple(
            tuple
                .elts
                .iter()
                .map(|element| semantic_expr(element, source, target_version, annotation))
                .collect(),
        ),
        Expr::List(list) => SemanticExpr::List(
            list.elts
                .iter()
                .map(|element| semantic_expr(element, source, target_version, annotation))
                .collect(),
        ),
        Expr::StringLiteral(string) if annotation => {
            let value = string.value.to_str().to_owned();
            SemanticExpr::Forward {
                expression: parse_forward_annotation(&value, target_version).map(Box::new),
                value,
            }
        }
        Expr::StringLiteral(string) => SemanticExpr::String(string.value.to_str().to_owned()),
        Expr::BytesLiteral(bytes) => SemanticExpr::Bytes(
            source
                .get(bytes.range.start().to_usize()..bytes.range.end().to_usize())
                .unwrap_or_default()
                .to_owned(),
        ),
        Expr::NumberLiteral(number) => SemanticExpr::Number(
            source
                .get(number.range.start().to_usize()..number.range.end().to_usize())
                .unwrap_or_default()
                .to_owned(),
        ),
        Expr::UnaryOp(unary) if matches!(unary.op, UnaryOp::UAdd | UnaryOp::USub) => {
            match unary.operand.as_ref() {
                Expr::NumberLiteral(_) => SemanticExpr::Number(
                    source
                        .get(unary.range.start().to_usize()..unary.range.end().to_usize())
                        .unwrap_or_default()
                        .to_owned(),
                ),
                _ => SemanticExpr::Unknown,
            }
        }
        Expr::BooleanLiteral(boolean) => SemanticExpr::Boolean(boolean.value),
        Expr::NoneLiteral(_) => SemanticExpr::None,
        Expr::EllipsisLiteral(_) => SemanticExpr::Ellipsis,
        _ => SemanticExpr::Unknown,
    }
}

fn parse_forward_annotation(
    annotation: &str,
    target_version: PythonVersion,
) -> Option<SemanticExpr> {
    let options =
        ParseOptions::from(Mode::Expression).with_target_version(target_version.to_ruff());
    let parsed = parse_unchecked(annotation, options);
    if !parsed.errors().is_empty() || !parsed.unsupported_syntax_errors().is_empty() {
        return None;
    }
    let expression = parsed.try_into_expression()?.into_syntax();
    Some(semantic_expr(
        &expression.body,
        annotation,
        target_version,
        true,
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConstructorDefault {
    None,
    Falsy,
    Other,
}

struct ConstructorFallbackCollector<'a> {
    path: &'a Path,
    scopes: Vec<SyntaxKind>,
    facts: Vec<ConstructorFallbackFact>,
}

impl<'a> ConstructorFallbackCollector<'a> {
    fn collect(module: &ModModule, path: &'a Path) -> Vec<ConstructorFallbackFact> {
        let mut collector = Self {
            path,
            scopes: Vec::new(),
            facts: Vec::new(),
        };
        for statement in &module.body {
            collector.visit_stmt(statement);
        }
        collector.facts
    }

    fn analyze(&mut self, function: &ruff_python_ast::StmtFunctionDef) {
        let defaults = function
            .parameters
            .iter_non_variadic_params()
            .filter_map(|parameter| {
                let default = parameter.default()?;
                if let Expr::Call(call) = default {
                    self.facts.push(ConstructorFallbackFact {
                        path: self.path.to_owned(),
                        range: ByteRange::from_ruff(call.range),
                        parameter: parameter.name().to_string(),
                        kind: ConstructorFallbackKind::DirectDefault,
                    });
                }
                Some((parameter.name().to_string(), constructor_default(default)))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        collect_body_fallbacks(&function.body, self.path, &defaults, &mut self.facts);
    }
}

impl<'a> SourceOrderVisitor<'a> for ConstructorFallbackCollector<'_> {
    fn enter_node(
        &mut self,
        node: AnyNodeRef<'a>,
    ) -> ruff_python_ast::visitor::source_order::TraversalSignal {
        match node {
            AnyNodeRef::StmtClassDef(_) => self.scopes.push(SyntaxKind::ClassDefinition),
            AnyNodeRef::StmtFunctionDef(function) => {
                if function.name.as_str() == "__init__"
                    && self.scopes.last() == Some(&SyntaxKind::ClassDefinition)
                {
                    self.analyze(function);
                }
                self.scopes.push(SyntaxKind::FunctionDefinition);
            }
            _ => {}
        }
        ruff_python_ast::visitor::source_order::TraversalSignal::Traverse
    }

    fn leave_node(&mut self, node: AnyNodeRef<'a>) {
        if matches!(
            node,
            AnyNodeRef::StmtClassDef(_) | AnyNodeRef::StmtFunctionDef(_)
        ) {
            self.scopes.pop();
        }
    }
}

fn constructor_default(expression: &Expr) -> ConstructorDefault {
    match expression {
        Expr::NoneLiteral(_) => ConstructorDefault::None,
        Expr::BooleanLiteral(boolean) if !boolean.value => ConstructorDefault::Falsy,
        Expr::NumberLiteral(number)
            if match &number.value {
                Number::Int(value) => value.as_u64() == Some(0),
                Number::Float(value) => *value == 0.0,
                Number::Complex { real, imag } => *real == 0.0 && *imag == 0.0,
            } =>
        {
            ConstructorDefault::Falsy
        }
        Expr::StringLiteral(string) if string.value.is_empty() => ConstructorDefault::Falsy,
        Expr::BytesLiteral(bytes) if bytes.value.is_empty() => ConstructorDefault::Falsy,
        Expr::Tuple(tuple) if tuple.elts.is_empty() => ConstructorDefault::Falsy,
        Expr::List(list) if list.elts.is_empty() => ConstructorDefault::Falsy,
        Expr::Dict(dict) if dict.items.is_empty() => ConstructorDefault::Falsy,
        Expr::Set(set) if set.elts.is_empty() => ConstructorDefault::Falsy,
        _ => ConstructorDefault::Other,
    }
}

fn collect_body_fallbacks(
    statements: &[Stmt],
    path: &Path,
    defaults: &std::collections::BTreeMap<String, ConstructorDefault>,
    facts: &mut Vec<ConstructorFallbackFact>,
) {
    for statement in statements {
        if let Some(value) = assignment_value(statement) {
            collect_assignment_expression_fallback(value, path, defaults, facts);
        }
        if let Stmt::If(statement) = statement
            && let Some(parameter) = none_comparison_parameter(&statement.test, CmpOp::Is)
            && defaults.get(parameter) == Some(&ConstructorDefault::None)
        {
            collect_none_assignments(&statement.body, path, parameter, facts);
        }
        if !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            for_each_child_suite(statement, |suite| {
                collect_body_fallbacks(suite, path, defaults, facts);
            });
        }
    }
}

fn assignment_value(statement: &Stmt) -> Option<&Expr> {
    match statement {
        Stmt::Assign(assignment) => Some(&assignment.value),
        Stmt::AnnAssign(assignment) => assignment.value.as_deref(),
        _ => None,
    }
}

fn collect_assignment_expression_fallback(
    expression: &Expr,
    path: &Path,
    defaults: &std::collections::BTreeMap<String, ConstructorDefault>,
    facts: &mut Vec<ConstructorFallbackFact>,
) {
    if let Expr::BoolOp(boolean) = expression
        && boolean.op == BoolOp::Or
        && boolean.values.len() == 2
        && let Expr::Name(parameter) = &boolean.values[0]
        && matches!(
            defaults.get(parameter.id.as_str()),
            Some(ConstructorDefault::None | ConstructorDefault::Falsy)
        )
        && let Expr::Call(call) = &boolean.values[1]
    {
        facts.push(ConstructorFallbackFact {
            path: path.to_owned(),
            range: ByteRange::from_ruff(call.range),
            parameter: parameter.id.to_string(),
            kind: ConstructorFallbackKind::OrExpression,
        });
    }
    if let Expr::If(conditional) = expression
        && let Some((parameter, call)) = conditional_fallback(conditional)
        && defaults.get(parameter) == Some(&ConstructorDefault::None)
    {
        facts.push(ConstructorFallbackFact {
            path: path.to_owned(),
            range: ByteRange::from_ruff(call.range),
            parameter: parameter.to_owned(),
            kind: ConstructorFallbackKind::ConditionalExpression,
        });
    }
}

fn conditional_fallback(
    conditional: &ruff_python_ast::ExprIf,
) -> Option<(&str, &ruff_python_ast::ExprCall)> {
    if let Some(parameter) = none_comparison_parameter(&conditional.test, CmpOp::Is)
        && matches!(conditional.orelse.as_ref(), Expr::Name(name) if name.id.as_str() == parameter)
        && let Expr::Call(call) = conditional.body.as_ref()
    {
        return Some((parameter, call));
    }
    if let Some(parameter) = none_comparison_parameter(&conditional.test, CmpOp::IsNot)
        && matches!(conditional.body.as_ref(), Expr::Name(name) if name.id.as_str() == parameter)
        && let Expr::Call(call) = conditional.orelse.as_ref()
    {
        return Some((parameter, call));
    }
    None
}

fn none_comparison_parameter(expression: &Expr, operator: CmpOp) -> Option<&str> {
    let Expr::Compare(comparison) = expression else {
        return None;
    };
    if comparison.ops.len() != 1
        || comparison.ops[0] != operator
        || comparison.comparators.len() != 1
    {
        return None;
    }
    match (comparison.left.as_ref(), &comparison.comparators[0]) {
        (Expr::Name(name), Expr::NoneLiteral(_)) => Some(name.id.as_str()),
        (Expr::NoneLiteral(_), Expr::Name(name)) => Some(name.id.as_str()),
        _ => None,
    }
}

fn collect_none_assignments(
    statements: &[Stmt],
    path: &Path,
    parameter: &str,
    facts: &mut Vec<ConstructorFallbackFact>,
) {
    for statement in statements {
        let targets = match statement {
            Stmt::Assign(assignment) => assignment.targets.as_slice(),
            Stmt::AnnAssign(assignment) => std::slice::from_ref(assignment.target.as_ref()),
            _ => &[],
        };
        if targets
            .iter()
            .any(|target| target_matches_parameter(target, parameter))
            && let Some(Expr::Call(call)) = assignment_value(statement)
        {
            facts.push(ConstructorFallbackFact {
                path: path.to_owned(),
                range: ByteRange::from_ruff(call.range),
                parameter: parameter.to_owned(),
                kind: ConstructorFallbackKind::NoneAssignment,
            });
        }
    }
}

fn target_matches_parameter(target: &Expr, parameter: &str) -> bool {
    match target {
        Expr::Name(name) => name.id.as_str() == parameter,
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == parameter
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "self")
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SyntaxErrorKind {
    InvalidSyntax,
    UnsupportedSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub message: String,
    pub detail: String,
    pub range: ByteRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseErrors {
    source: Box<SourceFile>,
    comments: Vec<Comment>,
    nodes: Vec<SyntaxNode>,
    errors: Vec<SyntaxError>,
}

impl ParseErrors {
    pub fn source(&self) -> &SourceFile {
        &self.source
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }
}

impl Display for ParseErrors {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let Some(first) = self.errors.first() else {
            return formatter.write_str("Python parsing failed");
        };
        let location = self
            .source
            .location(first.range.start)
            .unwrap_or(SourceLocation { line: 1, column: 1 });
        write!(
            formatter,
            "{}:{}:{}: {}",
            self.source.path().display(),
            location.line,
            location.column,
            first.message
        )?;
        if self.errors.len() > 1 {
            write!(formatter, " (and {} more errors)", self.errors.len() - 1)?;
        }
        Ok(())
    }
}

impl Error for ParseErrors {}

struct ClosedTypeCollector<'a> {
    path: &'a Path,
    source: &'a str,
    target_version: PythonVersion,
    scope: Vec<String>,
    excluded_expressions: usize,
    facts: Vec<ClosedTypeSiteFact>,
}

impl<'a> ClosedTypeCollector<'a> {
    fn collect(
        module: &ModModule,
        path: &'a Path,
        source: &'a str,
        target_version: PythonVersion,
    ) -> Vec<ClosedTypeSiteFact> {
        let mut collector = Self {
            path,
            source,
            target_version,
            scope: Vec::new(),
            excluded_expressions: 0,
            facts: Vec::new(),
        };
        for statement in &module.body {
            collector.visit_stmt(statement);
        }
        collector.facts
    }

    fn scope(&self) -> Option<String> {
        (!self.scope.is_empty()).then(|| self.scope.join("."))
    }

    fn collect_test(&mut self, expression: &Expr) {
        let mut tests = Vec::new();
        collect_direct_tests(expression, self.source, self.target_version, &mut tests);
        let scope = self.scope();
        self.facts.extend(tests.into_iter().map(|(range, test)| {
            ClosedTypeSiteFact::Conditional(ConditionalFact {
                path: self.path.to_owned(),
                scope: scope.clone(),
                range,
                test,
            })
        }));
    }
}

impl<'a> SourceOrderVisitor<'a> for ClosedTypeCollector<'_> {
    fn enter_node(
        &mut self,
        node: AnyNodeRef<'a>,
    ) -> ruff_python_ast::visitor::source_order::TraversalSignal {
        match node {
            AnyNodeRef::StmtClassDef(class) => self.scope.push(class.name.to_string()),
            AnyNodeRef::StmtFunctionDef(function) => self.scope.push(function.name.to_string()),
            AnyNodeRef::StmtMatch(statement) => {
                self.facts.push(ClosedTypeSiteFact::Match(MatchFact {
                    path: self.path.to_owned(),
                    scope: self.scope(),
                    range: ByteRange::from_ruff(statement.range),
                    subject_range: ByteRange::from_ruff(statement.subject.range()),
                    subject: semantic_expr(
                        &statement.subject,
                        self.source,
                        self.target_version,
                        false,
                    ),
                    cases: statement
                        .cases
                        .iter()
                        .map(|case| MatchCaseFact {
                            pattern: pattern_fact(&case.pattern, self.source, self.target_version),
                            guarded: case.guard.is_some(),
                        })
                        .collect(),
                }));
            }
            AnyNodeRef::StmtIf(statement) => {
                self.collect_test(&statement.test);
                for clause in &statement.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.collect_test(test);
                    }
                }
            }
            AnyNodeRef::ExprIf(expression) if self.excluded_expressions == 0 => {
                self.collect_test(&expression.test);
            }
            AnyNodeRef::ExprListComp(_)
            | AnyNodeRef::ExprSetComp(_)
            | AnyNodeRef::ExprDictComp(_)
            | AnyNodeRef::ExprGenerator(_)
            | AnyNodeRef::ExprLambda(_) => self.excluded_expressions += 1,
            _ => {}
        }
        ruff_python_ast::visitor::source_order::TraversalSignal::Traverse
    }

    fn leave_node(&mut self, node: AnyNodeRef<'a>) {
        match node {
            AnyNodeRef::StmtClassDef(_) | AnyNodeRef::StmtFunctionDef(_) => {
                self.scope.pop();
            }
            AnyNodeRef::ExprListComp(_)
            | AnyNodeRef::ExprSetComp(_)
            | AnyNodeRef::ExprDictComp(_)
            | AnyNodeRef::ExprGenerator(_)
            | AnyNodeRef::ExprLambda(_) => self.excluded_expressions -= 1,
            _ => {}
        }
    }
}

fn collect_direct_tests(
    expression: &Expr,
    source: &str,
    target_version: PythonVersion,
    facts: &mut Vec<(ByteRange, DirectTestFact)>,
) {
    match expression {
        Expr::UnaryOp(unary) if unary.op == UnaryOp::Not => {
            collect_direct_tests(&unary.operand, source, target_version, facts);
        }
        Expr::BoolOp(boolean) => {
            for value in &boolean.values {
                collect_direct_tests(value, source, target_version, facts);
            }
        }
        Expr::Call(call)
            if matches!(call.func.as_ref(), Expr::Name(name) if name.id.as_str() == "isinstance")
                && call.arguments.args.len() == 2
                && call.arguments.keywords.is_empty() =>
        {
            facts.push((
                ByteRange::from_ruff(call.range),
                DirectTestFact::IsInstance {
                    value: semantic_expr(&call.arguments.args[0], source, target_version, false),
                    member: semantic_expr(&call.arguments.args[1], source, target_version, false),
                },
            ));
        }
        Expr::Compare(comparison) => {
            let mut left = comparison.left.as_ref();
            for (operator, right) in comparison.ops.iter().zip(comparison.comparators.iter()) {
                if let Some(operator) = comparison_fact(*operator) {
                    facts.push((
                        ByteRange::from_ruff(comparison.range),
                        DirectTestFact::Compare {
                            left: semantic_expr(left, source, target_version, false),
                            operator,
                            right: semantic_expr(right, source, target_version, false),
                        },
                    ));
                }
                left = right;
            }
        }
        _ => {}
    }
}

fn comparison_fact(operator: CmpOp) -> Option<ComparisonFact> {
    match operator {
        CmpOp::Eq => Some(ComparisonFact::Equal),
        CmpOp::NotEq => Some(ComparisonFact::NotEqual),
        CmpOp::Is => Some(ComparisonFact::Is),
        CmpOp::IsNot => Some(ComparisonFact::IsNot),
        _ => None,
    }
}

fn pattern_fact(pattern: &Pattern, source: &str, target_version: PythonVersion) -> PatternFact {
    let range = ByteRange::from_ruff(pattern.range());
    if pattern.is_irrefutable() {
        return PatternFact::CatchAll { range };
    }
    match pattern {
        Pattern::MatchValue(pattern) => PatternFact::Value {
            range,
            value: semantic_expr(&pattern.value, source, target_version, false),
        },
        Pattern::MatchSingleton(pattern) => PatternFact::Singleton {
            range,
            value: match pattern.value {
                Singleton::None => SingletonFact::None,
                Singleton::True => SingletonFact::True,
                Singleton::False => SingletonFact::False,
            },
        },
        Pattern::MatchClass(pattern) => PatternFact::Class {
            range,
            class: semantic_expr(&pattern.cls, source, target_version, false),
        },
        Pattern::MatchAs(pattern) => {
            pattern
                .pattern
                .as_deref()
                .map_or(PatternFact::Other { range }, |inner| PatternFact::As {
                    range,
                    pattern: Box::new(pattern_fact(inner, source, target_version)),
                })
        }
        Pattern::MatchOr(pattern) => PatternFact::Or {
            range,
            patterns: pattern
                .patterns
                .iter()
                .map(|pattern| pattern_fact(pattern, source, target_version))
                .collect(),
        },
        _ => PatternFact::Other { range },
    }
}

struct NodeCollector {
    nodes: Vec<SyntaxNode>,
    depth: usize,
}

impl NodeCollector {
    fn collect(module: &ModModule) -> Vec<SyntaxNode> {
        let mut collector = Self {
            nodes: vec![SyntaxNode {
                kind: SyntaxKind::Module,
                range: ByteRange::from_ruff(module.range),
                depth: 0,
            }],
            depth: 1,
        };
        for statement in &module.body {
            collector.visit_stmt(statement);
        }
        collector.nodes
    }
}

impl<'a> SourceOrderVisitor<'a> for NodeCollector {
    fn enter_node(
        &mut self,
        node: AnyNodeRef<'a>,
    ) -> ruff_python_ast::visitor::source_order::TraversalSignal {
        self.nodes.push(SyntaxNode {
            kind: map_node_kind(node.kind()),
            range: ByteRange::from_ruff(node.range()),
            depth: self.depth,
        });
        self.depth += 1;
        ruff_python_ast::visitor::source_order::TraversalSignal::Traverse
    }

    fn leave_node(&mut self, _node: AnyNodeRef<'a>) {
        self.depth -= 1;
    }
}

fn map_node_kind(kind: NodeKind) -> SyntaxKind {
    match kind {
        NodeKind::ModModule | NodeKind::ModExpression => SyntaxKind::Module,
        NodeKind::StmtFunctionDef => SyntaxKind::FunctionDefinition,
        NodeKind::StmtClassDef => SyntaxKind::ClassDefinition,
        NodeKind::StmtReturn => SyntaxKind::ReturnStatement,
        NodeKind::StmtTypeAlias => SyntaxKind::TypeAliasStatement,
        NodeKind::StmtAssign => SyntaxKind::Assignment,
        NodeKind::StmtAnnAssign => SyntaxKind::AnnotatedAssignment,
        NodeKind::StmtIf => SyntaxKind::IfStatement,
        NodeKind::StmtMatch => SyntaxKind::MatchStatement,
        NodeKind::StmtImport => SyntaxKind::ImportStatement,
        NodeKind::StmtImportFrom => SyntaxKind::ImportFromStatement,
        NodeKind::StmtExpr => SyntaxKind::ExpressionStatement,
        NodeKind::StmtDelete
        | NodeKind::StmtAugAssign
        | NodeKind::StmtFor
        | NodeKind::StmtWhile
        | NodeKind::StmtWith
        | NodeKind::StmtRaise
        | NodeKind::StmtTry
        | NodeKind::StmtAssert
        | NodeKind::StmtGlobal
        | NodeKind::StmtNonlocal
        | NodeKind::StmtPass
        | NodeKind::StmtBreak
        | NodeKind::StmtContinue
        | NodeKind::StmtIpyEscapeCommand => SyntaxKind::OtherStatement,
        NodeKind::ExprBoolOp => SyntaxKind::BooleanOperation,
        NodeKind::ExprNamed => SyntaxKind::NamedExpression,
        NodeKind::ExprBinOp => SyntaxKind::BinaryOperation,
        NodeKind::ExprUnaryOp => SyntaxKind::UnaryOperation,
        NodeKind::ExprIf => SyntaxKind::ConditionalExpression,
        NodeKind::ExprCompare => SyntaxKind::Comparison,
        NodeKind::ExprCall => SyntaxKind::Call,
        NodeKind::ExprStringLiteral => SyntaxKind::StringLiteral,
        NodeKind::ExprBytesLiteral => SyntaxKind::BytesLiteral,
        NodeKind::ExprNumberLiteral => SyntaxKind::NumberLiteral,
        NodeKind::ExprBooleanLiteral => SyntaxKind::BooleanLiteral,
        NodeKind::ExprNoneLiteral => SyntaxKind::NoneLiteral,
        NodeKind::ExprEllipsisLiteral => SyntaxKind::EllipsisLiteral,
        NodeKind::ExprAttribute => SyntaxKind::Attribute,
        NodeKind::ExprSubscript => SyntaxKind::Subscript,
        NodeKind::ExprName => SyntaxKind::Name,
        NodeKind::ExprList => SyntaxKind::List,
        NodeKind::ExprTuple => SyntaxKind::Tuple,
        NodeKind::ExprLambda
        | NodeKind::ExprDict
        | NodeKind::ExprSet
        | NodeKind::ExprListComp
        | NodeKind::ExprSetComp
        | NodeKind::ExprDictComp
        | NodeKind::ExprGenerator
        | NodeKind::ExprAwait
        | NodeKind::ExprYield
        | NodeKind::ExprYieldFrom
        | NodeKind::ExprFString
        | NodeKind::ExprTString
        | NodeKind::ExprStarred
        | NodeKind::ExprSlice
        | NodeKind::ExprIpyEscapeCommand => SyntaxKind::OtherExpression,
        NodeKind::PatternMatchValue => SyntaxKind::ValuePattern,
        NodeKind::PatternMatchSingleton => SyntaxKind::SingletonPattern,
        NodeKind::PatternMatchSequence => SyntaxKind::SequencePattern,
        NodeKind::PatternMatchMapping => SyntaxKind::MappingPattern,
        NodeKind::PatternMatchClass => SyntaxKind::ClassPattern,
        NodeKind::PatternMatchStar => SyntaxKind::StarPattern,
        NodeKind::PatternMatchAs => SyntaxKind::AsPattern,
        NodeKind::PatternMatchOr => SyntaxKind::OrPattern,
        NodeKind::Parameter => SyntaxKind::Parameter,
        NodeKind::ParameterWithDefault => SyntaxKind::ParameterWithDefault,
        NodeKind::Arguments => SyntaxKind::Arguments,
        NodeKind::Parameters => SyntaxKind::Parameters,
        NodeKind::Keyword => SyntaxKind::Keyword,
        NodeKind::Alias => SyntaxKind::Alias,
        NodeKind::MatchCase => SyntaxKind::MatchCase,
        NodeKind::Decorator => SyntaxKind::Decorator,
        NodeKind::ElifElseClause => SyntaxKind::ElifElseClause,
        NodeKind::TypeParams => SyntaxKind::TypeParameters,
        NodeKind::Identifier => SyntaxKind::Identifier,
        _ => SyntaxKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteRange, SourceFile, SourceKind, SourceLocation};

    #[test]
    fn source_file_maps_utf8_byte_offsets_to_character_columns() {
        let source = SourceFile::new("unicode.py", "café = 1\nvalue = café\n");

        assert_eq!(
            source.location(0),
            Some(SourceLocation { line: 1, column: 1 })
        );
        assert_eq!(
            source.location(5),
            Some(SourceLocation { line: 1, column: 5 })
        );
        assert_eq!(
            source.location(10),
            Some(SourceLocation { line: 2, column: 1 })
        );
        assert_eq!(source.location(4), None);
        assert_eq!(source.slice(ByteRange { start: 0, end: 5 }), Some("café"));
    }

    #[test]
    fn source_kind_uses_pyi_extension() {
        assert_eq!(
            SourceKind::from_path(Path::new("model.py")),
            SourceKind::Python
        );
        assert_eq!(
            SourceKind::from_path(Path::new("model.pyi")),
            SourceKind::Stub
        );
    }

    use std::path::Path;
}
