use std::fs;
use std::process;

use crate::core::config;

use super::ModuleKind;

pub fn cmd_new(matches: &clap::ArgMatches) {
    if let Some(module_path) = matches.get_one::<String>("test") {
        cmd_new_module(module_path, ModuleKind::Test);
    } else if let Some(module_path) = matches.get_one::<String>("effect") {
        cmd_new_module(module_path, ModuleKind::Effect);
    } else {
        cmd_new_project();
    }
}

fn cmd_new_project() {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        process::exit(1);
    });

    let toml_path = cwd.join(config::CONFIG_FILE);
    if toml_path.exists() {
        eprintln!("error: {} already exists", config::CONFIG_FILE);
        process::exit(1);
    }

    let relux_dir = cwd.join(config::RELUX_DIR);
    let tests_dir = relux_dir.join(config::TESTS_DIR);
    let lib_dir = relux_dir.join(config::LIB_DIR);
    let gitignore_path = relux_dir.join(".gitignore");

    fs::create_dir_all(&tests_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create tests directory: {e}");
        process::exit(1);
    });
    fs::create_dir_all(&lib_dir).unwrap_or_else(|e| {
        eprintln!("error: cannot create lib directory: {e}");
        process::exit(1);
    });

    let toml_content = format!(
        r#"# name = "my-test-suite"

# [shell]
# command = "{command}"
# prompt = "{prompt}"

# [timeout]
# match = "5s"
# test = "5m"
# suite = "10m"

# [run]
# jobs = 1
"#,
        command = config::DEFAULT_SHELL_COMMAND,
        prompt = config::DEFAULT_SHELL_PROMPT,
    );

    fs::write(&toml_path, toml_content).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", config::CONFIG_FILE);
        process::exit(1);
    });

    fs::write(&gitignore_path, "out/\n").unwrap_or_else(|e| {
        eprintln!("error: cannot write .gitignore: {e}");
        process::exit(1);
    });

    eprintln!("Created {}", config::CONFIG_FILE);
    eprintln!("Created {}/{}/", config::RELUX_DIR, config::TESTS_DIR);
    eprintln!("Created {}/{}/", config::RELUX_DIR, config::LIB_DIR);
    eprintln!("Created {}/.gitignore", config::RELUX_DIR);
}

fn validate_module_path(raw: &str) -> Result<String, String> {
    let normalized = raw.strip_suffix(".relux").unwrap_or(raw);
    if normalized.is_empty() {
        return Err("module path cannot be empty".to_string());
    }
    for segment in normalized.split('/') {
        if segment.is_empty() {
            return Err("module path contains empty segment".to_string());
        }
        if !segment.chars().next().unwrap().is_ascii_lowercase() && !segment.starts_with('_') {
            return Err(format!(
                "segment `{segment}` must start with a lowercase letter or underscore"
            ));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!(
                "segment `{segment}` must contain only lowercase letters, digits, and underscores"
            ));
        }
    }
    Ok(normalized.to_string())
}

fn cmd_new_module(raw_path: &str, kind: ModuleKind) {
    let module_path = validate_module_path(raw_path).unwrap_or_else(|e| {
        eprintln!("error: invalid module path: {e}");
        process::exit(1);
    });

    let (project_root, _config) = config::discover_project_root().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let base_dir = match kind {
        ModuleKind::Test => config::tests_dir(&project_root),
        ModuleKind::Effect => config::lib_dir(&project_root),
    };

    let file_path = base_dir.join(&module_path).with_extension("relux");
    if file_path.exists() {
        eprintln!("error: {} already exists", file_path.display());
        process::exit(1);
    }

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: cannot create directory {}: {e}", parent.display());
            process::exit(1);
        });
    }

    let content = match kind {
        ModuleKind::Test => r#"test "hello-relux" {
    shell myshell {
        > echo hello-relux
        <? ^hello-relux$
        match_ok()
    }
}
"#
        .to_string(),
        ModuleKind::Effect => r#"effect HelloEffect -> myshell {
    shell myshell {
    }
}
"#
        .to_string(),
    };

    fs::write(&file_path, content).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", file_path.display());
        process::exit(1);
    });

    let relative = file_path.strip_prefix(&project_root).unwrap_or(&file_path);
    eprintln!("Created {}", relative.display());
}
