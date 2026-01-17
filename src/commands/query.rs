use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Subcommand;

use crate::externals::ExternalDb;
use crate::index::{Function, Index};

#[derive(Subcommand)]
pub enum QueryCommand {
    /// Get function details
    Function {
        /// Qualified function name (or partial match)
        name: String,
    },

    /// Trace call graph (forward edge: what does this function call)
    Trace {
        /// Qualified function name
        name: String,
        /// Trace depth (0 = unlimited, default: 2)
        #[arg(long, short = 'd', default_value = "2")]
        depth: usize,
        /// Show summaries for each function call
        #[arg(long, short = 's')]
        summaries: bool,
        /// Show caller chain up to root before forward trace
        #[arg(long, short = 'c')]
        callers: bool,
    },

    /// Find all usages of a symbol (backward edge: what calls this function)
    Usages {
        /// Symbol name
        name: String,
        /// Maximum depth for caller chain (0 = unlimited, default: 1)
        #[arg(long, short = 'd', default_value = "1")]
        depth: usize,
        /// Show summaries for each function call
        #[arg(long, short = 's')]
        summaries: bool,
    },

    /// Get file overview
    File {
        /// File path
        path: String,
    },

    /// List all functions in a file
    List {
        /// File path (optional - lists all if not defined)
        path: Option<String>,
    },
}

pub fn run(cmd: QueryCommand) -> ExitCode {
    let index = match load_index() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cmd {
        QueryCommand::Function { name } => query_function(&index, &name),
        QueryCommand::Trace { name, depth, summaries, callers } => {
            let max_depth = if depth == 0 { usize::MAX } else { depth };
            query_trace(&index, &name, max_depth, summaries, callers)
        }
        QueryCommand::Usages { name, depth, summaries } => {
            let max_depth = if depth == 0 { usize::MAX } else { depth };
            query_usages(&index, &name, max_depth, summaries)
        }
        QueryCommand::File { path } => query_file(&index, &path),
        QueryCommand::List { path } => query_list(&index, path.as_deref()),
    }
}

fn load_index() -> Result<Index, String> {
    let index_path = Path::new(".aria/index.json");
    if !index_path.exists() {
        return Err("index not found (run `aria index` first)".to_string());
    }

    let content = fs::read_to_string(index_path)
        .map_err(|e| format!("failed to read index: {e}"))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse index: {e}"))
}

/// Build a lookup table: qualified_name -> (file_path, &Function)
fn build_function_map<'a>(index: &'a Index) -> HashMap<&'a str, (&'a str, &'a Function)> {
    let mut map = HashMap::new();
    for (file_path, entry) in &index.files {
        for func in &entry.functions {
            map.insert(func.qualified_name.as_str(), (file_path.as_str(), func));
        }
    }
    map
}

/// Find functions matching a name (exact or partial)
fn find_functions<'a>(index: &'a Index, name: &str) -> Vec<(&'a str, &'a Function)> {
    let mut matches = Vec::new();

    for (file_path, entry) in &index.files {
        for func in &entry.functions {
            // Exact match on qualified name
            if func.qualified_name == name {
                matches.push((file_path.as_str(), func));
            }
            // Exact match on simple name
            else if func.name == name {
                matches.push((file_path.as_str(), func));
            }
            // Partial match (contains)
            else if func.qualified_name.contains(name) {
                matches.push((file_path.as_str(), func));
            }
        }
    }

    matches
}

fn query_function(index: &Index, name: &str) -> ExitCode {
    let matches = find_functions(index, name);

    if matches.is_empty() {
        eprintln!("No function found matching '{name}'");
        return ExitCode::FAILURE;
    }

    for (file_path, func) in &matches {
        println!("{} ({}:{}-{})", func.qualified_name, file_path, func.line_start, func.line_end);
        println!("  {}", func.signature);

        if let Some(summary) = &func.summary {
            println!("  {summary}");
        }

        if !func.calls.is_empty() {
            println!("  calls:");
            for call in &func.calls {
                println!("    {} (line {})", call.target, call.line);
            }
        }

        if !func.called_by.is_empty() {
            println!("  called_by:");
            for caller in &func.called_by {
                println!("    {caller}");
            }
        }

        println!();
    }

    ExitCode::SUCCESS
}

fn query_trace(index: &Index, name: &str, max_depth: usize, show_summaries: bool, show_callers: bool) -> ExitCode {
    let func_map = build_function_map(index);
    let matches = find_functions(index, name);

    if matches.is_empty() {
        eprintln!("No function found matching '{name}'");
        return ExitCode::FAILURE;
    }

    // Use first match
    let (file_path, func) = matches[0];

    if matches.len() > 1 {
        eprintln!("Multiple matches, showing first: {}", func.qualified_name);
        eprintln!();
    }

    // Track seen externals so we only show summary on first occurrence
    let mut seen_externals = HashSet::new();
    let external_db = if show_summaries { Some(ExternalDb::new()) } else { None };

    // If showing callers, build and print the caller chain first
    if show_callers {
        let caller_chain = build_caller_chain(&func_map, func);
        let target_level = caller_chain.len();

        if !caller_chain.is_empty() {
            // Print callers from root down to parent of target
            for (i, (caller_file, caller_func)) in caller_chain.iter().enumerate() {
                print_level_node(i, caller_file, caller_func, show_summaries);
            }
        } else {
            println!("[root]");
        }

        // Print target function
        print_level_node(target_level, file_path, func, show_summaries);

        // Print forward edges
        let mut visited = HashSet::new();
        visited.insert(func.qualified_name.as_str());
        print_trace_level(&func_map, index, func, target_level + 1, max_depth, 1, &mut visited, show_summaries, &mut seen_externals, &external_db);
    } else {
        // Original behavior - just print forward trace with levels
        print_level_node(0, file_path, func, show_summaries);
        let mut visited = HashSet::new();
        visited.insert(func.qualified_name.as_str());
        print_trace_level(&func_map, index, func, 1, max_depth, 1, &mut visited, show_summaries, &mut seen_externals, &external_db);
    }

    ExitCode::SUCCESS
}

/// Build caller chain from root to the target function's parent
fn build_caller_chain<'a>(
    func_map: &HashMap<&'a str, (&'a str, &'a Function)>,
    target: &'a Function,
) -> Vec<(&'a str, &'a Function)> {
    if target.called_by.is_empty() {
        return Vec::new();
    }

    let mut chain = Vec::new();
    let mut current = target;
    let mut visited = HashSet::new();
    visited.insert(target.qualified_name.as_str());

    while !current.called_by.is_empty() {
        let caller_name = current.called_by.iter()
            .find(|name| !visited.contains(name.as_str()));

        match caller_name {
            Some(name) => {
                if let Some((file, caller_func)) = func_map.get(name.as_str()) {
                    visited.insert(name.as_str());
                    chain.push((*file, *caller_func));
                    current = caller_func;
                } else {
                    break;
                }
            }
            None => break,
        }
    }

    chain.reverse();
    chain
}

/// Print a caller chain node: [N] --- name (file:line)
fn print_level_node(level: usize, file_path: &str, func: &Function, show_summary: bool) {
    let dashes = "-".repeat(level);
    let summary_part = if show_summary {
        func.summary.as_ref().map(|s| format!(" : \"{}\"", s)).unwrap_or_default()
    } else {
        String::new()
    };
    if level == 0 {
        println!("[{}] {} ({}:{}-{}){}",
            level,
            func.qualified_name,
            file_path,
            func.line_start,
            func.line_end,
            summary_part
        );
    } else {
        println!("[{}] {} {} ({}:{}-{}){}",
            level,
            dashes,
            func.qualified_name,
            file_path,
            func.line_start,
            func.line_end,
            summary_part
        );
    }
}


/// Print trace children using level-based format
fn print_trace_level<'a>(
    func_map: &HashMap<&'a str, (&'a str, &'a Function)>,
    index: &'a Index,
    func: &'a Function,
    level: usize,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<&'a str>,
    show_summaries: bool,
    seen_externals: &mut HashSet<String>,
    external_db: &Option<ExternalDb>,
) {
    if current_depth > max_depth {
        return;
    }

    let dashes = "-".repeat(level);

    for call in &func.calls {
        if call.target == "[unresolved]" {
            println!("[{}] {} [unresolved] {}", level, dashes, call.raw);
            continue;
        }

        if let Some((child_file, child_func)) = func_map.get(call.target.as_str()) {
            if visited.contains(call.target.as_str()) {
                println!("[{}] {} [cycle] {}", level, dashes, call.target);
                continue;
            }

            let summary_part = if show_summaries {
                child_func.summary.as_ref().map(|s| format!(" : \"{}\"", s)).unwrap_or_default()
            } else {
                String::new()
            };
            println!("[{}] {} {} ({}:{}-{}){}",
                level,
                dashes,
                child_func.qualified_name,
                child_file,
                child_func.line_start,
                child_func.line_end,
                summary_part
            );

            visited.insert(call.target.as_str());
            print_trace_level(func_map, index, child_func, level + 1, max_depth, current_depth + 1, visited, show_summaries, seen_externals, external_db);
            visited.remove(call.target.as_str());
        } else {
            // External
            if show_summaries {
                let first_occurrence = seen_externals.insert(call.target.clone());
                let summary_suffix = if first_occurrence {
                    get_external_summary(index, &call.target, external_db.as_ref().unwrap())
                } else {
                    String::new()
                };
                println!("[{}] {} [external] {}{}", level, dashes, call.target, summary_suffix);
            } else {
                println!("[{}] {} [external] {}", level, dashes, call.target);
            }
        }
    }
}

/// Get summary for an external call, checking index.externals first, then ExternalDb
/// Target is already formatted as "[kind:name]", so we just return ` : "summary"` or empty
fn get_external_summary(index: &Index, target: &str, external_db: &ExternalDb) -> String {
    // First check if we have it in index.externals
    if let Some(ext) = index.externals.get(target) {
        if let Some(summary) = &ext.summary {
            return format!(" : \"{}\"", summary);
        }
    }

    // Extract the function name - handle "[kind:name]" format
    let func_name = if target.starts_with('[') && target.contains(':') {
        // Format is [kind:name], extract name
        target.trim_start_matches('[')
            .trim_end_matches(']')
            .split(':')
            .nth(1)
            .unwrap_or(target)
    } else {
        // Plain name or qualified name like "pkg.Func"
        target.rsplit('.').next().unwrap_or(target)
    };

    // Try to categorize via ExternalDb (handles syscalls, libc, macros)
    let (_, summary) = external_db.categorize(func_name);

    if let Some(s) = summary {
        format!(" : \"{}\"", s)
    } else {
        String::new()
    }
}

fn query_usages(index: &Index, name: &str, max_depth: usize, show_summaries: bool) -> ExitCode {
    let func_map = build_function_map(index);
    let matches = find_functions(index, name);

    if matches.is_empty() {
        eprintln!("No function found matching '{name}'");
        return ExitCode::FAILURE;
    }

    let (file_path, func) = matches[0];

    if matches.len() > 1 {
        eprintln!("Multiple matches, showing first: {}", func.qualified_name);
        eprintln!();
    }

    // Print root with optional summary
    if show_summaries {
        let summary_part = func.summary.as_ref()
            .map(|s| format!(" : \"{}\"", s))
            .unwrap_or_default();
        println!(
            "{} ({}:{}-{}){}",
            func.qualified_name, file_path, func.line_start, func.line_end, summary_part
        );
    } else {
        println!(
            "{} ({}:{}-{})",
            func.qualified_name, file_path, func.line_start, func.line_end
        );
    }

    if func.called_by.is_empty() {
        println!("  (no callers found)");
        return ExitCode::SUCCESS;
    }

    // Print caller tree (reverse of trace)
    let mut visited = HashSet::new();
    visited.insert(func.qualified_name.as_str());
    print_usages_callers(&func_map, func, "", max_depth, 1, &mut visited, show_summaries);

    ExitCode::SUCCESS
}

fn print_usages_callers<'a>(
    func_map: &HashMap<&'a str, (&'a str, &'a Function)>,
    func: &'a Function,
    prefix: &str,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<&'a str>,
    show_summaries: bool,
) {
    if current_depth > max_depth {
        return;
    }

    let callers = &func.called_by;
    let total = callers.len();

    for (i, caller_name) in callers.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let new_prefix = format!("{}{}", prefix, child_prefix);

        if let Some((caller_file, caller_func)) = func_map.get(caller_name.as_str()) {
            if visited.contains(caller_name.as_str()) {
                println!("{}{}[cycle] {}", prefix, connector, caller_name);
                continue;
            }

            if show_summaries {
                let summary_part = caller_func.summary.as_ref()
                    .map(|s| format!(" : \"{}\"", s))
                    .unwrap_or_default();
                println!(
                    "{}{}{} ({}:{}-{}){}",
                    prefix, connector, caller_func.qualified_name, caller_file,
                    caller_func.line_start, caller_func.line_end, summary_part
                );
            } else {
                println!(
                    "{}{}{} ({}:{}-{})",
                    prefix, connector, caller_func.qualified_name, caller_file,
                    caller_func.line_start, caller_func.line_end
                );
            }

            visited.insert(caller_name.as_str());
            print_usages_callers(func_map, caller_func, &new_prefix, max_depth, current_depth + 1, visited, show_summaries);
            visited.remove(caller_name.as_str());
        } else {
            println!("{}{}[external] {}", prefix, connector, caller_name);
        }
    }
}

fn query_file(index: &Index, path: &str) -> ExitCode {
    // Normalize path - try with and without "./" prefix
    let normalized = path.strip_prefix("./").unwrap_or(path);
    let with_prefix = format!("./{}", normalized);

    let entry = index.files.get(path)
        .or_else(|| index.files.get(normalized))
        .or_else(|| index.files.get(&with_prefix));

    let (actual_path, entry) = match entry {
        Some(e) => {
            let p = if index.files.contains_key(path) {
                path
            } else if index.files.contains_key(normalized) {
                normalized
            } else {
                &with_prefix
            };
            (p, e)
        }
        None => {
            // Try partial match
            let matches: Vec<_> = index.files.iter()
                .filter(|(p, _)| p.contains(path) || p.contains(normalized))
                .collect();

            if matches.is_empty() {
                eprintln!("File not found in index: {path}");
                return ExitCode::FAILURE;
            }

            if matches.len() > 1 {
                eprintln!("Multiple files match '{path}':");
                for (p, _) in &matches {
                    eprintln!("  {p}");
                }
                return ExitCode::FAILURE;
            }

            (matches[0].0.as_str(), matches[0].1)
        }
    };

    println!("{actual_path}");
    println!();

    if !entry.types.is_empty() {
        println!("Types:");
        for t in &entry.types {
            println!("  {} {:?} (lines {}-{})", t.qualified_name, t.kind, t.line_start, t.line_end);
            if !t.methods.is_empty() {
                for m in &t.methods {
                    println!("    .{m}");
                }
            }
        }
        println!();
    }

    if !entry.functions.is_empty() {
        println!("Functions:");
        for f in &entry.functions {
            let calls_count = f.calls.len();
            let callers_count = f.called_by.len();
            println!(
                "  {} (lines {}-{}) [{} calls, {} callers]",
                f.qualified_name, f.line_start, f.line_end, calls_count, callers_count
            );
        }
    }

    ExitCode::SUCCESS
}

fn query_list(index: &Index, path: Option<&str>) -> ExitCode {
    match path {
        Some(p) => {
            // List functions in specific file
            let normalized = p.strip_prefix("./").unwrap_or(p);
            let with_prefix = format!("./{}", normalized);

            let entry = index.files.get(p)
                .or_else(|| index.files.get(normalized))
                .or_else(|| index.files.get(&with_prefix));

            match entry {
                Some(e) => {
                    for f in &e.functions {
                        println!("{}", f.qualified_name);
                    }
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("File not found in index: {p}");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            // List all functions
            let mut all_funcs: Vec<_> = index.files.iter()
                .flat_map(|(_, entry)| entry.functions.iter())
                .map(|f| &f.qualified_name)
                .collect();

            all_funcs.sort();

            for name in all_funcs {
                println!("{name}");
            }

            ExitCode::SUCCESS
        }
    }
}
