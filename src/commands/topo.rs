use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::index::{self, Index};
use crate::topo;

const OUTPUT_FILE: &str = "rank.json";

#[derive(Serialize, Deserialize)]
struct RankOutput {
    index_hash: String,
    levels: Vec<Vec<String>>,
}

pub fn run() -> ExitCode {
    let start = Instant::now();
    let aria_dir = Path::new(".aria");
    let output_path = aria_dir.join(OUTPUT_FILE);

    let idx = match index::load_index() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let index_hash = match compute_index_hash(aria_dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if is_cache_valid(&output_path, &index_hash) {
        println!("{OUTPUT_FILE}: up to date");
        return ExitCode::SUCCESS;
    }

    let (all_functions, calls_map) = build_call_graph(&idx);
    let levels = topo::hierarchy(&all_functions, &calls_map);

    let output = RankOutput {
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

fn compute_index_hash(aria_dir: &Path) -> Result<String, String> {
    let bytes = fs::read(aria_dir.join("index.json"))
        .map_err(|e| format!("failed to read index.json: {e}"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn is_cache_valid(output_path: &Path, index_hash: &str) -> bool {
    let Ok(existing) = fs::read_to_string(output_path) else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&existing) else {
        return false;
    };
    val.get("index_hash").and_then(|v| v.as_str()) == Some(index_hash)
}

fn build_call_graph(idx: &Index) -> (HashSet<String>, HashMap<String, HashSet<String>>) {
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

    (all_functions, calls_map)
}
