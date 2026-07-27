use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::python::{
    PythonVersion, SourceKind, SourceLocation, SyntaxErrorKind, SyntaxKind, parse_module,
};

const V1_SURFACE: &str = include_str!("../tests/fixtures/python/v1_surface.py");
const VALID_STUB: &str = include_str!("../tests/fixtures/python/valid.pyi");
const INVALID_PYTHON: &str = include_str!("../tests/fixtures/python/invalid.py");
const INVALID_STUB: &str = include_str!("../tests/fixtures/python/invalid.pyi");
const PYTHON_312: &str = include_str!("../tests/fixtures/python/python312.py");

#[test]
fn parses_v1_syntax_surface_and_comments() {
    let module = parse_module("v1_surface.py", V1_SURFACE, PythonVersion::Py310)
        .expect("v1 syntax fixture should parse");

    assert_eq!(module.source_kind(), SourceKind::Python);
    assert_eq!(module.target_version(), PythonVersion::Py310);
    assert_eq!(
        module
            .comments()
            .iter()
            .map(|comment| comment.text.as_str())
            .collect::<Vec<_>>(),
        ["# intentional nullable field"]
    );

    let kinds = module
        .nodes()
        .iter()
        .map(|node| node.kind)
        .collect::<HashSet<_>>();
    for expected in [
        SyntaxKind::Module,
        SyntaxKind::ImportFromStatement,
        SyntaxKind::ClassDefinition,
        SyntaxKind::FunctionDefinition,
        SyntaxKind::Assignment,
        SyntaxKind::AnnotatedAssignment,
        SyntaxKind::IfStatement,
        SyntaxKind::MatchStatement,
        SyntaxKind::Call,
        SyntaxKind::BinaryOperation,
        SyntaxKind::Attribute,
        SyntaxKind::Subscript,
        SyntaxKind::Name,
        SyntaxKind::NoneLiteral,
        SyntaxKind::ClassPattern,
        SyntaxKind::AsPattern,
        SyntaxKind::OrPattern,
        SyntaxKind::SingletonPattern,
        SyntaxKind::ValuePattern,
        SyntaxKind::SequencePattern,
        SyntaxKind::StarPattern,
        SyntaxKind::MappingPattern,
        SyntaxKind::Parameter,
        SyntaxKind::ParameterWithDefault,
        SyntaxKind::MatchCase,
        SyntaxKind::Decorator,
    ] {
        assert!(
            kinds.contains(&expected),
            "missing syntax kind: {expected:?}"
        );
    }
}

#[test]
fn maps_unicode_and_multiline_ranges() {
    let source = "café = 1\nmodel: (\n    Draft\n    | Published\n)\n";
    let module =
        parse_module("ranges.py", source, PythonVersion::Py310).expect("source should parse");

    let unicode_name = module
        .nodes()
        .iter()
        .find(|node| {
            node.kind == SyntaxKind::Name && module.source().slice(node.range) == Some("café")
        })
        .expect("Unicode name should exist");
    assert_eq!(unicode_name.range.start(), 0);
    assert_eq!(unicode_name.range.end(), 5);
    assert_eq!(
        module
            .source()
            .source_range(unicode_name.range)
            .expect("valid range")
            .end,
        SourceLocation { line: 1, column: 5 }
    );

    let annotation = module
        .nodes()
        .iter()
        .find(|node| node.kind == SyntaxKind::AnnotatedAssignment)
        .expect("annotated assignment should exist");
    let range = module
        .source()
        .source_range(annotation.range)
        .expect("valid range");
    assert_eq!(range.start, SourceLocation { line: 2, column: 1 });
    assert_eq!(range.end, SourceLocation { line: 5, column: 2 });
}

#[test]
fn reports_stable_python_and_stub_errors() {
    for (path, source, expected_kind, expected_range, expected_detail, expected_display) in [
        (
            "invalid.py",
            INVALID_PYTHON,
            SourceKind::Python,
            (236, 237),
            "Expected a parameter or the end of the parameter list",
            "invalid.py:5:12: invalid Python syntax (and 1 more errors)",
        ),
        (
            "invalid.pyi",
            INVALID_STUB,
            SourceKind::Stub,
            (248, 249),
            "Expected `,`, found `:`",
            "invalid.pyi:6:10: invalid Python syntax (and 1 more errors)",
        ),
    ] {
        let errors = parse_module(path, source, PythonVersion::Py310)
            .expect_err("invalid fixture should fail");

        assert_eq!(errors.source().source_kind(), expected_kind);
        assert_eq!(errors.errors()[0].kind, SyntaxErrorKind::InvalidSyntax);
        assert_eq!(errors.errors()[0].message, "invalid Python syntax");
        assert_eq!(errors.errors()[0].detail, expected_detail);
        assert_eq!(
            (
                errors.errors()[0].range.start(),
                errors.errors()[0].range.end()
            ),
            expected_range
        );
        assert_eq!(errors.to_string(), expected_display);
    }
}

#[test]
fn preserves_source_order_and_nesting_depth() {
    let source =
        "class Service:\n    def run(self):\n        if ready:\n            return execute()\n";
    let module =
        parse_module("depth.py", source, PythonVersion::Py310).expect("source should parse");
    let observed = module
        .nodes()
        .iter()
        .filter_map(|node| {
            matches!(
                node.kind,
                SyntaxKind::ClassDefinition
                    | SyntaxKind::FunctionDefinition
                    | SyntaxKind::IfStatement
                    | SyntaxKind::ReturnStatement
                    | SyntaxKind::Call
            )
            .then_some((node.kind, node.depth))
        })
        .collect::<Vec<_>>();

    assert_eq!(
        observed,
        [
            (SyntaxKind::ClassDefinition, 1),
            (SyntaxKind::FunctionDefinition, 2),
            (SyntaxKind::IfStatement, 3),
            (SyntaxKind::ReturnStatement, 4),
            (SyntaxKind::Call, 5),
        ]
    );
    assert!(
        module
            .nodes()
            .windows(2)
            .all(|nodes| nodes[0].range.start() <= nodes[1].range.start())
    );
}

#[test]
fn enforces_target_version_and_accepts_supported_versions() {
    let errors = parse_module("python312.py", PYTHON_312, PythonVersion::Py310)
        .expect_err("type statement should require Python 3.12");
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| error.kind == SyntaxErrorKind::UnsupportedSyntax)
    );

    parse_module("python312.py", PYTHON_312, PythonVersion::Py312)
        .expect("type statement should parse on Python 3.12");

    for version in [
        PythonVersion::Py310,
        PythonVersion::Py311,
        PythonVersion::Py312,
        PythonVersion::Py313,
        PythonVersion::Py314,
        PythonVersion::Py315,
    ] {
        parse_module("valid.py", "value: int = 1\n", version)
            .unwrap_or_else(|error| panic!("Python {version} should parse: {error}"));
    }
}

#[test]
fn recognizes_version_specific_syntax_through_python315() {
    parse_module(
        "python310.py",
        "match value:\n    case 1:\n        pass\n",
        PythonVersion::Py310,
    )
    .expect("match should parse on Python 3.10");

    for (path, source, previous, supported) in [
        (
            "python311.py",
            "try:\n    pass\nexcept* ValueError:\n    pass\n",
            PythonVersion::Py310,
            PythonVersion::Py311,
        ),
        (
            "python312.py",
            "type Alias = int\n",
            PythonVersion::Py311,
            PythonVersion::Py312,
        ),
        (
            "python313.py",
            "type Alias[T = int] = T\n",
            PythonVersion::Py312,
            PythonVersion::Py313,
        ),
        (
            "python314.py",
            "template = t\"value: {value}\"\n",
            PythonVersion::Py313,
            PythonVersion::Py314,
        ),
        (
            "python315.py",
            "lazy import os\n",
            PythonVersion::Py314,
            PythonVersion::Py315,
        ),
    ] {
        let errors = parse_module(path, source, previous)
            .expect_err("new syntax should fail on the previous Python version");
        assert!(
            errors
                .errors()
                .iter()
                .any(|error| error.kind == SyntaxErrorKind::UnsupportedSyntax),
            "{path} should report unsupported syntax on Python {previous}"
        );
        parse_module(path, source, supported)
            .unwrap_or_else(|error| panic!("{path} should parse on Python {supported}: {error}"));
    }
}

#[test]
fn indexes_valid_stub_syntax() {
    let module = parse_module("valid.pyi", VALID_STUB, PythonVersion::Py310)
        .expect("valid stub should parse");

    assert_eq!(module.source_kind(), SourceKind::Stub);
    assert!(
        module
            .nodes()
            .iter()
            .any(|node| node.kind == SyntaxKind::FunctionDefinition)
    );
}

#[test]
fn applies_versions_and_extracts_comments_for_stubs() {
    let source = "type Alias = int  # stub alias\n";
    let errors = parse_module("alias.pyi", source, PythonVersion::Py310)
        .expect_err("type alias should require Python 3.12 in stubs");
    assert_eq!(errors.errors()[0].kind, SyntaxErrorKind::UnsupportedSyntax);
    assert_eq!(
        errors.errors()[0].message,
        "syntax is not supported on Python 3.10"
    );

    let module = parse_module("alias.pyi", source, PythonVersion::Py312)
        .expect("type alias should parse on Python 3.12");
    assert_eq!(module.comments()[0].text, "# stub alias");
}

#[test]
fn ruff_types_are_confined_to_adapter_module() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    assert_no_ruff_imports(&source_root);
}

fn assert_no_ruff_imports(directory: &std::path::Path) {
    let python_crate_prefix = ["ruff", "_python_"].concat();
    let text_size_crate = ["ruff", "_text_size"].concat();
    for entry in fs::read_dir(directory).expect("source directory should exist") {
        let path = entry.expect("source entry should be readable").path();
        if path.is_dir() {
            assert_no_ruff_imports(&path);
            continue;
        }
        if path.file_name().is_some_and(|name| name == "python.rs")
            || path.extension().is_none_or(|extension| extension != "rs")
        {
            continue;
        }

        let source = fs::read_to_string(&path).expect("Rust source should be readable");
        assert!(
            !source.contains(&python_crate_prefix) && !source.contains(&text_size_crate),
            "Ruff dependency escaped adapter boundary: {}",
            path.display()
        );
    }
}
