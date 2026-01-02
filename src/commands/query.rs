use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Subcommand;

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
        /// Trace depth
        #[arg(long, default_value = "2")]
        depth: usize,
    },

    /// Find all usages of a symbol (backward edge: what calls this function)
    Usages {
        /// Symbol name
        name: String,
        /// Maximum depth for caller chain
        #[arg(long, default_value = "1")]
        depth: usize,
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
        QueryCommand::Trace { name, depth } => query_trace(&index, &name, depth),
        QueryCommand::Usages { name, depth } => query_usages(&index, &name, depth),
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

fn query_trace(index: &Index, name: &str, max_depth: usize) -> ExitCode {
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

    // Print root
    print_trace_node(file_path, func, "", true);

    // Recursively print call tree
    let mut visited = HashSet::new();
    visited.insert(func.qualified_name.as_str());
    print_trace_children(&func_map, func, "", max_depth, 1, &mut visited);

    ExitCode::SUCCESS
}

fn print_trace_node(file_path: &str, func: &Function, prefix: &str, is_root: bool) {
    if is_root {
        println!(
            "{} ({}:{}-{})",
            func.qualified_name, file_path, func.line_start, func.line_end
        );
    } else {
        println!(
            "{}{} ({}:{}-{})",
            prefix, func.qualified_name, file_path, func.line_start, func.line_end
        );
    }

    if let Some(summary) = &func.summary {
        let summary_prefix = if is_root { "│ " } else { &format!("{}│ ", prefix.replace("├── ", "│   ").replace("└── ", "    ")) };
        println!("{}{}", summary_prefix, summary);
    }
}

fn print_trace_children<'a>(
    func_map: &HashMap<&'a str, (&'a str, &'a Function)>,
    func: &'a Function,
    prefix: &str,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<&'a str>,
) {
    if current_depth > max_depth {
        return;
    }

    let calls: Vec<_> = func.calls.iter().collect();
    let total = calls.len();

    for (i, call) in calls.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let new_prefix = format!("{}{}", prefix, child_prefix);

        if call.target == "[unresolved]" {
            println!("{}{}[unresolved] {}", prefix, connector, call.raw);
            continue;
        }

        if let Some((child_file, child_func)) = func_map.get(call.target.as_str()) {
            // Check for cycles
            if visited.contains(call.target.as_str()) {
                println!("{}{}[cycle] {}", prefix, connector, call.target);
                continue;
            }

            print_trace_node(child_file, child_func, &format!("{}{}", prefix, connector), false);

            visited.insert(call.target.as_str());
            print_trace_children(func_map, child_func, &new_prefix, max_depth, current_depth + 1, visited);
            visited.remove(call.target.as_str());
        } else {
            // Resolved but not in index (external)
            println!("{}{}[external] {}", prefix, connector, call.target);
        }
    }
}

fn query_usages(index: &Index, name: &str, max_depth: usize) -> ExitCode {
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

    println!(
        "{} ({}:{}-{})",
        func.qualified_name, file_path, func.line_start, func.line_end
    );

    if func.called_by.is_empty() {
        println!("  (no callers found)");
        return ExitCode::SUCCESS;
    }

    // Print caller tree (reverse of trace)
    let mut visited = HashSet::new();
    visited.insert(func.qualified_name.as_str());
    print_usages_callers(&func_map, func, "", max_depth, 1, &mut visited);

    ExitCode::SUCCESS
}

fn print_usages_callers<'a>(
    func_map: &HashMap<&'a str, (&'a str, &'a Function)>,
    func: &'a Function,
    prefix: &str,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<&'a str>,
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

            println!(
                "{}{}{} ({}:{}-{})",
                prefix, connector, caller_func.qualified_name, caller_file, caller_func.line_start, caller_func.line_end
            );

            visited.insert(caller_name.as_str());
            print_usages_callers(func_map, caller_func, &new_prefix, max_depth, current_depth + 1, visited);
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
