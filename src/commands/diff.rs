use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use walkdir::WalkDir;

use crate::index::Index;
use crate::parser::GoParser;

pub fn run() -> ExitCode {
    let aria_dir = Path::new(".aria");

    if !aria_dir.exists() {
        eprintln!("error: not initialized (run `aria init` first)");
        return ExitCode::FAILURE;
    }

    // Load existing index
    let index_path = aria_dir.join("index.json");
    let index: Index = match fs::read_to_string(&index_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(idx) => idx,
            Err(e) => {
                eprintln!("error: failed to parse index.json: {e}");
                return ExitCode::FAILURE;
            }
        },
        Err(e) => {
            eprintln!("error: failed to read index.json: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Build lookup: qualified_name -> ast_hash from existing index
    let mut indexed_hashes: HashMap<String, String> = HashMap::new();
    for entry in index.files.values() {
        for func in &entry.functions {
            indexed_hashes.insert(func.qualified_name.clone(), func.ast_hash.clone());
        }
    }

    // Parse current working tree
    let mut parser = GoParser::new();
    let mut current_funcs: HashMap<String, (String, String)> = HashMap::new(); // qualified_name -> (ast_hash, path)

    for entry in WalkDir::new(".")
        .into_iter()
        .filter_entry(|e| !is_hidden(e) && !is_ignored(e))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if path.extension().is_none_or(|ext| ext != "go") {
            continue;
        }

        if path.to_string_lossy().ends_with("_test.go") {
            continue;
        }

        let path_str = path.to_string_lossy();

        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if let Some(file_entry) = parser.parse_file(&source, &path_str) {
            for func in file_entry.functions {
                current_funcs.insert(
                    func.qualified_name.clone(),
                    (func.ast_hash, path_str.to_string()),
                );
            }
        }
    }

    // Compare
    let mut added: Vec<(String, String)> = Vec::new(); // (qualified_name, path)
    let mut modified: Vec<(String, String)> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();

    // Check for added/modified
    for (qname, (hash, path)) in &current_funcs {
        match indexed_hashes.get(qname) {
            None => added.push((qname.clone(), path.clone())),
            Some(indexed_hash) if indexed_hash.is_empty() || indexed_hash != hash => {
                // Empty hash means old index format - treat as modified
                modified.push((qname.clone(), path.clone()))
            }
            _ => {}
        }
    }

    // Check for deleted
    for qname in indexed_hashes.keys() {
        if !current_funcs.contains_key(qname) {
            deleted.push(qname.clone());
        }
    }

    // Output
    let total = added.len() + modified.len() + deleted.len();

    if total == 0 {
        println!("No changes detected");
        return ExitCode::SUCCESS;
    }

    if !added.is_empty() {
        println!("Added ({}):", added.len());
        for (qname, path) in &added {
            println!("  + {} ({})", qname, path);
        }
    }

    if !modified.is_empty() {
        println!("Modified ({}):", modified.len());
        for (qname, path) in &modified {
            println!("  ~ {} ({})", qname, path);
        }
    }

    if !deleted.is_empty() {
        println!("Deleted ({}):", deleted.len());
        for qname in &deleted {
            println!("  - {}", qname);
        }
    }

    println!("\nTotal: {} changes", total);

    ExitCode::SUCCESS
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
