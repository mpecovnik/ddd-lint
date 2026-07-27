use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::source_order::SourceOrderVisitor;
use ruff_python_ast::{
    AnyNodeRef, Expr, ModModule, NodeKind, Operator, PySourceType,
    PythonVersion as RuffPythonVersion, Stmt, UnaryOp,
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
    for statement in &module.body {
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
    }
    ModuleFacts {
        imports,
        classes,
        functions,
        assignments,
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
    for statement in &class.body {
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
    }
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
