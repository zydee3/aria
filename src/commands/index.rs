use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use walkdir::WalkDir;

use crate::config::Config;
use crate::index::Index;
use crate::parser::GoParser;
use crate::resolver::Resolver;
use crate::summarizer::{Summarizer, SummaryRequest};

pub fn run() -> ExitCode {
    let aria_dir = Path::new(".aria");

    if !aria_dir.exists() {
        eprintln!("error: not initialized (run `aria init` first)");
        return ExitCode::FAILURE;
    }

    // Load config
    let config = load_config(aria_dir);

    let mut index = Index::new();
    let mut parser = GoParser::new();
    let mut file_count = 0;
    let mut func_count = 0;
    let mut type_count = 0;

    // Store sources for body extraction during summarization
    let mut sources: HashMap<String, String> = HashMap::new();

    // Walk current directory for Go files
    for entry in WalkDir::new(".")
        .into_iter()
        .filter_entry(|e| !is_hidden(e) && !is_ignored(e))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip non-Go files
        if path.extension().is_none_or(|ext| ext != "go") {
            continue;
        }

        // Skip test files for now
        if path.to_string_lossy().ends_with("_test.go") {
            continue;
        }

        let path_str = path.to_string_lossy();

        // Read and parse file
        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: failed to read {}: {}", path_str, e);
                continue;
            }
        };

        match parser.parse_file(&source, &path_str) {
            Some(entry) => {
                func_count += entry.functions.len();
                type_count += entry.types.len();
                file_count += 1;
                // Store source for summarization
                if config.features.summaries {
                    sources.insert(path_str.to_string(), source);
                }
                index.files.insert(path_str.to_string(), entry);
            }
            None => {
                eprintln!("warning: failed to parse {}", path_str);
            }
        }
    }

    // Resolve call targets and populate called_by
    let mut resolver = Resolver::new();
    resolver.build_symbol_table(&index.files);
    resolver.resolve(&mut index);

    // Generate summaries if enabled
    if config.features.summaries {
        run_summarization(&config, &mut index, &sources);
    }

    // Count resolved vs unresolved calls for stats
    // Traverse: files -> functions -> calls
    let mut resolved_count = 0;
    let mut unresolved_count = 0;
    for entry in index.files.values() {
        for func in &entry.functions {
            for call in &func.calls {
                if call.target == "[unresolved]" {
                    unresolved_count += 1;
                } else {
                    resolved_count += 1;
                }
            }
        }
    }

    // Get current git commit if available
    index.commit = get_git_head().unwrap_or_default();

    // Write index
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

    let total_calls = resolved_count + unresolved_count;
    let resolution_pct = if total_calls > 0 {
        (resolved_count as f64 / total_calls as f64) * 100.0
    } else {
        100.0
    };

    println!(
        "Indexed {} files: {} functions, {} types, {} calls ({:.0}% resolved)",
        file_count, func_count, type_count, total_calls, resolution_pct
    );

    ExitCode::SUCCESS
}

fn load_config(aria_dir: &Path) -> Config {
    let config_path = aria_dir.join("config.toml");
    if let Ok(content) = fs::read_to_string(&config_path) {
        toml::from_str(&content).unwrap_or_default()
    } else {
        Config::default()
    }
}

fn run_summarization(config: &Config, index: &mut Index, sources: &HashMap<String, String>) {
    let summarizer = Summarizer::new(config.llm.batch_size, config.llm.parallel);

    // Collect functions that need summarization with their location info
    let mut requests: Vec<SummaryRequest> = Vec::new();
    let mut locations: Vec<(String, usize)> = Vec::new(); // (path, func_idx)

    for (path, entry) in index.files.iter() {
        let Some(source) = sources.get(path) else {
            continue;
        };
        let lines: Vec<&str> = source.lines().collect();

        for (func_idx, func) in entry.functions.iter().enumerate() {
            // Skip if already has summary
            if func.summary.is_some() {
                continue;
            }

            // Extract function body from source using line numbers
            let body = extract_body(&lines, func.line_start, func.line_end);
            if body.is_empty() {
                continue;
            }

            let id = requests.len();
            requests.push(SummaryRequest {
                id,
                signature: func.signature.clone(),
                body,
            });
            locations.push((path.clone(), func_idx));
        }
    }

    let total = requests.len();
    if total == 0 {
        return;
    }

    println!(
        "Generating summaries for {} functions (batch={}, parallel={})...",
        total, config.llm.batch_size, config.llm.parallel
    );

    // Process all summaries with batching and parallelism
    let results = summarizer.summarize_batch(requests);

    let mut summary_count = 0;
    let mut error_count = 0;

    for result in results {
        let (path, func_idx) = &locations[result.id];

        match result.summary {
            Ok(summary) => {
                if let Some(entry) = index.files.get_mut(path) {
                    if let Some(func) = entry.functions.get_mut(*func_idx) {
                        func.summary = Some(summary);
                        summary_count += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: failed to summarize: {}", e);
                error_count += 1;
            }
        }
    }

    println!(
        "Generated {} summaries ({} errors)",
        summary_count, error_count
    );
}

fn extract_body(lines: &[&str], line_start: u32, line_end: u32) -> String {
    let start = (line_start as usize).saturating_sub(1);
    let end = (line_end as usize).min(lines.len());

    if start >= end || start >= lines.len() {
        return String::new();
    }

    lines[start..end].join("\n")
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s != "." && s.starts_with('.'))
}

fn is_ignored(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    matches!(name.as_ref(), "vendor" | "node_modules" | "target")
}

fn get_git_head() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}
