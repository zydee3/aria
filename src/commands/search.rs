use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Args;

use crate::config::Config;
use crate::embedder::{cosine_similarity, Embedder};
use crate::embeddings::EmbeddingStore;
use crate::index::Index;

#[derive(Args)]
pub struct SearchArgs {
    /// Natural language query
    pub query: String,
    /// Maximum results
    #[arg(long, short = 'n', default_value = "10")]
    pub limit: usize,
    /// Minimum similarity threshold (0.0 to 1.0)
    #[arg(long, default_value = "0.0")]
    pub threshold: f32,
}

pub fn run(args: SearchArgs) -> ExitCode {
    let aria_dir = Path::new(".aria");

    // Load config
    let config_path = aria_dir.join("config.toml");
    let config: Config = match fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    };

    // Load index
    let index_path = aria_dir.join("index.json");
    let index: Index = match fs::read_to_string(&index_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(idx) => idx,
            Err(e) => {
                eprintln!("Error parsing index: {e}");
                return ExitCode::FAILURE;
            }
        },
        Err(_) => {
            eprintln!("No index found. Run 'aria index' first.");
            return ExitCode::FAILURE;
        }
    };

    // Load embeddings
    let store = match EmbeddingStore::load(aria_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error loading embeddings: {e}");
            return ExitCode::FAILURE;
        }
    };

    if store.is_empty() {
        eprintln!("No embeddings found. Run 'aria embed' to generate embeddings.");
        return ExitCode::FAILURE;
    }

    // Initialize embedder and check Ollama
    let embedder = Embedder::new(&config.embeddings);
    if let Err(e) = embedder.check_available() {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }

    // Embed the query
    eprint!("Embedding query...");
    let query_embedding = match embedder.embed(&args.query) {
        Ok(emb) => {
            eprintln!(" done");
            emb
        }
        Err(e) => {
            eprintln!("\nError embedding query: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Build lookup: qualified_name -> (path, summary)
    let mut func_info: std::collections::HashMap<&str, (&str, Option<&str>)> =
        std::collections::HashMap::new();
    for (path, file_entry) in &index.files {
        for func in &file_entry.functions {
            func_info.insert(&func.qualified_name, (path, func.summary.as_deref()));
        }
    }

    // Compute similarities and collect results
    let mut results: Vec<(f32, &str, &str, Option<&str>)> = Vec::new();

    for (qualified_name, embedding) in store.iter() {
        let similarity = cosine_similarity(&query_embedding, embedding);
        if similarity >= args.threshold {
            if let Some(&(path, summary)) = func_info.get(qualified_name.as_str()) {
                results.push((similarity, path, qualified_name, summary));
            }
        }
    }

    // Sort by similarity descending
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Display results
    if results.is_empty() {
        println!("No results found.");
        return ExitCode::SUCCESS;
    }

    println!("\nSearch results for: \"{}\"\n", args.query);

    for (i, (similarity, path, qualified_name, summary)) in
        results.iter().take(args.limit).enumerate()
    {
        println!(
            "{}. {} ({:.1}%)",
            i + 1,
            qualified_name,
            similarity * 100.0
        );
        println!("   File: {path}");
        if let Some(summary) = summary {
            println!("   {summary}");
        }
        println!();
    }

    ExitCode::SUCCESS
}
