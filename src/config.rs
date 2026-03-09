use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use walkdir::WalkDir;

pub const DEFAULT_SHELL_COMMAND: &str = "/bin/sh";
pub const DEFAULT_SHELL_PROMPT: &str = "relux> ";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

pub const RELUX_DIR: &str = "relux";
pub const TESTS_DIR: &str = "tests";
pub const LIB_DIR: &str = "lib";
pub const OUT_DIR: &str = "out";
pub const CONFIG_FILE: &str = "Relux.toml";

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    humantime::parse_duration(&s).map_err(serde::de::Error::custom)
}

fn deserialize_optional_duration<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(s) => humantime::parse_duration(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReluxConfig {
    pub name: Option<String>,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub timeout: TimeoutConfig,
}

#[derive(Debug, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_shell_command")]
    pub command: String,
    #[serde(default = "default_shell_prompt")]
    pub prompt: String,
}

fn default_shell_command() -> String {
    DEFAULT_SHELL_COMMAND.to_string()
}

fn default_shell_prompt() -> String {
    DEFAULT_SHELL_PROMPT.to_string()
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            command: default_shell_command(),
            prompt: default_shell_prompt(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    #[serde(rename = "match", deserialize_with = "deserialize_duration")]
    pub match_timeout: Duration,
    #[serde(deserialize_with = "deserialize_optional_duration")]
    pub case: Option<Duration>,
    #[serde(deserialize_with = "deserialize_optional_duration")]
    pub suite: Option<Duration>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            match_timeout: DEFAULT_TIMEOUT,
            case: None,
            suite: None,
        }
    }
}

impl Default for ReluxConfig {
    fn default() -> Self {
        Self {
            name: None,
            shell: ShellConfig::default(),
            timeout: TimeoutConfig::default(),
        }
    }
}

pub fn discover_project_root() -> Result<(PathBuf, ReluxConfig), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(CONFIG_FILE);
        if candidate.is_file() {
            let mut config = load_config(&candidate)?;
            if config.name.is_none() {
                config.name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
            }
            return Ok((dir.to_path_buf(), config));
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => {
                return Err(format!(
                    "no {} found in {} or any parent directory",
                    CONFIG_FILE,
                    cwd.display()
                ));
            }
        }
    }
}

pub fn load_config(path: &Path) -> Result<ReluxConfig, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&contents).map_err(|e| format!("invalid {}: {e}", path.display()))
}

pub fn tests_dir(project_root: &Path) -> PathBuf {
    project_root.join(RELUX_DIR).join(TESTS_DIR)
}

pub fn lib_dir(project_root: &Path) -> PathBuf {
    project_root.join(RELUX_DIR).join(LIB_DIR)
}

pub fn out_dir(project_root: &Path) -> PathBuf {
    project_root.join(RELUX_DIR).join(OUT_DIR)
}

pub fn discover_relux_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            // Always include the root directory itself
            if e.path() == dir {
                return true;
            }
            // Skip subdirectories that contain a Relux.toml (nested suites)
            if e.file_type().is_dir() && e.path().join(CONFIG_FILE).exists() {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "relux"))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
}

pub fn resolve_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(discover_relux_files(path));
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    files
}

pub fn apply_multiplier(config: &mut ReluxConfig, multiplier: f64) {
    config.timeout.match_timeout =
        Duration::from_secs_f64(config.timeout.match_timeout.as_secs_f64() * multiplier);
    if let Some(case) = &mut config.timeout.case {
        *case = Duration::from_secs_f64(case.as_secs_f64() * multiplier);
    }
    if let Some(suite) = &mut config.timeout.suite {
        *suite = Duration::from_secs_f64(suite.as_secs_f64() * multiplier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = ReluxConfig::default();
        assert_eq!(config.shell.command, "/bin/sh");
        assert_eq!(config.shell.prompt, "relux> ");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(5));
        assert!(config.timeout.case.is_none());
        assert!(config.timeout.suite.is_none());
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"name = "test-suite""#;
        let config: ReluxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name.as_deref(), Some("test-suite"));
        assert_eq!(config.shell.command, "/bin/sh");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(5));
    }

    #[test]
    fn parse_full_toml() {
        let toml_str = r#"
name = "my-suite"

[shell]
command = "/bin/zsh"
prompt = "test> "

[timeout]
match = "3s"
case = "1m"
suite = "30m"
"#;
        let config: ReluxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name.as_deref(), Some("my-suite"));
        assert_eq!(config.shell.command, "/bin/zsh");
        assert_eq!(config.shell.prompt, "test> ");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(3));
        assert_eq!(config.timeout.case, Some(Duration::from_secs(60)));
        assert_eq!(config.timeout.suite, Some(Duration::from_secs(1800)));
    }

    #[test]
    fn parse_empty_toml() {
        let config: ReluxConfig = toml::from_str("").unwrap();
        assert_eq!(config.shell.command, "/bin/sh");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(5));
    }

    #[test]
    fn multiplier_scales_timeouts() {
        let mut config = ReluxConfig::default();
        config.timeout.case = Some(Duration::from_secs(60));
        apply_multiplier(&mut config, 2.0);
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(10));
        assert_eq!(config.timeout.case, Some(Duration::from_secs(120)));
    }
}
