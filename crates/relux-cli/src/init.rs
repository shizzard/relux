use std::fs;
use std::process;

use relux_core::config;

const TOML_TEMPLATE: &str = r#"# name = "my-test-suite"

# [shell]
# command = "/bin/sh"
# prompt = "relux> "

# [timeout]
# match = "5s"
# test = "5m"
# suite = "10m"

# [run]
# jobs = 1

# [available_ports]
# range_start = 20000
# range_end = 29999
"#;

pub fn cmd_init() {
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

    fs::write(&toml_path, TOML_TEMPLATE).unwrap_or_else(|e| {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The template must stay parseable both as-is and with every
    /// commented-out setting uncommented.
    #[test]
    fn toml_template_parses_commented_and_uncommented() {
        toml::from_str::<relux_core::config::ReluxConfig>(TOML_TEMPLATE).expect("template as-is");
        let uncommented: String = TOML_TEMPLATE
            .lines()
            .map(|l| l.strip_prefix("# ").unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n");
        let cfg = toml::from_str::<relux_core::config::ReluxConfig>(&uncommented)
            .expect("template uncommented");
        assert_eq!(cfg.available_ports.range_start, Some(20000));
        assert_eq!(cfg.available_ports.range_end, Some(29999));
        cfg.available_ports.validate().unwrap();
    }
}
