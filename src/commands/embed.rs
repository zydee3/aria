use std::fs;
use std::path::Path;
use std::process::ExitCode;

use crate::config::Config;
use crate::embedder::Embedder;
use crate::embeddings::EmbeddingStore;
use crate::index::Index;

pub fn run() -> ExitCode {
    let aria_dir = Path::new(".aria");

    // Load config
    let config_path = aria_dir.join("config.toml");
    let config: Config = match fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => {
            eprintln!("No config found. Run 'aria init' first.");
            return ExitCode::FAILURE;
        }
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

    // Load existing embeddings
    let mut store = match EmbeddingStore::load(aria_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error loading embeddings: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Initialize embedder
    let embedder = Embedder::new(&config.embeddings);

    // Check Ollama is running
    println!("Checking Ollama...");
    if let Err(e) = embedder.check_available() {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }

    // Collect functions that need embeddings
    // We embed: signature + summary (if available)
    let mut to_embed: Vec<(String, String)> = Vec::new(); // (qualified_name, text)

    for file_entry in index.files.values() {
        for func in &file_entry.functions {
            // Skip if already has embedding
            if store.contains(&func.qualified_name) {
                continue;
            }

            // Build text to embed: signature + summary
            let text = if let Some(ref summary) = func.summary {
                format!("{}\n{}", func.signature, summary)
            } else {
                func.signature.clone()
            };

            to_embed.push((func.qualified_name.clone(), text));
        }
    }

    if to_embed.is_empty() {
        println!("All functions already have embeddings.");
        return ExitCode::SUCCESS;
    }

    println!("Generating embeddings for {} functions...", to_embed.len());

    // Extract just the texts for batch embedding
    let texts: Vec<String> = to_embed.iter().map(|(_, text)| text.clone()).collect();

    // Embed all texts
    let embeddings = embedder.embed_batch(&texts);

    // Update store with embeddings
    let mut success_count = 0;
    let mut error_count = 0;

    for ((qualified_name, _), embedding_result) in to_embed.iter().zip(embeddings.into_iter()) {
        match embedding_result {
            Ok(embedding) => {
                store.insert(qualified_name.clone(), embedding);
                success_count += 1;
            }
            Err(e) => {
                if error_count == 0 {
                    eprintln!("\nFirst error: {e}");
                }
                error_count += 1;
            }
        }
    }

    // Save embeddings
    println!("Saving embeddings...");
    if let Err(e) = store.save(aria_dir) {
        eprintln!("Error writing embeddings: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "Done. Embedded {} functions ({} errors). Total: {} embeddings.",
        success_count, error_count, store.len()
    );

    if error_count > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
