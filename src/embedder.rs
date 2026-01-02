use serde::{Deserialize, Serialize};

use crate::config::EmbeddingsConfig;

#[derive(Debug)]
pub struct Embedder {
    url: String,
    model: String,
    batch_size: usize,
}

#[derive(Debug)]
pub enum EmbedderError {
    OllamaNotRunning,
    RequestFailed(String),
    InvalidResponse(String),
}

impl std::fmt::Display for EmbedderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OllamaNotRunning => write!(f, "Ollama is not running. Start it with: ollama serve"),
            Self::RequestFailed(msg) => write!(f, "Embedding request failed: {msg}"),
            Self::InvalidResponse(msg) => write!(f, "Invalid response from Ollama: {msg}"),
        }
    }
}

impl std::error::Error for EmbedderError {}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

impl Embedder {
    pub fn new(config: &EmbeddingsConfig) -> Self {
        Self {
            url: config.ollama_url.clone(),
            model: config.model.clone(),
            batch_size: config.batch_size,
        }
    }

    /// Check if Ollama is running
    pub fn check_available(&self) -> Result<(), EmbedderError> {
        let url = format!("{}/api/tags", self.url);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(2)).call() {
            Ok(_) => Ok(()),
            Err(_) => Err(EmbedderError::OllamaNotRunning),
        }
    }

    /// Embed a single text
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedderError> {
        let url = format!("{}/api/embeddings", self.url);
        let request = EmbedRequest {
            model: &self.model,
            prompt: text,
        };

        let response = ureq::post(&url)
            .send_json(&request)
            .map_err(|e| EmbedderError::RequestFailed(e.to_string()))?;

        let embed_response: EmbedResponse = response
            .into_json()
            .map_err(|e| EmbedderError::InvalidResponse(e.to_string()))?;

        Ok(embed_response.embedding)
    }

    /// Embed multiple texts, showing progress
    pub fn embed_batch(&self, texts: &[String]) -> Vec<Result<Vec<f32>, EmbedderError>> {
        let total = texts.len();
        let mut results = Vec::with_capacity(total);

        for (i, chunk) in texts.chunks(self.batch_size).enumerate() {
            let batch_start = i * self.batch_size;
            for (j, text) in chunk.iter().enumerate() {
                let idx = batch_start + j + 1;
                eprint!("\r  Embedding {}/{}", idx, total);
                results.push(self.embed(text));
            }
        }
        eprintln!();

        results
    }
}

/// Compute cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
