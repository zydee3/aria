use std::fs;
use std::process::ExitCode;

use crate::config::Config;
use crate::embedder::Embedder;
use crate::index::Index;

pub fn run() -> ExitCode {
    // Load config
    let config_path = ".aria/config.toml";
    let config: Config = match fs::read_to_string(config_path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => {
            eprintln!("No config found. Run 'aria init' first.");
            return ExitCode::FAILURE;
        }
    };

    // Load index
    let index_path = ".aria/index.json";
    let mut index: Index = match fs::read_to_string(index_path) {
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
    let mut to_embed: Vec<(String, String, String)> = Vec::new(); // (file_path, qualified_name, text)

    for (path, file_entry) in &index.files {
        for func in &file_entry.functions {
            // Skip if already has embedding
            if func.embedding.is_some() {
                continue;
            }

            // Build text to embed: signature + summary
            let text = if let Some(ref summary) = func.summary {
                format!("{}\n{}", func.signature, summary)
            } else {
                func.signature.clone()
            };

            to_embed.push((path.clone(), func.qualified_name.clone(), text));
        }
    }

    if to_embed.is_empty() {
        println!("All functions already have embeddings.");
        return ExitCode::SUCCESS;
    }

    println!("Generating embeddings for {} functions...", to_embed.len());

    // Extract just the texts for batch embedding
    let texts: Vec<String> = to_embed.iter().map(|(_, _, text)| text.clone()).collect();

    // Embed all texts
    let embeddings = embedder.embed_batch(&texts);

    // Update index with embeddings
    let mut success_count = 0;
    let mut error_count = 0;

    for ((path, qualified_name, _), embedding_result) in to_embed.iter().zip(embeddings.into_iter()) {
        match embedding_result {
            Ok(embedding) => {
                // Find and update the function in the index
                if let Some(file_entry) = index.files.get_mut(path) {
                    if let Some(func) = file_entry
                        .functions
                        .iter_mut()
                        .find(|f| &f.qualified_name == qualified_name)
                    {
                        func.embedding = Some(embedding);
                        success_count += 1;
                    }
                }
            }
            Err(e) => {
                if error_count == 0 {
                    eprintln!("\nFirst error: {e}");
                }
                error_count += 1;
            }
        }
    }

    // Save updated index
    println!("Saving index...");
    let json = serde_json::to_string_pretty(&index).expect("Failed to serialize index");
    if let Err(e) = fs::write(index_path, json) {
        eprintln!("Error writing index: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "Done. Embedded {} functions ({} errors).",
        success_count, error_count
    );

    if error_count > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
