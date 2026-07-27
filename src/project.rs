use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::WalkBuilder;

use crate::config::{Config, Pyproject, UvWorkspace, compile_globs, normalize_path};

const DEFAULT_EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    ".tox",
    ".nox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "__pycache__",
    "build",
    "dist",
    "site-packages",
    "node_modules",
];

#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    config_path: Option<PathBuf>,
    config: Config,
    workspace_members: Vec<PathBuf>,
    workspace_member_scopes: Vec<PathBuf>,
    module_roots: Vec<PathBuf>,
    workspace_exclude: Vec<String>,
}

impl Project {
    pub fn resolve(
        current_dir: &Path,
        explicit_config: Option<&Path>,
    ) -> Result<Self, ProjectError> {
        let current_dir = canonicalize(current_dir, "invocation directory")?;
        if let Some(explicit_config) = explicit_config {
            return Self::resolve_explicit(&current_dir, explicit_config);
        }

        let mut nearest_config = None;
        let mut nearest_workspace = None;
        for directory in current_dir.ancestors() {
            let path = directory.join("pyproject.toml");
            if !path.is_file() {
                continue;
            }
            let document = Pyproject::read(&path).map_err(ProjectError::config)?;
            if nearest_config.is_none() && document.config.is_some() {
                nearest_config = Some((path.clone(), document.config.clone()));
            }
            if nearest_workspace.is_none() && document.workspace.is_some() {
                nearest_workspace = Some((path, document));
            }
        }

        if let Some((workspace_path, document)) = nearest_workspace {
            let Some((config_path, _)) = nearest_config else {
                return Err(ProjectError::new(format!(
                    "uv workspace {} must define [tool.ddd-lint] at its root",
                    workspace_path.display()
                )));
            };
            if config_path != workspace_path {
                return Err(ProjectError::new(format!(
                    "member-level [tool.ddd-lint] in {} is unsupported; define policy only in {}",
                    config_path.display(),
                    workspace_path.display()
                )));
            }
            return Self::from_document(workspace_path, document);
        }

        if let Some((config_path, config)) = nearest_config {
            return Self::normal(config_path, config.expect("selected config exists"));
        }

        Ok(Self::default_at(current_dir))
    }

    fn resolve_explicit(current_dir: &Path, explicit_config: &Path) -> Result<Self, ProjectError> {
        let path = if explicit_config.is_absolute() {
            explicit_config.to_owned()
        } else {
            current_dir.join(explicit_config)
        };
        if !path.is_file() {
            return Err(ProjectError::new(format!(
                "configuration file does not exist: {}",
                path.display()
            )));
        }
        let path = canonicalize(&path, "configuration file")?;
        let document = Pyproject::read(&path).map_err(ProjectError::config)?;
        if document.config.is_none() {
            return Err(ProjectError::new(format!(
                "explicit configuration {} does not contain [tool.ddd-lint]",
                path.display()
            )));
        }

        for directory in path
            .parent()
            .expect("configuration file has a parent")
            .ancestors()
        {
            let workspace_path = directory.join("pyproject.toml");
            if !workspace_path.is_file() {
                continue;
            }
            let workspace_document = if workspace_path == path {
                document.clone()
            } else {
                Pyproject::read(&workspace_path).map_err(ProjectError::config)?
            };
            if workspace_document.workspace.is_some() {
                if workspace_path != path {
                    return Err(ProjectError::new(format!(
                        "member-level [tool.ddd-lint] in {} is unsupported; define policy only in {}",
                        path.display(),
                        workspace_path.display()
                    )));
                }
                return Self::from_document(path, workspace_document);
            }
        }

        Self::normal(path, document.config.expect("explicit config exists"))
    }

    fn default_at(root: PathBuf) -> Self {
        let config = Config::default();
        let module_roots = config.src.iter().map(|src| root.join(src)).collect();
        Self {
            root,
            config_path: None,
            config,
            workspace_members: Vec::new(),
            workspace_member_scopes: Vec::new(),
            module_roots,
            workspace_exclude: Vec::new(),
        }
    }

    fn normal(config_path: PathBuf, config: Config) -> Result<Self, ProjectError> {
        let root = config_path
            .parent()
            .expect("configuration file has a parent")
            .to_owned();
        let module_roots = config.src.iter().map(|src| root.join(src)).collect();
        Ok(Self {
            root,
            config_path: Some(config_path),
            config,
            workspace_members: Vec::new(),
            workspace_member_scopes: Vec::new(),
            module_roots,
            workspace_exclude: Vec::new(),
        })
    }

    fn from_document(config_path: PathBuf, document: Pyproject) -> Result<Self, ProjectError> {
        let root = config_path
            .parent()
            .expect("configuration file has a parent")
            .to_owned();
        let config = document.config.ok_or_else(|| {
            ProjectError::new(format!(
                "uv workspace {} must define [tool.ddd-lint] at its root",
                config_path.display()
            ))
        })?;
        let workspace = document.workspace.expect("workspace document has metadata");
        let workspace_members = expand_workspace_members(&root, &workspace)?;
        let workspace_member_scopes = workspace_member_scopes(&root, &workspace.members);
        for member in &workspace_members {
            let member_config = member.join("pyproject.toml");
            if !member_config.is_file() {
                return Err(ProjectError::new(format!(
                    "uv workspace member {} does not contain pyproject.toml",
                    member.display()
                )));
            }
            let member_document = Pyproject::read(&member_config).map_err(ProjectError::config)?;
            if member_document.config.is_some() {
                return Err(ProjectError::new(format!(
                    "member-level [tool.ddd-lint] in {} is unsupported; define policy only in {}",
                    member_config.display(),
                    config_path.display()
                )));
            }
        }

        let mut module_roots = BTreeSet::new();
        for project_root in std::iter::once(&root).chain(&workspace_members) {
            module_roots.insert(project_root.to_owned());
            module_roots.insert(project_root.join("src"));
        }

        Ok(Self {
            root,
            config_path: Some(config_path),
            config,
            workspace_members,
            workspace_member_scopes,
            module_roots: module_roots.into_iter().collect(),
            workspace_exclude: workspace.exclude,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    #[cfg(test)]
    fn workspace_members(&self) -> &[PathBuf] {
        &self.workspace_members
    }

    pub(crate) fn module_roots(&self) -> &[PathBuf] {
        &self.module_roots
    }

    pub fn discover_files(
        &self,
        current_dir: &Path,
        input_paths: &[PathBuf],
    ) -> Result<DiscoveredFiles, ProjectError> {
        let exclude = compile_globs(&self.config.exclude).map_err(ProjectError::config)?;
        let workspace_exclude =
            compile_globs(&self.workspace_exclude).map_err(ProjectError::config)?;
        let root = self.root.clone();
        let filter_root = root.clone();
        let filter_exclude = exclude.clone();
        let filter_workspace_exclude = workspace_exclude.clone();
        let mut builder = WalkBuilder::new(&root);
        builder
            .hidden(false)
            .parents(true)
            .ignore(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(move |entry| {
                entry.depth() == 0
                    || !is_excluded(
                        entry.path(),
                        &filter_root,
                        &filter_exclude,
                        &filter_workspace_exclude,
                    )
            });

        let mut supporting = Vec::new();
        for entry in builder.build() {
            let entry = entry.map_err(|error| {
                ProjectError::new(format!("failed to discover Python files: {error}"))
            })?;
            let path = entry.path();
            if !entry.file_type().is_some_and(|kind| kind.is_file())
                || !is_python_source(path)
                || !self.includes_source_path(path)
                || is_excluded(path, &self.root, &exclude, &workspace_exclude)
            {
                continue;
            }
            supporting.push(path.to_owned());
        }
        supporting.sort();
        supporting.dedup();

        let scopes = if input_paths.is_empty() {
            vec![canonicalize(current_dir, "input path")?]
        } else {
            input_paths
                .iter()
                .map(|path| {
                    let path = if path.is_absolute() {
                        path.clone()
                    } else {
                        current_dir.join(path)
                    };
                    canonicalize(&path, "input path")
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let diagnostics = supporting
            .iter()
            .filter(|path| {
                scopes
                    .iter()
                    .any(|scope| path == &scope || path.starts_with(scope))
            })
            .cloned()
            .collect();

        Ok(DiscoveredFiles {
            supporting,
            diagnostics,
        })
    }

    fn includes_source_path(&self, path: &Path) -> bool {
        if self.workspace_member_scopes.is_empty() {
            return self.module_roots.iter().any(|root| path.starts_with(root));
        }
        if path.starts_with(self.root.join("src")) {
            return true;
        }
        if self
            .workspace_members
            .iter()
            .any(|member| path.starts_with(member))
        {
            return true;
        }
        !self
            .workspace_member_scopes
            .iter()
            .any(|scope| path.starts_with(scope))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFiles {
    pub supporting: Vec<PathBuf>,
    pub diagnostics: Vec<PathBuf>,
}

fn expand_workspace_members(
    root: &Path,
    workspace: &UvWorkspace,
) -> Result<Vec<PathBuf>, ProjectError> {
    let members = compile_globs(&workspace.members).map_err(ProjectError::config)?;
    let excludes = compile_globs(&workspace.exclude).map_err(ProjectError::config)?;
    let filter_root = root.to_owned();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            entry.depth() == 0 || !has_default_excluded_component(entry.path(), &filter_root)
        });

    let mut expanded = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|error| {
            ProjectError::new(format!("failed to expand uv workspace members: {error}"))
        })?;
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        let relative = relative_path(entry.path(), root);
        if members.is_match(&relative)
            && !matches_path_or_ancestor(&excludes, &relative)
            && entry.path().join("pyproject.toml").is_file()
        {
            expanded.push(entry.path().to_owned());
        }
    }
    expanded.sort();
    expanded.dedup();
    Ok(expanded)
}

fn workspace_member_scopes(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut scopes = patterns
        .iter()
        .map(|pattern| {
            let mut literal_prefix = PathBuf::new();
            let mut found_wildcard = false;
            for component in pattern.replace('\\', "/").split('/') {
                if component
                    .chars()
                    .any(|character| matches!(character, '*' | '?' | '[' | '{'))
                {
                    found_wildcard = true;
                    break;
                }
                literal_prefix.push(component);
            }
            if !found_wildcard {
                literal_prefix.pop();
            }
            root.join(literal_prefix)
        })
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    scopes.retain(|scope| scope != root);
    scopes
}

fn is_excluded(path: &Path, root: &Path, exclude: &GlobSet, workspace_exclude: &GlobSet) -> bool {
    if has_default_excluded_component(path, root) {
        return true;
    }
    let relative = relative_path(path, root);
    matches_path_or_ancestor(exclude, &relative)
        || matches_path_or_ancestor(workspace_exclude, &relative)
}

fn matches_path_or_ancestor(patterns: &GlobSet, path: &str) -> bool {
    if patterns.is_match(path) {
        return true;
    }
    let mut prefix = String::new();
    for component in path.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        if patterns.is_match(&prefix) {
            return true;
        }
    }
    false
}

fn has_default_excluded_component(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative.components().any(|component| {
            DEFAULT_EXCLUDED_DIRECTORIES
                .iter()
                .any(|excluded| component.as_os_str() == *excluded)
        })
    })
}

fn relative_path(path: &Path, root: &Path) -> String {
    normalize_path(path.strip_prefix(root).unwrap_or(path))
}

fn is_python_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "py" | "pyi"))
}

fn canonicalize(path: &Path, kind: &str) -> Result<PathBuf, ProjectError> {
    fs::canonicalize(path).map_err(|error| {
        ProjectError::new(format!(
            "failed to resolve {kind} {}: {error}",
            path.display()
        ))
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectError(String);

impl ProjectError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn config(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl Display for ProjectError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ProjectError {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::Project;

    fn write(path: &Path, source: &str) {
        fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("fixture directory should exist");
        fs::write(path, source).expect("fixture should write");
    }

    fn names(paths: &[PathBuf], root: &Path) -> Vec<String> {
        paths
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .expect("fixture path should be below root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn canonical(path: &Path) -> PathBuf {
        fs::canonicalize(path).expect("fixture path should canonicalize")
    }

    #[test]
    fn no_config_uses_invocation_directory_and_zero_rule_defaults() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        write(&root.join("module.py"), "value = 1\n");

        let project = Project::resolve(&root, None).expect("defaults should resolve");

        assert_eq!(project.root(), root);
        assert_eq!(project.config_path(), None);
        assert!(project.config().enabled_rules.is_empty());
    }

    #[test]
    fn nearest_ancestor_config_wins_and_explicit_config_overrides_it() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        let nested = root.join("packages/member/deep");
        fs::create_dir_all(&nested).expect("fixture directory");
        write(
            &root.join("pyproject.toml"),
            "[tool.ddd-lint]\nselect = [\"DM001\"]\n",
        );
        write(
            &root.join("packages/member/pyproject.toml"),
            "[tool.ddd-lint]\nselect = [\"DI001\"]\n",
        );

        let nearest = Project::resolve(&nested, None).expect("nearest config should resolve");
        assert_eq!(nearest.root(), root.join("packages/member"));
        assert_eq!(nearest.config().enabled_rules[0].as_str(), "DI001");

        let explicit = Project::resolve(&nested, Some(&root.join("pyproject.toml")))
            .expect("explicit config should resolve");
        assert_eq!(explicit.root(), root);
        assert_eq!(explicit.config().enabled_rules[0].as_str(), "DM001");
    }

    #[test]
    fn flat_and_src_layouts_discover_expected_files() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        let flat = root.join("flat");
        write(&flat.join("pyproject.toml"), "[tool.ddd-lint]\n");
        write(&flat.join("domain.py"), "value = 1\n");
        let flat_project = Project::resolve(&flat, None).expect("flat project");
        let files = flat_project.discover_files(&flat, &[]).expect("flat files");
        assert_eq!(names(&files.supporting, &flat), ["domain.py"]);

        let src = root.join("src-layout");
        write(
            &src.join("pyproject.toml"),
            "[tool.ddd-lint]\nsrc = [\"src\"]\n",
        );
        write(&src.join("outside.py"), "value = 1\n");
        write(&src.join("src/domain/model.py"), "value = 1\n");
        let src_project = Project::resolve(&src, None).expect("src project");
        let files = src_project.discover_files(&src, &[]).expect("src files");
        assert_eq!(names(&files.supporting, &src), ["src/domain/model.py"]);
    }

    #[test]
    fn respects_default_git_and_custom_excludes_for_explicit_paths() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        write(
            &root.join("pyproject.toml"),
            "[tool.ddd-lint]\nexclude = [\"generated/**\"]\n",
        );
        write(&root.join(".gitignore"), "ignored.py\n");
        write(&root.join(".ignore"), "private.py\n");
        write(&root.join(".git/info/exclude"), "git-info.py\n");
        write(&root.join("kept.py"), "value = 1\n");
        write(&root.join("ignored.py"), "value = 1\n");
        write(&root.join("private.py"), "value = 1\n");
        write(&root.join("git-info.py"), "value = 1\n");
        write(&root.join("generated/model.py"), "value = 1\n");
        write(&root.join(".venv/model.py"), "value = 1\n");
        let project = Project::resolve(&root, None).expect("project should resolve");

        let all = project
            .discover_files(&root, &[])
            .expect("files should resolve");
        assert_eq!(names(&all.supporting, &root), ["kept.py"]);
        for explicit in [
            "ignored.py",
            "private.py",
            "git-info.py",
            "generated/model.py",
            ".venv/model.py",
        ] {
            let files = project
                .discover_files(&root, &[PathBuf::from(explicit)])
                .expect("explicit ignored path should not fail");
            assert!(files.diagnostics.is_empty(), "{explicit}");
        }
    }

    #[test]
    fn uv_workspace_expands_members_roots_and_supporting_files() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        write(
            &root.join("pyproject.toml"),
            "[tool.ddd-lint]\nselect = []\n\n[tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/skipped\"]\n",
        );
        write(&root.join("root.py"), "value = 1\n");
        write(&root.join("tools/helper.py"), "value = 1\n");
        write(&root.join("packages/not-a-member/model.py"), "value = 1\n");
        for member in ["alpha", "beta", "skipped"] {
            write(
                &root.join(format!("packages/{member}/pyproject.toml")),
                &format!("[project]\nname = \"{member}\"\nversion = \"0.1.0\"\n"),
            );
            write(
                &root.join(format!("packages/{member}/src/{member}/model.py")),
                "value = 1\n",
            );
        }

        let project =
            Project::resolve(&root.join("packages/alpha"), None).expect("workspace should resolve");
        assert_eq!(
            names(project.workspace_members(), &root),
            ["packages/alpha", "packages/beta"]
        );
        assert_eq!(
            names(project.module_roots(), &root),
            [
                "",
                "packages/alpha",
                "packages/alpha/src",
                "packages/beta",
                "packages/beta/src",
                "src"
            ]
        );

        let files = project
            .discover_files(&root.join("packages/alpha"), &[PathBuf::from("src")])
            .expect("workspace files should resolve");
        assert_eq!(
            names(&files.supporting, &root),
            [
                "packages/alpha/src/alpha/model.py",
                "packages/beta/src/beta/model.py",
                "root.py",
                "tools/helper.py"
            ]
        );
        assert_eq!(
            names(&files.diagnostics, &root),
            ["packages/alpha/src/alpha/model.py"]
        );
    }

    #[test]
    fn broad_workspace_globs_only_admit_project_directories() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        write(
            &root.join("pyproject.toml"),
            "[tool.ddd-lint]\n\n[tool.uv.workspace]\nmembers = [\"packages/**\"]\n",
        );
        write(
            &root.join("packages/apps/api/pyproject.toml"),
            "[project]\nname = \"api\"\nversion = \"0.1.0\"\n",
        );
        write(
            &root.join("packages/apps/api/src/api/model.py"),
            "value = 1\n",
        );
        write(&root.join("packages/apps/intermediate.py"), "value = 1\n");

        let project = Project::resolve(&root.join("packages/apps/api"), None)
            .expect("nested workspace member should resolve");
        assert_eq!(
            names(project.workspace_members(), &root),
            ["packages/apps/api"]
        );
        let files = project
            .discover_files(&root, &[])
            .expect("workspace files should resolve");
        assert_eq!(
            names(&files.supporting, &root),
            ["packages/apps/api/src/api/model.py"]
        );
    }

    #[test]
    fn exact_and_root_wildcard_members_preserve_root_sources() {
        for (members, member) in [
            ("[\"packages/api\"]", "packages/api"),
            ("[\"member-*\"]", "member-api"),
        ] {
            let directory = TempDir::new().expect("temporary directory");
            let root = canonical(directory.path());
            write(
                &root.join("pyproject.toml"),
                &format!("[tool.ddd-lint]\n\n[tool.uv.workspace]\nmembers = {members}\n"),
            );
            write(
                &root.join(member).join("pyproject.toml"),
                "[project]\nname = \"api\"\nversion = \"0.1.0\"\n",
            );
            write(&root.join(member).join("model.py"), "value = 1\n");
            write(&root.join("src/root/model.py"), "value = 1\n");
            write(&root.join("root.py"), "value = 1\n");
            let non_member = if member.contains('/') {
                "packages/other/model.py"
            } else {
                "other/model.py"
            };
            write(&root.join(non_member), "value = 1\n");

            let project = Project::resolve(&root.join(member), None)
                .expect("workspace member should resolve");
            let files = project
                .discover_files(&root, &[])
                .expect("workspace files should resolve");
            assert_eq!(
                names(&files.supporting, &root),
                if member.contains('/') {
                    vec![
                        format!("{member}/model.py"),
                        "root.py".to_owned(),
                        "src/root/model.py".to_owned(),
                    ]
                } else {
                    vec![
                        format!("{member}/model.py"),
                        "other/model.py".to_owned(),
                        "root.py".to_owned(),
                        "src/root/model.py".to_owned(),
                    ]
                },
                "{members}"
            );
        }
    }

    #[test]
    fn rejects_workspace_member_policy() {
        let directory = TempDir::new().expect("temporary directory");
        let root = canonical(directory.path());
        write(
            &root.join("pyproject.toml"),
            "[tool.ddd-lint]\n\n[tool.uv.workspace]\nmembers = [\"member\"]\n",
        );
        write(
            &root.join("member/pyproject.toml"),
            "[tool.ddd-lint]\nselect = [\"DM001\"]\n",
        );

        let error = Project::resolve(&root.join("member"), None)
            .expect_err("member policy should fail")
            .to_string();

        assert!(error.contains("member-level [tool.ddd-lint]"), "{error}");
        assert!(error.contains("member/pyproject.toml"), "{error}");
    }
}
