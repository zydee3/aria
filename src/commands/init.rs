use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Args;

use crate::config::Config;
use crate::index::Index;

#[derive(Args)]
pub struct InitArgs {
    /// Exclude .aria/ from git (adds .aria/ to .gitignore)
    #[arg(long)]
    pub local: bool,
}

pub fn run(args: InitArgs) -> ExitCode {
    let aria_dir = Path::new(".aria");

    if aria_dir.exists() {
        eprintln!("error: .aria/ already exists");
        return ExitCode::FAILURE;
    }

    // Create .aria/ directory
    if let Err(e) = fs::create_dir(aria_dir) {
        eprintln!("error: failed to create .aria/: {e}");
        return ExitCode::FAILURE;
    }

    // Create .aria/cache/ directory
    if let Err(e) = fs::create_dir(aria_dir.join("cache")) {
        eprintln!("error: failed to create .aria/cache/: {e}");
        return ExitCode::FAILURE;
    }

    // Write index.json
    let index = Index::new();
    let index_json = match serde_json::to_string_pretty(&index) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("error: failed to serialize index: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::write(aria_dir.join("index.json"), index_json) {
        eprintln!("error: failed to write index.json: {e}");
        return ExitCode::FAILURE;
    }

    // Write config.toml
    let config = Config::default();
    let config_toml = match toml::to_string_pretty(&config) {
        Ok(toml) => toml,
        Err(e) => {
            eprintln!("error: failed to serialize config: {e}");
            return ExitCode::FAILURE;
        }
    };
    
    if let Err(e) = fs::write(aria_dir.join("config.toml"), config_toml) {
        eprintln!("error: failed to write config.toml: {e}");
        return ExitCode::FAILURE;
    }

    // Handle --local flag
    if args.local {
        if let Err(e) = add_to_gitignore(".aria/") {
            eprintln!("error: failed to update .gitignore: {e}");
            return ExitCode::FAILURE;
        }
    }

    println!("Initialized aria in .aria/");
    if args.local {
        println!("Added .aria/ to .gitignore");
    }

    // Run initial index
    super::index::run()
}

fn add_to_gitignore(entry: &str) -> std::io::Result<()> {
    let gitignore_path = Path::new(".gitignore");

    // Check if .gitignore exists and if entry is already present
    if gitignore_path.exists() {
        let contents = fs::read_to_string(gitignore_path)?;
        for line in contents.lines() {
            if line.trim() == entry {
                return Ok(()); // Already present
            }
        }

        // Append to existing file
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(gitignore_path)?;

        // Add newline if file doesn't end with one
        if !contents.ends_with('\n') {
            writeln!(file)?;
        }

        writeln!(file, "{entry}")?;
    } else {
        // Create new .gitignore
        fs::write(gitignore_path, format!("{entry}\n"))?;
    }

    Ok(())
}
