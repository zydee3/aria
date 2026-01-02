//! Binary embeddings storage
//!
//! Format:
//! - embeddings.idx: newline-separated qualified names
//! - embeddings.bin: raw f32 values, EMBEDDING_DIM per function, in same order as .idx

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

pub const EMBEDDING_DIM: usize = 768;

/// In-memory embedding store
pub struct EmbeddingStore {
    /// qualified_name -> embedding
    embeddings: HashMap<String, Vec<f32>>,
}

impl EmbeddingStore {
    pub fn new() -> Self {
        Self {
            embeddings: HashMap::new(),
        }
    }

    /// Load embeddings from .aria directory
    pub fn load(aria_dir: &Path) -> std::io::Result<Self> {
        let idx_path = aria_dir.join("embeddings.idx");
        let bin_path = aria_dir.join("embeddings.bin");

        if !idx_path.exists() || !bin_path.exists() {
            return Ok(Self::new());
        }

        // Read qualified names
        let idx_file = File::open(&idx_path)?;
        let reader = BufReader::new(idx_file);
        let names: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

        // Read binary embeddings
        let mut bin_file = File::open(&bin_path)?;
        let expected_size = names.len() * EMBEDDING_DIM * 4;
        let file_size = bin_file.metadata()?.len() as usize;

        if file_size != expected_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "embeddings.bin size mismatch: expected {} bytes, got {}",
                    expected_size, file_size
                ),
            ));
        }

        let mut embeddings = HashMap::with_capacity(names.len());
        let mut buf = vec![0u8; EMBEDDING_DIM * 4];

        for name in names {
            bin_file.read_exact(&mut buf)?;
            let embedding: Vec<f32> = buf
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            embeddings.insert(name, embedding);
        }

        Ok(Self { embeddings })
    }

    /// Save embeddings to .aria directory
    pub fn save(&self, aria_dir: &Path) -> std::io::Result<()> {
        let idx_path = aria_dir.join("embeddings.idx");
        let bin_path = aria_dir.join("embeddings.bin");

        // Sort names for deterministic output
        let mut names: Vec<&String> = self.embeddings.keys().collect();
        names.sort();

        // Write index file
        let idx_file = File::create(&idx_path)?;
        let mut idx_writer = BufWriter::new(idx_file);
        for name in &names {
            writeln!(idx_writer, "{}", name)?;
        }
        idx_writer.flush()?;

        // Write binary file
        let bin_file = File::create(&bin_path)?;
        let mut bin_writer = BufWriter::new(bin_file);
        for name in &names {
            if let Some(embedding) = self.embeddings.get(*name) {
                for &val in embedding {
                    bin_writer.write_all(&val.to_le_bytes())?;
                }
            }
        }
        bin_writer.flush()?;

        Ok(())
    }

    /// Get embedding for a qualified name
    pub fn get(&self, qualified_name: &str) -> Option<&Vec<f32>> {
        self.embeddings.get(qualified_name)
    }

    /// Insert an embedding
    pub fn insert(&mut self, qualified_name: String, embedding: Vec<f32>) {
        self.embeddings.insert(qualified_name, embedding);
    }

    /// Check if embedding exists
    pub fn contains(&self, qualified_name: &str) -> bool {
        self.embeddings.contains_key(qualified_name)
    }

    /// Number of embeddings
    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }

    /// Iterate over all embeddings
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<f32>)> {
        self.embeddings.iter()
    }
}

impl Default for EmbeddingStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove embeddings for functions that no longer exist
pub fn prune_embeddings(aria_dir: &Path, valid_names: &std::collections::HashSet<String>) -> std::io::Result<usize> {
    let mut store = EmbeddingStore::load(aria_dir)?;
    let before = store.len();

    store.embeddings.retain(|name, _| valid_names.contains(name));

    let removed = before - store.len();
    if removed > 0 {
        store.save(aria_dir)?;
    }

    Ok(removed)
}
