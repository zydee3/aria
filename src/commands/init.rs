use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Args;

use crate::config::Config;
use crate::index::Index;

const AGENT_MD: &str = r#"# Aria Codebase Index

This project was indexed with [aria](https://github.com/zydee3/aria), a codebase indexer for AI agents.

## Commands

```bash
# Structure Search - Search by function definition and call graph
aria query function <name>      # Get function details (signature, calls, callers)
aria query trace <name>         # Trace call graph (what does this call?)
aria query usages <name>        # Find callers (what calls this?)
aria query file <path>          # Get file overview (types, functions)
aria query list                 # List all functions

# Semantic Search - Search by natural language
aria search "natural language"  # Semantic search (requires: aria embed)
```

## Trace Flags

```bash
aria query trace <name> [OPTIONS]
  -d, --depth <N>  # Trace depth (default: 2, use 0 for unlimited)
  -s, --summaries  # Show function summaries inline
  -c, --callers    # Show caller chain from root to target function
```

Output format uses level numbers and dashes to show call depth:
```
[0] main (./main.c:10-50)
[1] - process (./proc.c:20-80)
[2] -- handler (./handler.c:5-30)
[3] --- [external] [libc:malloc]
```

With `-s`, summaries appear inline:
```
[0] main (./main.c:10-50) : "Entry point for the application"
[1] - process (./proc.c:20-80) : "Processes incoming requests"
```

With `-c`, shows the full call path from root to your target, then forward calls:
```
[0] main (./main.c:10-50)
[1] - caller1 (./a.c:1-10)
[2] -- caller2 (./b.c:1-10)
[3] --- targetFunction (./target.c:5-20)
[4] ---- childCall (./child.c:1-5)
```


## Function Command
```bash
aria query function <name> [OPTIONS]
  -s, --source-only  # Show function source only
```

```bash
vince@vince:~/criu$ aria query function -s write_all
ssize_t write_all(int fd, const void *buf, size_t size)
{
	ssize_t n = 0;
	while (size > 0) {
		ssize_t ret = write(fd, buf, size);
		if (ret == -1) {
			if (errno == EINTR)
				continue;
			/*
			 * The caller should use standard write() for
			 * non-blocking I/O.
			 */
			if (errno == EAGAIN || errno == EWOULDBLOCK)
				errno = EINVAL;
			return ret;
		}
		n += ret;
		buf = (char *)buf + ret;
		size -= ret;
	}
	return n;
}
```

## Examples

```bash
# Find a function by partial name
aria query function HandleRequest

# Trace what main() calls, 3 levels deep
aria query trace main --depth 3

# Trace with full context: callers + callees + summaries
aria query trace myFunction -c -s --depth 3

# Find all callers of a function
aria query usages Validate

# Semantic search
aria search "functions that handle errors"
```
"#;

#[derive(Args)]
pub struct InitArgs {
    /// Exclude .aria/ from git (adds .aria/ to .gitignore)
    #[arg(long)]
    pub local: bool,
}

pub fn run(args: InitArgs) -> ExitCode {
    let aria_dir = Path::new(".aria");

    // Create .aria/ directory if it doesn't exist
    if !aria_dir.exists() {
        if let Err(e) = fs::create_dir(aria_dir) {
            eprintln!("error: failed to create .aria/: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Create .aria/cache/ directory if it doesn't exist
    let cache_dir = aria_dir.join("cache");
    if !cache_dir.exists() {
        if let Err(e) = fs::create_dir(&cache_dir) {
            eprintln!("error: failed to create .aria/cache/: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Write index.json only if it doesn't exist
    let index_path = aria_dir.join("index.json");
    let created_index = !index_path.exists();
    if created_index {
        let index = Index::new();
        let index_json = match serde_json::to_string_pretty(&index) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("error: failed to serialize index: {e}");
                return ExitCode::FAILURE;
            }
        };

        if let Err(e) = fs::write(&index_path, index_json) {
            eprintln!("error: failed to write index.json: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Write config.toml only if it doesn't exist
    let config_path = aria_dir.join("config.toml");
    if !config_path.exists() {
        let config = Config::default();
        let config_toml = match toml::to_string_pretty(&config) {
            Ok(toml) => toml,
            Err(e) => {
                eprintln!("error: failed to serialize config: {e}");
                return ExitCode::FAILURE;
            }
        };

        if let Err(e) = fs::write(&config_path, config_toml) {
            eprintln!("error: failed to write config.toml: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Always write AGENT.md (replace if exists)
    if let Err(e) = fs::write(aria_dir.join("AGENT.md"), AGENT_MD) {
        eprintln!("error: failed to write AGENT.md: {e}");
        return ExitCode::FAILURE;
    }

    // Handle --local flag (add_to_gitignore already checks for duplicates)
    if args.local {
        if let Err(e) = add_to_gitignore(".aria/") {
            eprintln!("error: failed to update .gitignore: {e}");
            return ExitCode::FAILURE;
        }
    }

    println!("Initialized aria in .aria/");

    // Run initial index only if we created a new index.json
    if created_index {
        super::index::run()
    } else {
        ExitCode::SUCCESS
    }
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
