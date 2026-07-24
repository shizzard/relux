//! `.env` discovery and layered substitution.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use relux_core::config::DOTENV_FILE;
use relux_core::pure::Env;
use relux_core::pure::LayeredEnv;
use relux_core::pure::LayeredEnvBuilder;
use relux_core::pure::LayeredEnvSource;
use relux_ir::IrNode;
use relux_ir::Plan;
use relux_ir::Suite;
use thiserror::Error;

use crate::dotenv::DotenvParseError;
use crate::dotenv::parse_env;

/// `.env` candidate paths from `project_root` down to `test_file`'s directory,
/// shallowest (root) first. Pure: does not touch the filesystem, does not
/// filter by existence. Directories outside `project_root` are not walked.
pub fn dotenv_candidate_paths(project_root: &Path, test_file: &Path) -> Vec<PathBuf> {
    let start = test_file.parent().unwrap_or(test_file);
    let mut dirs: Vec<&Path> = Vec::new();
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if !dir.starts_with(project_root) {
            break;
        }
        dirs.push(dir);
        if dir == project_root {
            break;
        }
        cur = dir.parent();
    }
    dirs.reverse();
    dirs.into_iter().map(|d| d.join(DOTENV_FILE)).collect()
}

/// A reserved (`__RELUX_`-prefixed) key seen in a `.env`. The value is inert
/// (the `ReluxInternal` layer outranks `.env`); the warning surfaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedKeyWarning {
    pub path: PathBuf,
    pub key: String,
}

/// The resolved `.env` stack for a test: the layered env plus any warnings.
#[derive(Debug, Clone)]
pub struct StackResult {
    pub env: Arc<LayeredEnv>,
    pub warnings: Vec<ReservedKeyWarning>,
}

/// Failure resolving a `.env` stack.
#[derive(Debug, Error)]
pub enum DotenvError {
    /// A `.env` file could not be read or parsed; carries its path.
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: DotenvParseError,
    },
}

/// Fold parsed `.env` files (root-first) over `base` into a `DotEnv`-layered
/// env. Each deeper file resolves `${VAR}` against `base` plus the resolved
/// values of shallower files. Pure: takes file contents, does no I/O.
pub fn layer_stack(
    base: Arc<LayeredEnv>,
    files: Vec<(PathBuf, String)>,
) -> Result<StackResult, DotenvError> {
    let mut current = base;
    let mut warnings = Vec::new();
    for (path, content) in files {
        let mut builder =
            LayeredEnvBuilder::new(current.clone(), LayeredEnvSource::DotEnv(path.clone()));
        let pairs =
            parse_env(content.as_bytes(), &mut builder).map_err(|source| DotenvError::Parse {
                path: path.clone(),
                source,
            })?;
        for (key, _) in &pairs {
            if key.starts_with("__RELUX_") {
                warnings.push(ReservedKeyWarning {
                    path: path.clone(),
                    key: key.clone(),
                });
            }
        }
        current = Arc::new(builder.build());
    }
    Ok(StackResult {
        env: current,
        warnings,
    })
}

/// Discover and resolve the `.env` stack for `test_file` under `project_root`,
/// folding it over `base`. Missing `.env` files are skipped; a present but
/// unreadable or non-UTF-8 file is an error.
pub fn resolve_dotenv_stack(
    base: Arc<LayeredEnv>,
    project_root: &Path,
    test_file: &Path,
) -> Result<StackResult, DotenvError> {
    let mut files = Vec::new();
    for path in dotenv_candidate_paths(project_root, test_file) {
        match std::fs::read_to_string(&path) {
            Ok(content) => files.push((path, content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(DotenvError::Parse {
                    path,
                    source: DotenvParseError::Io(e),
                });
            }
        }
    }
    layer_stack(base, files)
}

/// Capture the process environment as the immutable `Base` layer. The single
/// place the process env is snapshotted, so `check`/`run`/`dump` agree on it.
/// Warns on `__RELUX_`-prefixed keys in the process env (relux owns that
/// namespace; such values are shadowed by the `ReluxInternal` layer).
pub fn capture_base() -> Arc<LayeredEnv> {
    let base = Env::capture();
    for (k, _) in base.iter() {
        if k.starts_with("__RELUX_") {
            eprintln!("warning: reserved key {k} in the process environment is ignored");
        }
    }
    Arc::new(LayeredEnv::root_with_source(base, LayeredEnvSource::Base))
}

/// Resolve and attach each runnable test's `.env` stack (`Base -> DotEnv...`)
/// onto its plan, using `suite.env` as the base and the plan's source file path
/// for discovery. Reserved-key warnings are printed. Returns the malformed-`.env`
/// errors (if any) so the caller can report them and abort; on success every
/// runnable plan's `env` is the folded stack.
pub fn attach_dotenv(suite: &mut Suite, project_root: &Path) -> Result<(), Vec<DotenvError>> {
    let base = suite.env.clone();
    let sources = &suite.tables.sources;
    let mut errors = Vec::new();
    for plan in suite.plans.iter_mut() {
        if let Plan::Runnable { meta, env, .. } = plan {
            let file_id = meta.span().file();
            let path = sources
                .get(file_id)
                .map(|sf| sf.path.clone())
                .unwrap_or_else(|| file_id.path().clone());
            match resolve_dotenv_stack(base.clone(), project_root, &path) {
                Ok(stack) => {
                    for w in &stack.warnings {
                        eprintln!(
                            "warning: {}: reserved key {} in .env is ignored",
                            w.path.display(),
                            w.key
                        );
                    }
                    *env = stack.env;
                }
                Err(e) => errors.push(e),
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use relux_core::pure::Env;

    use super::*;

    #[test]
    fn candidates_root_first_from_root_to_test_dir() {
        let root = Path::new("/proj");
        let test = Path::new("/proj/relux/tests/db/login.relux");
        assert_eq!(
            dotenv_candidate_paths(root, test),
            vec![
                PathBuf::from("/proj/.env"),
                PathBuf::from("/proj/relux/.env"),
                PathBuf::from("/proj/relux/tests/.env"),
                PathBuf::from("/proj/relux/tests/db/.env"),
            ]
        );
    }

    #[test]
    fn candidates_single_when_test_at_root() {
        let root = Path::new("/proj");
        let test = Path::new("/proj/smoke.relux");
        assert_eq!(
            dotenv_candidate_paths(root, test),
            vec![PathBuf::from("/proj/.env")]
        );
    }

    #[test]
    fn candidates_stops_at_project_root() {
        // nothing above /proj is walked even though the test path is deep
        let root = Path::new("/proj");
        let test = Path::new("/proj/a/b.relux");
        let got = dotenv_candidate_paths(root, test);
        assert!(got.iter().all(|p| p.starts_with("/proj")));
        assert_eq!(got.first(), Some(&PathBuf::from("/proj/.env")));
    }

    fn base(pairs: &[(&str, &str)]) -> Arc<LayeredEnv> {
        let mut env = Env::new();
        for (k, v) in pairs {
            env.insert((*k).into(), (*v).into());
        }
        Arc::new(LayeredEnv::root(env))
    }

    #[test]
    fn layer_stack_empty_returns_base() {
        let b = base(&[("X", "1")]);
        let out = layer_stack(b.clone(), vec![]).unwrap();
        assert_eq!(out.env.stack_hash(), b.stack_hash());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn layer_stack_deeper_references_shallower() {
        let files = vec![
            (PathBuf::from("/proj/.env"), "ROOT=/proj\n".to_string()),
            (
                PathBuf::from("/proj/db/.env"),
                "DB_DIR=${ROOT}/db\n".to_string(),
            ),
        ];
        let out = layer_stack(base(&[]), files).unwrap();
        assert_eq!(out.env.get("ROOT"), Some("/proj"));
        assert_eq!(out.env.get("DB_DIR"), Some("/proj/db")); // cross-layer subst
    }

    #[test]
    fn layer_stack_deeper_layer_wins_on_precedence() {
        let files = vec![
            (PathBuf::from("/proj/.env"), "PORT=5432\n".to_string()),
            (PathBuf::from("/proj/db/.env"), "PORT=5433\n".to_string()),
        ];
        let out = layer_stack(base(&[]), files).unwrap();
        assert_eq!(out.env.get("PORT"), Some("5433")); // deepest wins
        assert_eq!(
            out.env.source(),
            &LayeredEnvSource::DotEnv("/proj/db/.env".into())
        );
    }

    #[test]
    fn layer_stack_references_base() {
        let files = vec![(PathBuf::from("/proj/.env"), "URL=${HOST}/x\n".to_string())];
        let out = layer_stack(base(&[("HOST", "localhost")]), files).unwrap();
        assert_eq!(out.env.get("URL"), Some("localhost/x"));
    }

    #[test]
    fn layer_stack_warns_on_reserved_key() {
        let files = vec![(
            PathBuf::from("/proj/.env"),
            "__RELUX_RUN_ID=nope\nOK=1\n".to_string(),
        )];
        let out = layer_stack(base(&[]), files).unwrap();
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(out.warnings[0].key, "__RELUX_RUN_ID");
        assert_eq!(out.warnings[0].path, PathBuf::from("/proj/.env"));
    }

    #[test]
    fn layer_stack_malformed_file_errors_with_path() {
        let files = vec![(PathBuf::from("/proj/.env"), "bogus line\n".to_string())];
        let err = layer_stack(base(&[]), files).unwrap_err();
        match err {
            DotenvError::Parse { path, .. } => assert_eq!(path, PathBuf::from("/proj/.env")),
        }
    }
}
