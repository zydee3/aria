use std::collections::HashSet;
use std::process::ExitCode;

use crate::externals::ExternalDb;
use crate::index::{self, Function, Index};

pub fn run(name: &str, forward: bool, backward: bool, depth: usize) -> ExitCode {
    let index = match index::load_index() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let func_map = index::build_function_map(&index);
    let matches = index::find_functions(&index, name);

    if matches.is_empty() {
        eprintln!("No function found matching '{name}'");
        return ExitCode::FAILURE;
    }

    let max_depth = if depth == 0 { usize::MAX } else { depth };
    let show_both = !forward && !backward;

    for (i, (file_path, func)) in matches.iter().enumerate() {
        if matches.len() > 1 {
            if i > 0 {
                println!();
            }
            println!("=== {} ({}:{}-{}) ===", func.qualified_name, file_path, func.line_start, func.line_end);
        }

        if backward || show_both {
            print_backward(&func_map, file_path, func, max_depth);
        }

        if forward || show_both {
            if (backward || show_both) && !func.called_by.is_empty() {
                println!();
            }
            print_forward(&func_map, &index, file_path, func, max_depth);
        }
    }

    ExitCode::SUCCESS
}

fn print_backward(
    func_map: &std::collections::HashMap<&str, (&str, &Function)>,
    file_path: &str,
    func: &Function,
    max_depth: usize,
) {
    println!(
        "{} ({}:{}-{})",
        func.qualified_name, file_path, func.line_start, func.line_end
    );

    if func.called_by.is_empty() {
        println!("  (no callers found)");
        return;
    }

    println!("  called by:");
    let mut visited = HashSet::new();
    visited.insert(func.qualified_name.as_str());
    print_callers(func_map, func, "  ", max_depth, 1, &mut visited);
}

fn print_callers<'a>(
    func_map: &std::collections::HashMap<&'a str, (&'a str, &'a Function)>,
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
                prefix, connector, caller_func.qualified_name, caller_file,
                caller_func.line_start, caller_func.line_end
            );

            visited.insert(caller_name.as_str());
            print_callers(func_map, caller_func, &new_prefix, max_depth, current_depth + 1, visited);
            visited.remove(caller_name.as_str());
        } else {
            println!("{}{}[external] {}", prefix, connector, caller_name);
        }
    }
}

fn print_forward(
    func_map: &std::collections::HashMap<&str, (&str, &Function)>,
    index: &Index,
    file_path: &str,
    func: &Function,
    max_depth: usize,
) {
    let external_db = ExternalDb::new();
    let mut seen_externals = HashSet::new();

    println!(
        "[0] {} ({}:{}-{})",
        func.qualified_name, file_path, func.line_start, func.line_end
    );

    let mut visited = HashSet::new();
    visited.insert(func.qualified_name.as_str());
    print_forward_level(func_map, index, func, 1, max_depth, 1, &mut visited, &mut seen_externals, &external_db);
}

fn print_forward_level<'a>(
    func_map: &std::collections::HashMap<&'a str, (&'a str, &'a Function)>,
    index: &'a Index,
    func: &'a Function,
    level: usize,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<&'a str>,
    seen_externals: &mut HashSet<String>,
    external_db: &ExternalDb,
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

            println!(
                "[{}] {} {} ({}:{}-{})",
                level, dashes, child_func.qualified_name, child_file,
                child_func.line_start, child_func.line_end
            );

            visited.insert(call.target.as_str());
            print_forward_level(func_map, index, child_func, level + 1, max_depth, current_depth + 1, visited, seen_externals, external_db);
            visited.remove(call.target.as_str());
        } else {
            let first_occurrence = seen_externals.insert(call.target.clone());
            let summary_suffix = if first_occurrence {
                get_external_summary(index, &call.target, external_db)
            } else {
                String::new()
            };
            println!("[{}] {} [external] {}{}", level, dashes, call.target, summary_suffix);
        }
    }
}

fn get_external_summary(index: &Index, target: &str, external_db: &ExternalDb) -> String {
    if let Some(ext) = index.externals.get(target) {
        if let Some(summary) = &ext.summary {
            return format!(" : \"{}\"", summary);
        }
    }

    let func_name = if target.starts_with('[') && target.contains(':') {
        target.trim_start_matches('[')
            .trim_end_matches(']')
            .split(':')
            .nth(1)
            .unwrap_or(target)
    } else {
        target.rsplit('.').next().unwrap_or(target)
    };

    let (_, summary) = external_db.categorize(func_name);

    if let Some(s) = summary {
        format!(" : \"{}\"", s)
    } else {
        String::new()
    }
}
