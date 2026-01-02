use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub version: String,
    pub commit: String,
    pub indexed_at: DateTime<Utc>,
    pub files: HashMap<String, FileEntry>,
}

impl Index {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            commit: String::new(),
            indexed_at: Utc::now(),
            files: HashMap::new(),
        }
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub ast_hash: String,
    pub functions: Vec<Function>,
    pub types: Vec<TypeDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub qualified_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    pub scope: Scope,
    pub calls: Vec<CallSite>,
    pub called_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    /// Resolved qualified name of the called function, or "[unresolved]" if resolution fails
    pub target: String,
    /// Original call expression as written in source (e.g., "pkg.Foo", "obj.Method()")
    pub raw: String,
    /// 1-indexed line number of the call site
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    pub name: String,
    pub qualified_name: String,
    pub kind: TypeKind,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Public,
    Static,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypeKind {
    Struct,
    Interface,
    Typedef,
    Enum,
}
