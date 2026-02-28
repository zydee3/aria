use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub version: String,
    pub commit: String,
    pub indexed_at: DateTime<Utc>,
    pub files: HashMap<String, FileEntry>,
    /// External symbols (syscalls, libc, macros) referenced but not defined in codebase
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub externals: HashMap<String, ExternalEntry>,
}

impl Index {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            commit: String::new(),
            indexed_at: Utc::now(),
            files: HashMap::new(),
            externals: HashMap::new(),
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub qualified_name: String,
    #[serde(default)]
    pub ast_hash: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub qualified_name: String,
    /// Type as written in source (e.g., "struct cr_fd_desc_tmpl", "int")
    pub type_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub scope: Scope,
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

/// Entry for an external symbol (syscall, libc function, macro)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalEntry {
    /// Kind of external: syscall, libc, macro, external
    pub kind: String,
    /// Optional summary/description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Number of call sites referencing this external
    #[serde(default)]
    pub references: u32,
}

/// Load index from .aria/index.json
pub fn load_index() -> Result<Index, String> {
    let index_path = Path::new(".aria/index.json");
    if !index_path.exists() {
        return Err("index not found (run `aria index` first)".to_string());
    }

    let content = fs::read_to_string(index_path)
        .map_err(|e| format!("failed to read index: {e}"))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse index: {e}"))
}

/// Find functions matching a name (exact qualified, exact simple, then contains)
pub fn find_functions<'a>(index: &'a Index, name: &str) -> Vec<(&'a str, &'a Function)> {
    let mut matches = Vec::new();

    for (file_path, entry) in &index.files {
        for func in &entry.functions {
            if func.qualified_name == name || func.name == name || func.qualified_name.contains(name) {
                matches.push((file_path.as_str(), func));
            }
        }
    }

    matches
}

/// Build a lookup table: qualified_name -> (file_path, &Function)
pub fn build_function_map<'a>(index: &'a Index) -> HashMap<&'a str, (&'a str, &'a Function)> {
    let mut map = HashMap::new();
    for (file_path, entry) in &index.files {
        for func in &entry.functions {
            map.insert(func.qualified_name.as_str(), (file_path.as_str(), func));
        }
    }
    map
}
