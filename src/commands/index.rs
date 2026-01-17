use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use walkdir::WalkDir;

use crate::config::Config;
use crate::index::Index;
use crate::parser::{GoParser, RustParser};
use crate::resolver::Resolver;
use crate::summarizer::{Summarizer, SummaryRequest};
use crate::topo;

pub fn run() -> ExitCode {
    let aria_dir = Path::new(".aria");

    if !aria_dir.exists() {
        eprintln!("error: not initialized (run `aria init` first)");
        return ExitCode::FAILURE;
    }

    // Load config
    let config = load_config(aria_dir);

    // Load existing index to preserve summaries/embeddings for unchanged functions
    let old_index = load_existing_index(aria_dir);

    let mut index = Index::new();
    let mut go_parser = GoParser::new();
    let mut rust_parser = RustParser::new();
    let mut file_count = 0;
    let mut func_count = 0;
    let mut type_count = 0;

    // Store sources for body extraction during summarization
    let mut sources: HashMap<String, String> = HashMap::new();

    // Walk current directory for source files
    for entry in WalkDir::new(".")
        .into_iter()
        .filter_entry(|e| !is_hidden(e) && !is_ignored(e))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());

        // Determine language from extension
        let lang = match ext {
            Some("go") => "go",
            Some("rs") => "rust",
            _ => continue,
        };

        // Skip test files
        let path_str = path.to_string_lossy();
        if lang == "go" && path_str.ends_with("_test.go") {
            continue;
        }

        // Read and parse file
        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: failed to read {}: {}", path_str, e);
                continue;
            }
        };

        let parsed = match lang {
            "go" => go_parser.parse_file(&source, &path_str),
            "rust" => rust_parser.parse_file(&source, &path_str),
            _ => None,
        };

        match parsed {
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

    println!(
        "Parsed {} files: {} functions, {} types",
        file_count, func_count, type_count
    );

    // Resolve call targets and populate called_by
    let mut resolver = Resolver::new();
    resolver.build_symbol_table(&index.files);
    resolver.resolve(&mut index);

    // Preserve summaries from old index for unchanged functions (by ast_hash)
    let preserved = preserve_summaries(&mut index, &old_index);
    if preserved > 0 {
        println!("Preserved {} existing summaries", preserved);
    }

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
    let summarizer = Summarizer::new(config.llm.batch_size, config.llm.parallel, config.debug);

    // Build the call graph: qualified_name -> set of callee qualified_names
    let topo_start = Instant::now();
    let mut all_functions: HashSet<String> = HashSet::new();
    let mut calls_map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut total_funcs = 0;

    for entry in index.files.values() {
        for func in &entry.functions {
            total_funcs += 1;
            if config.debug && all_functions.contains(&func.qualified_name) {
                eprintln!("warning: duplicate qualified_name: {}", func.qualified_name);
            }
            all_functions.insert(func.qualified_name.clone());

            let callees: HashSet<String> = func
                .calls
                .iter()
                .filter(|c| c.target != "[unresolved]")
                .map(|c| c.target.clone())
                .collect();

            if !callees.is_empty() {
                calls_map.insert(func.qualified_name.clone(), callees);
            }
        }
    }

    // Compute levels (topological sort)
    let levels = topo::assign_levels(&all_functions, &calls_map);
    let level_groups = topo::group_by_level(&levels);
    let topo_elapsed = topo_start.elapsed();

    let funcs_with_calls = calls_map.len();
    let total_in_levels: usize = level_groups.iter().map(|g| g.len()).sum();
    let duplicates = total_funcs - all_functions.len();
    println!(
        "Computed topology in {:.2?} ({} functions, {} duplicates, {} with resolved calls, {} in levels)",
        topo_elapsed, all_functions.len(), duplicates, funcs_with_calls, total_in_levels
    );

    // Build lookup: qualified_name -> (path, func_idx)
    let mut func_locations: HashMap<String, (String, usize)> = HashMap::new();
    for (path, entry) in index.files.iter() {
        for (func_idx, func) in entry.functions.iter().enumerate() {
            func_locations.insert(func.qualified_name.clone(), (path.clone(), func_idx));
        }
    }

    // Track summaries as we generate them (for callee context)
    let mut summaries: HashMap<String, String> = HashMap::new();

    // Pre-populate with existing summaries
    for entry in index.files.values() {
        for func in &entry.functions {
            if let Some(summary) = &func.summary {
                summaries.insert(func.qualified_name.clone(), summary.clone());
            }
        }
    }

    // Count functions to summarize (excluding those with existing summaries)
    let total: usize = level_groups
        .iter()
        .flat_map(|g| g.iter())
        .filter(|qn| !summaries.contains_key(*qn))
        .count();

    if total == 0 {
        return;
    }

    let num_levels = level_groups.len();
    println!(
        "Generating summaries for {} functions in {} levels (batch={}, parallel={})...",
        total, num_levels, config.llm.batch_size, config.llm.parallel
    );

    let mut summary_count = 0;
    let mut error_count = 0;
    let summarization_start = Instant::now();

    // Process each level sequentially (so callee summaries are available for higher levels)
    for (level, funcs_at_level) in level_groups.iter().enumerate() {
        let level_start = Instant::now();

        if config.debug {
            eprintln!(
                "debug: Starting level {} with {} functions, {} summaries available",
                level,
                funcs_at_level.len(),
                summaries.len()
            );
        }

        // Collect requests for this level
        let mut requests: Vec<SummaryRequest> = Vec::new();
        let mut request_qnames: Vec<String> = Vec::new();

        for qualified_name in funcs_at_level {
            // Skip if already has summary
            if summaries.contains_key(qualified_name) {
                continue;
            }

            let Some((path, func_idx)) = func_locations.get(qualified_name) else {
                continue;
            };
            let Some(source) = sources.get(path) else {
                continue;
            };
            let Some(entry) = index.files.get(path) else {
                continue;
            };
            let Some(func) = entry.functions.get(*func_idx) else {
                continue;
            };

            let lines: Vec<&str> = source.lines().collect();
            let body = extract_body(&lines, func.line_start, func.line_end);
            if body.is_empty() {
                continue;
            }

            // Build callee context from already-computed summaries
            let resolved_calls: Vec<_> = func
                .calls
                .iter()
                .filter(|c| c.target != "[unresolved]")
                .collect();

            let callee_context: Vec<(String, String)> = resolved_calls
                .iter()
                .filter_map(|c| {
                    summaries.get(&c.target).map(|s| {
                        // Use simple name for readability
                        let simple_name = c.target.rsplit('.').next().unwrap_or(&c.target);
                        (simple_name.to_string(), s.clone())
                    })
                })
                .collect();

            // Debug: show missed lookups and successful ones
            if config.debug && !resolved_calls.is_empty() {
                if callee_context.is_empty() {
                    let missed: Vec<_> = resolved_calls.iter().map(|c| &c.target).collect();
                    eprintln!(
                        "debug [level {}]: {} has {} resolved calls but 0 found in summaries: {:?}",
                        level,
                        qualified_name,
                        resolved_calls.len(),
                        missed
                    );
                } else {
                    eprintln!(
                        "debug [level {}]: {} has {} callee summaries as context",
                        level,
                        qualified_name,
                        callee_context.len()
                    );
                }
            }

            let id = requests.len();
            requests.push(SummaryRequest {
                id,
                signature: func.signature.clone(),
                body,
                callee_context,
            });
            request_qnames.push(qualified_name.clone());
        }

        if requests.is_empty() {
            continue;
        }

        let funcs_in_level = requests.len();
        let with_context = requests.iter().filter(|r| !r.callee_context.is_empty()).count();

        // Process this level
        let results = summarizer.summarize_batch(requests);

        for result in results {
            let qualified_name = &request_qnames[result.id];

            match result.summary {
                Ok(summary) => {
                    // Store in our map for use as context in later levels
                    summaries.insert(qualified_name.clone(), summary.clone());

                    // Update the index
                    if let Some((path, func_idx)) = func_locations.get(qualified_name) {
                        if let Some(entry) = index.files.get_mut(path) {
                            if let Some(func) = entry.functions.get_mut(*func_idx) {
                                func.summary = Some(summary);
                                summary_count += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("warning: failed to summarize {}: {}", qualified_name, e);
                    error_count += 1;
                }
            }
        }

        let level_elapsed = level_start.elapsed();
        // Clear the progress line and print level summary
        eprint!("\r");
        println!(
            "  Level {}: {} functions ({} with callee context) in {:.2?}",
            level, funcs_in_level, with_context, level_elapsed
        );
    }

    let total_elapsed = summarization_start.elapsed();
    println!(
        "Generated {} summaries ({} errors) in {:.2?}",
        summary_count, error_count, total_elapsed
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

fn load_existing_index(aria_dir: &Path) -> Option<Index> {
    let index_path = aria_dir.join("index.json");
    fs::read_to_string(index_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

/// Preserve summaries from old index for functions with matching ast_hash.
/// Returns the number of summaries preserved.
fn preserve_summaries(index: &mut Index, old_index: &Option<Index>) -> usize {
    let Some(old) = old_index else {
        return 0;
    };

    // Build lookup: ast_hash -> summary for old functions
    let mut old_summaries: HashMap<String, String> = HashMap::new();
    for entry in old.files.values() {
        for func in &entry.functions {
            if let Some(summary) = &func.summary {
                if !func.ast_hash.is_empty() {
                    old_summaries.insert(func.ast_hash.clone(), summary.clone());
                }
            }
        }
    }

    if old_summaries.is_empty() {
        return 0;
    }

    // Apply to new index
    let mut preserved = 0;
    for entry in index.files.values_mut() {
        for func in &mut entry.functions {
            if func.summary.is_none() && !func.ast_hash.is_empty() {
                if let Some(summary) = old_summaries.get(&func.ast_hash) {
                    func.summary = Some(summary.clone());
                    preserved += 1;
                }
            }
        }
    }

    preserved
}
