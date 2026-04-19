use std::fs;
use std::process;

use relux_core::config;

use super::ModuleKind;

const TEST_TEMPLATE: &str = r#"test "hello relux" {
    shell myshell {
        > echo hello-relux
        <? ^hello-relux$
        match_ok()
    }
}
"#;

const EFFECT_TEMPLATE: &str = r#"effect InnerEffect {
    expect INNER_EFFECT_VAR

    expose service

    shell service {
        // commands go here
    }
}

effect HelloEffect {
    expect HELLO_EFFECT_VAR

    let test = "test"

    start InnerEffect as inner {
        INNER_EFFECT_VAR = test
    }

    expose service as hello_service
    expose inner.service as inner_service

    shell service {
        // commands go here
    }
}
"#;

const LIB_TEMPLATE: &str = r#"fn greet(name) {
    let line = greeting(name)
    > echo ${line}
}

pure fn greeting(name) {
    "hello, ${name}"
}
"#;

pub fn cmd_new(matches: &clap::ArgMatches) {
    if let Some(module_path) = matches.get_one::<String>("test") {
        cmd_new_module(module_path, ModuleKind::Test);
    } else if let Some(module_path) = matches.get_one::<String>("effect") {
        cmd_new_module(module_path, ModuleKind::Effect);
    } else if let Some(module_path) = matches.get_one::<String>("lib") {
        cmd_new_module(module_path, ModuleKind::Lib);
    } else {
        unreachable!()
    }
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
        ModuleKind::Effect | ModuleKind::Lib => config::lib_dir(&project_root),
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
        ModuleKind::Test => TEST_TEMPLATE,
        ModuleKind::Effect => EFFECT_TEMPLATE,
        ModuleKind::Lib => LIB_TEMPLATE,
    };

    fs::write(&file_path, content).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", file_path.display());
        process::exit(1);
    });

    let relative = file_path.strip_prefix(&project_root).unwrap_or(&file_path);
    eprintln!("Created {}", relative.display());
}
