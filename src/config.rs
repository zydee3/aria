use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: false,
            llm: LlmConfig::default(),
            features: FeaturesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_parallel")]
    pub parallel: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key: None,
            model: default_model(),
            batch_size: default_batch_size(),
            parallel: default_parallel(),
        }
    }
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-3-haiku".to_string()
}

fn default_batch_size() -> usize {
    10
}

fn default_parallel() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    #[serde(default)]
    pub summaries: bool,
    #[serde(default)]
    pub embeddings: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            summaries: false,
            embeddings: false,
        }
    }
}
