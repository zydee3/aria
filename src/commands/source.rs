use std::fs;
use std::process::ExitCode;

use crate::index::{self, Index, TypeKind};

/// Print raw source code for a symbol range
fn print_source(file_path: &str, line_start: u32, line_end: u32) {
    if let Ok(content) = fs::read_to_string(file_path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = (line_start as usize).saturating_sub(1);
        let end = (line_end as usize).min(lines.len());
        if start < lines.len() {
            for line in &lines[start..end] {
                println!("{line}");
            }
        }
    }
}

fn parse_kind_filter(kind: &str) -> Result<KindFilter, String> {
    match kind {
        "function" => Ok(KindFilter::Function),
        "struct" => Ok(KindFilter::Type(TypeKind::Struct)),
        "enum" => Ok(KindFilter::Type(TypeKind::Enum)),
        "typedef" => Ok(KindFilter::Type(TypeKind::Typedef)),
        "interface" => Ok(KindFilter::Type(TypeKind::Interface)),
        "variable" => Ok(KindFilter::Variable),
        _ => Err(format!("unknown kind '{kind}' (expected: function, struct, enum, typedef, interface, variable)")),
    }
}

enum KindFilter {
    Function,
    Type(TypeKind),
    Variable,
}

struct SymbolMatch {
    qualified_name: String,
    file_path: String,
    line_start: u32,
    line_end: u32,
}

/// Find all symbols matching `name`, optionally filtered by kind
fn find_symbols(index: &Index, name: &str, kind: Option<&str>) -> Result<Vec<SymbolMatch>, String> {
    let filter = match kind {
        Some(k) => Some(parse_kind_filter(k)?),
        None => None,
    };

    let mut matches = Vec::new();

    for (file_path, entry) in &index.files {
        if !matches!(filter, Some(KindFilter::Type(_)) | Some(KindFilter::Variable)) {
            for func in &entry.functions {
                if func.qualified_name == name || func.name == name || func.qualified_name.contains(name) {
                    matches.push(SymbolMatch {
                        qualified_name: func.qualified_name.clone(),
                        file_path: file_path.clone(),
                        line_start: func.line_start,
                        line_end: func.line_end,
                    });
                }
            }
        }

        if !matches!(filter, Some(KindFilter::Function) | Some(KindFilter::Variable)) {
            for t in &entry.types {
                if let Some(KindFilter::Type(ref k)) = filter {
                    if t.kind != *k {
                        continue;
                    }
                }
                if t.name == name || t.qualified_name == name || t.qualified_name.contains(name) {
                    matches.push(SymbolMatch {
                        qualified_name: t.qualified_name.clone(),
                        file_path: file_path.clone(),
                        line_start: t.line_start,
                        line_end: t.line_end,
                    });
                }
            }
        }

        if !matches!(filter, Some(KindFilter::Function) | Some(KindFilter::Type(_))) {
            for v in &entry.variables {
                if v.name == name || v.qualified_name == name || v.qualified_name.contains(name) {
                    matches.push(SymbolMatch {
                        qualified_name: v.qualified_name.clone(),
                        file_path: file_path.clone(),
                        line_start: v.line_start,
                        line_end: v.line_end,
                    });
                }
            }
        }
    }

    Ok(matches)
}

pub fn run(name: &str, kind: Option<&str>) -> ExitCode {
    let index = match index::load_index() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let matches = match find_symbols(&index, name, kind) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if matches.is_empty() {
        eprintln!("No symbol found matching '{name}'");
        return ExitCode::FAILURE;
    }

    let multiple = matches.len() > 1;

    for (i, m) in matches.iter().enumerate() {
        if multiple {
            if i > 0 {
                println!();
            }
            println!("--- {} ({}:{}-{}) ---", m.qualified_name, m.file_path, m.line_start, m.line_end);
        }
        print_source(&m.file_path, m.line_start, m.line_end);
    }

    ExitCode::SUCCESS
}
