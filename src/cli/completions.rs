use std::fs;
use std::path::PathBuf;
use std::process;

fn detect_shell() -> &'static str {
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("fish") {
            return "fish";
        }
        if shell.contains("zsh") {
            return "zsh";
        }
    }
    "bash"
}

fn completion_script(shell: &str) -> String {
    match shell {
        "bash" => "source <(COMPLETE=bash relux)".to_string(),
        "zsh" => "source <(COMPLETE=zsh relux)".to_string(),
        "fish" => "COMPLETE=fish relux | source".to_string(),
        _ => unreachable!(),
    }
}

fn default_install_path(shell: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    match shell {
        "bash" => Some(PathBuf::from(&home).join(".local/share/bash-completion/completions/relux")),
        "fish" => Some(PathBuf::from(&home).join(".config/fish/completions/relux.fish")),
        _ => None,
    }
}

pub fn cmd_completions(matches: &clap::ArgMatches) {
    let shell = matches
        .get_one::<String>("shell")
        .map(|s| s.as_str())
        .unwrap_or_else(|| detect_shell());

    let install = matches.get_flag("install");
    let path_override = matches.get_one::<PathBuf>("path");

    let script = completion_script(shell);
    let target = path_override
        .cloned()
        .or_else(|| default_install_path(shell));

    if install {
        let Some(path) = target else {
            eprintln!("error: no default install path for {shell}; specify one with --path <DIR>");
            process::exit(1);
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("error: cannot create {}: {e}", parent.display());
                process::exit(1);
            });
        }

        fs::write(&path, format!("{script}\n")).unwrap_or_else(|e| {
            eprintln!("error: cannot write {}: {e}", path.display());
            process::exit(1);
        });

        eprintln!("Installed {shell} completions to {}", path.display());
    } else {
        match target {
            Some(path) => {
                eprintln!("Would write to: {}", path.display());
                eprintln!("Contents:\n  {script}");
                eprintln!();
                eprintln!("Run `relux completions --shell {shell} --install` to install.");
            }
            None => {
                eprintln!("Add the following to your shell configuration:");
                eprintln!("  {script}");
                eprintln!();
                eprintln!(
                    "Or run `relux completions --shell {shell} --install --path <DIR>` \
                     to write to a specific directory (e.g. a directory in your fpath)."
                );
            }
        }
    }
}
