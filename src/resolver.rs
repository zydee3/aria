use std::collections::HashMap;

use crate::index::{FileEntry, Index};

/// Resolves call targets to qualified names and populates called_by relationships
pub struct Resolver {
    /// Maps function names to their qualified names and file paths
    /// Key: simple name (e.g., "Foo") or receiver.name (e.g., "Server.Start")
    /// Value: Vec of (qualified_name, file_path) for potential matches
    symbol_table: HashMap<String, Vec<(String, String)>>,

    /// Maps qualified names to their file paths
    qualified_to_file: HashMap<String, String>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            symbol_table: HashMap::new(),
            qualified_to_file: HashMap::new(),
        }
    }

    /// Build symbol table from parsed files
    pub fn build_symbol_table(&mut self, files: &HashMap<String, FileEntry>) {
        self.symbol_table.clear();
        self.qualified_to_file.clear();

        for (file_path, entry) in files {
            for func in &entry.functions {
                // Map qualified name to file
                self.qualified_to_file
                    .insert(func.qualified_name.clone(), file_path.clone());

                // Add to symbol table by simple name
                self.symbol_table
                    .entry(func.name.clone())
                    .or_default()
                    .push((func.qualified_name.clone(), file_path.clone()));

                // Also add by receiver.name for methods (e.g., "Server.Start")
                if let Some(ref receiver) = func.receiver {
                    let method_key = format!("{}.{}", receiver, func.name);
                    self.symbol_table
                        .entry(method_key)
                        .or_default()
                        .push((func.qualified_name.clone(), file_path.clone()));
                }
            }
        }
    }

    /// Resolve all calls in the index and populate called_by
    pub fn resolve(&self, index: &mut Index) {
        // First pass: resolve call targets
        let mut calls_to_targets: HashMap<String, Vec<String>> = HashMap::new();

        for (file_path, entry) in index.files.iter_mut() {
            // Extract package from file path or first function's qualified name
            let package = entry
                .functions
                .first()
                .map(|f| extract_package(&f.qualified_name))
                .unwrap_or_default();

            for func in &mut entry.functions {
                for call in &mut func.calls {
                    let target = self.resolve_call(&call.raw, &package, file_path);
                    call.target = target.clone();

                    // Track for called_by population
                    if target != "[unresolved]" {
                        calls_to_targets
                            .entry(target)
                            .or_default()
                            .push(func.qualified_name.clone());
                    }
                }
            }
        }

        // Second pass: populate called_by
        for entry in index.files.values_mut() {
            for func in &mut entry.functions {
                if let Some(callers) = calls_to_targets.get(&func.qualified_name) {
                    func.called_by = callers.clone();
                    func.called_by.sort();
                    func.called_by.dedup();
                }
            }
        }
    }

    /// Resolve a single call expression to a qualified name
    fn resolve_call(&self, raw: &str, package: &str, _file_path: &str) -> String {
        // Handle different call patterns:
        // 1. Simple function call: "foo" -> look up in same package first
        // 2. Package-qualified: "pkg.Foo" -> look up pkg.Foo
        // 3. Method on receiver: "s.Method" or "obj.Method" -> harder to resolve without type info
        // 4. Chained calls: "s.logger.Info" -> extract final method

        let parts: Vec<&str> = raw.split('.').collect();

        match parts.len() {
            1 => {
                // Simple function call: look in same package first
                let name = parts[0];
                let same_pkg_qualified = format!("{}.{}", package, name);

                if self.qualified_to_file.contains_key(&same_pkg_qualified) {
                    same_pkg_qualified
                } else {
                    // Try finding any match
                    self.find_single_match(name)
                }
            }
            2 => {
                // Could be pkg.Func or receiver.Method
                let first = parts[0];
                let second = parts[1];

                // Try as package.Function first
                let as_pkg_func = format!("{}.{}", first, second);
                if self.qualified_to_file.contains_key(&as_pkg_func) {
                    return as_pkg_func;
                }

                // Try as Type.Method in same package
                let as_method = format!("{}.{}.{}", package, first, second);
                if self.qualified_to_file.contains_key(&as_method) {
                    return as_method;
                }

                // Try finding method by Type.Method pattern
                let type_method = format!("{}.{}", first, second);
                self.find_single_match(&type_method)
            }
            _ => {
                // Chained: s.logger.Info -> try to resolve last segment
                // This is a simplification; proper resolution needs type inference
                let last_two = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);
                self.find_single_match(&last_two)
            }
        }
    }

    /// Find a single match in symbol table, return [unresolved] if none or ambiguous
    fn find_single_match(&self, key: &str) -> String {
        match self.symbol_table.get(key) {
            Some(matches) if matches.len() == 1 => matches[0].0.clone(),
            _ => "[unresolved]".to_string(),
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract package from qualified name (e.g., "main.Foo" -> "main")
fn extract_package(qualified_name: &str) -> String {
    qualified_name
        .split('.')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{CallSite, Function, Scope};

    fn make_function(name: &str, qualified: &str, calls: Vec<CallSite>) -> Function {
        Function {
            name: name.to_string(),
            qualified_name: qualified.to_string(),
            line_start: 1,
            line_end: 10,
            signature: format!("func {}()", name),
            summary: None,
            receiver: None,
            scope: Scope::Public,
            calls,
            called_by: Vec::new(),
        }
    }

    fn make_call(raw: &str) -> CallSite {
        CallSite {
            target: "[unresolved]".to_string(),
            raw: raw.to_string(),
            line: 1,
        }
    }

    #[test]
    fn test_resolve_same_package_call() {
        let mut index = Index::new();

        let foo = make_function("foo", "main.foo", vec![]);
        let bar = make_function("bar", "main.bar", vec![make_call("foo")]);

        index.files.insert(
            "main.go".to_string(),
            FileEntry {
                ast_hash: "abc".to_string(),
                functions: vec![foo, bar],
                types: vec![],
            },
        );

        let mut resolver = Resolver::new();
        resolver.build_symbol_table(&index.files);
        resolver.resolve(&mut index);

        let entry = index.files.get("main.go").unwrap();
        let bar = entry.functions.iter().find(|f| f.name == "bar").unwrap();
        assert_eq!(bar.calls[0].target, "main.foo");

        let foo = entry.functions.iter().find(|f| f.name == "foo").unwrap();
        assert_eq!(foo.called_by, vec!["main.bar"]);
    }

    #[test]
    fn test_resolve_cross_package_call() {
        let mut index = Index::new();

        let helper = make_function("Helper", "utils.Helper", vec![]);
        let main_fn = make_function("main", "main.main", vec![make_call("utils.Helper")]);

        index.files.insert(
            "utils/helper.go".to_string(),
            FileEntry {
                ast_hash: "abc".to_string(),
                functions: vec![helper],
                types: vec![],
            },
        );
        index.files.insert(
            "main.go".to_string(),
            FileEntry {
                ast_hash: "def".to_string(),
                functions: vec![main_fn],
                types: vec![],
            },
        );

        let mut resolver = Resolver::new();
        resolver.build_symbol_table(&index.files);
        resolver.resolve(&mut index);

        let entry = index.files.get("main.go").unwrap();
        let main_fn = entry.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(main_fn.calls[0].target, "utils.Helper");
    }

    #[test]
    fn test_unresolved_external_call() {
        let mut index = Index::new();

        let main_fn = make_function("main", "main.main", vec![make_call("fmt.Println")]);

        index.files.insert(
            "main.go".to_string(),
            FileEntry {
                ast_hash: "abc".to_string(),
                functions: vec![main_fn],
                types: vec![],
            },
        );

        let mut resolver = Resolver::new();
        resolver.build_symbol_table(&index.files);
        resolver.resolve(&mut index);

        let entry = index.files.get("main.go").unwrap();
        let main_fn = entry.functions.iter().find(|f| f.name == "main").unwrap();
        // fmt is external, should remain unresolved
        assert_eq!(main_fn.calls[0].target, "[unresolved]");
    }
}
