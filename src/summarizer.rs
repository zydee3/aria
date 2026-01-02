use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
pub struct Summarizer {
    batch_size: usize,
    parallel: usize,
}

#[derive(Debug)]
pub enum SummarizerError {
    CommandFailed(String),
    IoError(String),
}

impl std::fmt::Display for SummarizerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandFailed(msg) => write!(f, "claude command failed: {msg}"),
            Self::IoError(msg) => write!(f, "IO error: {msg}"),
        }
    }
}

impl std::error::Error for SummarizerError {}

impl From<std::io::Error> for SummarizerError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e.to_string())
    }
}

/// A function to be summarized
#[derive(Debug, Clone)]
pub struct SummaryRequest {
    pub id: usize,
    pub signature: String,
    pub body: String,
}

/// Result of summarization
#[derive(Debug)]
pub struct SummaryResult {
    pub id: usize,
    pub summary: Result<String, SummarizerError>,
}

impl Summarizer {
    pub fn new(batch_size: usize, parallel: usize) -> Self {
        Self {
            batch_size: batch_size.max(1),
            parallel: parallel.max(1),
        }
    }

    /// Summarize multiple functions with batching and parallelism
    pub fn summarize_batch(&self, requests: Vec<SummaryRequest>) -> Vec<SummaryResult> {
        if requests.is_empty() {
            return Vec::new();
        }

        // Group requests into batches
        let batches: Vec<Vec<SummaryRequest>> = requests
            .chunks(self.batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        // Process batches in parallel using threads
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();

        // Semaphore-like behavior: process `parallel` batches at a time
        for batch_chunk in batches.chunks(self.parallel) {
            let batch_chunk: Vec<Vec<SummaryRequest>> = batch_chunk.to_vec();

            for batch in batch_chunk {
                let tx = tx.clone();
                let handle = thread::spawn(move || {
                    let results = process_batch(batch);
                    for result in results {
                        let _ = tx.send(result);
                    }
                });
                handles.push(handle);
            }

            // Wait for this chunk of parallel batches to complete
            for handle in handles.drain(..) {
                let _ = handle.join();
            }
        }

        drop(tx);

        // Collect all results
        rx.into_iter().collect()
    }
}

/// Process a batch of functions, returning individual results
fn process_batch(batch: Vec<SummaryRequest>) -> Vec<SummaryResult> {
    if batch.len() == 1 {
        // Single function - simple prompt
        let req = &batch[0];
        let prompt = build_single_prompt(&req.signature, &req.body);
        let result = call_claude(&prompt);
        return vec![SummaryResult {
            id: req.id,
            summary: result,
        }];
    }

    // Multiple functions - batch prompt with structured output
    let prompt = build_batch_prompt(&batch);
    match call_claude(&prompt) {
        Ok(response) => parse_batch_response(&batch, &response),
        Err(e) => {
            // If batch fails, return error for all
            batch
                .iter()
                .map(|req| SummaryResult {
                    id: req.id,
                    summary: Err(SummarizerError::CommandFailed(e.to_string())),
                })
                .collect()
        }
    }
}

fn build_single_prompt(signature: &str, body: &str) -> String {
    format!(
        "Summarize what this function does in 1-2 sentences. \
         Focus on behavior, not implementation details. \
         Do not repeat documentation comments. \
         Reply with ONLY the summary, no preamble.\n\n\
         Function: {signature}\n\
         Body:\n{body}"
    )
}

fn build_batch_prompt(batch: &[SummaryRequest]) -> String {
    let mut prompt = String::from(
        "Summarize what each function does in 1-2 sentences. \
         Focus on behavior, not implementation details. \
         Do not repeat documentation comments.\n\n\
         Reply in this exact format for each function:\n\
         [N]: <summary>\n\n\
         Where N is the function number.\n\n",
    );

    for (i, req) in batch.iter().enumerate() {
        prompt.push_str(&format!(
            "=== Function {} ===\n{}\n{}\n\n",
            i + 1,
            req.signature,
            req.body
        ));
    }

    prompt
}

fn parse_batch_response(batch: &[SummaryRequest], response: &str) -> Vec<SummaryResult> {
    let mut results = Vec::new();

    for (i, req) in batch.iter().enumerate() {
        let marker = format!("[{}]:", i + 1);
        let summary = response
            .lines()
            .find(|line| line.starts_with(&marker))
            .map(|line| line[marker.len()..].trim().to_string())
            .unwrap_or_else(|| format!("(failed to parse summary for function {})", i + 1));

        results.push(SummaryResult {
            id: req.id,
            summary: Ok(summary),
        });
    }

    results
}

fn call_claude(prompt: &str) -> Result<String, SummarizerError> {
    let mut child = Command::new("claude")
        .arg("--print")
        .current_dir("/tmp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Write prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(SummarizerError::CommandFailed(stderr.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_single_prompt() {
        let prompt = build_single_prompt("func Foo(x int) int", "{ return x * 2 }");
        assert!(prompt.contains("func Foo"));
        assert!(prompt.contains("return x * 2"));
    }

    #[test]
    fn test_build_batch_prompt() {
        let batch = vec![
            SummaryRequest {
                id: 0,
                signature: "func A()".to_string(),
                body: "{}".to_string(),
            },
            SummaryRequest {
                id: 1,
                signature: "func B()".to_string(),
                body: "{}".to_string(),
            },
        ];
        let prompt = build_batch_prompt(&batch);
        assert!(prompt.contains("=== Function 1 ==="));
        assert!(prompt.contains("=== Function 2 ==="));
        assert!(prompt.contains("[N]:"));
    }

    #[test]
    fn test_parse_batch_response() {
        let batch = vec![
            SummaryRequest {
                id: 0,
                signature: "func A()".to_string(),
                body: "{}".to_string(),
            },
            SummaryRequest {
                id: 1,
                signature: "func B()".to_string(),
                body: "{}".to_string(),
            },
        ];
        let response = "[1]: Does thing A\n[2]: Does thing B";
        let results = parse_batch_response(&batch, response);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, 0);
        assert_eq!(results[0].summary.as_ref().unwrap(), "Does thing A");
        assert_eq!(results[1].id, 1);
        assert_eq!(results[1].summary.as_ref().unwrap(), "Does thing B");
    }
}
