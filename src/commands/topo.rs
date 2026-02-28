use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::index;
use crate::topo;

const OUTPUT_FILE: &str = "topo.json";

#[derive(Serialize, Deserialize)]
struct TopoOutput {
    index_hash: String,
    levels: Vec<Vec<String>>,
}

pub fn run() -> ExitCode {
    let start = Instant::now();
    let aria_dir = Path::new(".aria");
    let index_path = aria_dir.join("index.json");
    let output_path = aria_dir.join(OUTPUT_FILE);

    // Load index
    let idx = match index::load_index() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Compute hash of raw index.json bytes
    let index_bytes = match fs::read(&index_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: failed to read index.json: {e}");
            return ExitCode::FAILURE;
        }
    };
    let index_hash = format!("{:016x}", hash_bytes(&index_bytes));

    // Check cache
    if let Ok(existing) = fs::read_to_string(&output_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&existing) {
            if val.get("index_hash").and_then(|v| v.as_str()) == Some(&index_hash) {
                println!("{OUTPUT_FILE}: up to date");
                return ExitCode::SUCCESS;
            }
        }
    }

    // Build call graph from index
    let mut all_functions: HashSet<String> = HashSet::new();
    let mut calls_map: HashMap<String, HashSet<String>> = HashMap::new();

    for entry in idx.files.values() {
        for func in &entry.functions {
            all_functions.insert(func.qualified_name.clone());

            let callees: HashSet<String> = func
                .calls
                .iter()
                .filter(|c| !c.target.starts_with('['))
                .map(|c| c.target.clone())
                .collect();

            if !callees.is_empty() {
                calls_map.insert(func.qualified_name.clone(), callees);
            }
        }
    }

    let levels = topo::hierarchy(&all_functions, &calls_map);

    let output = TopoOutput {
        index_hash,
        levels,
    };

    let json = match serde_json::to_string_pretty(&output) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("error: failed to serialize: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::write(&output_path, &json) {
        eprintln!("error: failed to write {OUTPUT_FILE}: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "Wrote {OUTPUT_FILE}: {} functions in {} levels ({:.2?})",
        all_functions.len(),
        output.levels.len(),
        start.elapsed()
    );

    ExitCode::SUCCESS
}

fn hash_bytes(input: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}
