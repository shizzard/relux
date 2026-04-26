use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
pub const DEFAULT_SHELL_COMMAND: &str = "/bin/sh";
pub const DEFAULT_SHELL_PROMPT: &str = "relux> ";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const DEFAULT_SUITE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FlakyConfig {
    pub max_retries: u32,
    pub timeout_multiplier: f64,
}

impl Default for FlakyConfig {
    fn default() -> Self {
        Self {
            max_retries: 0,
            timeout_multiplier: 1.5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RunConfig {
    pub jobs: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { jobs: 1 }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReluxConfig {
    pub name: Option<String>,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub timeout: TimeoutConfig,
    #[serde(default)]
    pub flaky: FlakyConfig,
    #[serde(default)]
    pub run: RunConfig,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    #[serde(rename = "match", deserialize_with = "deserialize_duration")]
    pub match_timeout: Duration,
    #[serde(deserialize_with = "deserialize_duration")]
    pub test: Duration,
    #[serde(deserialize_with = "deserialize_duration")]
    pub suite: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            match_timeout: DEFAULT_TIMEOUT,
            test: DEFAULT_TEST_TIMEOUT,
            suite: DEFAULT_SUITE_TIMEOUT,
        }
    }
}

pub fn discover_project_root() -> Result<(PathBuf, ReluxConfig), String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(CONFIG_FILE);
        if candidate.is_file() {
            let mut config = load_config(&candidate)?;
            if config.name.is_none() {
                config.name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
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
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&contents).map_err(|e| format!("invalid {}: {e}", path.display()))
}

pub fn load_manifest(path: &Path) -> Result<(PathBuf, ReluxConfig), String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?;
    let project_root = path
        .parent()
        .ok_or_else(|| format!("manifest path has no parent directory: {}", path.display()))?
        .to_path_buf();
    let mut config = load_config(&path)?;
    if config.name.is_none() {
        config.name = project_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
    }
    Ok((project_root, config))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = ReluxConfig::default();
        assert_eq!(config.shell.command, "/bin/sh");
        assert_eq!(config.shell.prompt, "relux> ");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(5));
        assert_eq!(config.timeout.test, Duration::from_secs(300));
        assert_eq!(config.timeout.suite, Duration::from_secs(600));
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
test = "1m"
suite = "30m"
"#;
        let config: ReluxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name.as_deref(), Some("my-suite"));
        assert_eq!(config.shell.command, "/bin/zsh");
        assert_eq!(config.shell.prompt, "test> ");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(3));
        assert_eq!(config.timeout.test, Duration::from_secs(60));
        assert_eq!(config.timeout.suite, Duration::from_secs(1800));
    }

    #[test]
    fn parse_empty_toml() {
        let config: ReluxConfig = toml::from_str("").unwrap();
        assert_eq!(config.shell.command, "/bin/sh");
        assert_eq!(config.timeout.match_timeout, Duration::from_secs(5));
    }

    #[test]
    fn parse_flaky_defaults() {
        let config: ReluxConfig = toml::from_str("").unwrap();
        assert_eq!(config.flaky.max_retries, 0);
        assert_eq!(config.flaky.timeout_multiplier, 1.5);
    }

    #[test]
    fn parse_flaky_custom() {
        let toml_str = r#"
[flaky]
max_retries = 3
timeout_multiplier = 2.0
"#;
        let config: ReluxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.flaky.max_retries, 3);
        assert_eq!(config.flaky.timeout_multiplier, 2.0);
    }

    #[test]
    fn parse_flaky_partial() {
        let toml_str = r#"
[flaky]
max_retries = 5
"#;
        let config: ReluxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.flaky.max_retries, 5);
        assert_eq!(config.flaky.timeout_multiplier, 1.5);
    }

    #[test]
    fn parse_run_defaults() {
        let config: ReluxConfig = toml::from_str("").unwrap();
        assert_eq!(config.run.jobs, 1);
    }

    #[test]
    fn parse_run_jobs() {
        let config: ReluxConfig = toml::from_str("[run]\njobs = 4\n").unwrap();
        assert_eq!(config.run.jobs, 4);
    }
}
