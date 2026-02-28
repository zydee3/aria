use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use serde::Serialize;

use crate::index::{self, Index};
use crate::topo;

const OUTPUT_FILE: &str = "rank.json";

#[derive(Serialize)]
struct RankOutput {
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

    let (all_functions, calls_map) = build_call_graph(&idx);
    let levels = topo::hierarchy(&all_functions, &calls_map);

    let output = RankOutput { levels };

    let json = match serde_json::to_string_pretty(&output) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("error: failed to serialize: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::write(&output_path, &json) {
        eprintln!("error: failed to write {}: {e}", output_path.display());
        return ExitCode::FAILURE;
    }

    let full_path = fs::canonicalize(&output_path)
        .unwrap_or_else(|_| output_path.to_path_buf());

    println!(
        "Wrote \"{}\": {} functions in {} levels ({:.2?})",
        full_path.display(),
        all_functions.len(),
        output.levels.len(),
        start.elapsed()
    );

    ExitCode::SUCCESS
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
