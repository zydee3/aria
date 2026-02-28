use tree_sitter::Parser;

use crate::index::{CallSite, FileEntry, Function, Scope, TypeDef, TypeKind, Variable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Language {
    Go,
    Rust,
    C,
}

pub struct GoParser {
    parser: Parser,
}

impl GoParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .expect("failed to load Go grammar");
        Self { parser }
    }

    pub fn parse_file(&mut self, source: &str, path: &str) -> Option<FileEntry> {
        let tree = self.parser.parse(source, None)?;
        let root = tree.root_node();

        let mut functions = Vec::new();
        let mut types = Vec::new();

        // Extract package name for qualified names
        let package_name = self.extract_package_name(&root, source.as_bytes());

        // Use directory path as prefix to disambiguate packages with same name in different locations
        // e.g., "internal/foo/initializer/init.go" -> "internal/foo/initializer"
        // This mirrors Go's import path behavior
        let path_prefix = path_to_prefix(path);

        // For init functions, we need file-level disambiguation even within same package
        let file_suffix = path_to_file_suffix(path);

        // Walk top-level declarations
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(func) = self.extract_function(&child, source.as_bytes(), &package_name, &path_prefix, &file_suffix, None) {
                        functions.push(func);
                    }
                }
                "method_declaration" => {
                    if let Some(func) = self.extract_method(&child, source.as_bytes(), &package_name, &path_prefix) {
                        functions.push(func);
                    }
                }
                "type_declaration" => {
                    self.extract_types(&child, source.as_bytes(), &package_name, &path_prefix, &mut types);
                }
                _ => {}
            }
        }

        let ast_hash = format!("{:016x}", hash_bytes(source.as_bytes()));

        Some(FileEntry {
            ast_hash,
            functions,
            types,
            variables: Vec::new(),
        })
    }

    fn extract_package_name(&self, root: &tree_sitter::Node, source: &[u8]) -> String {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "package_clause" {
                // package_clause contains package_identifier as child
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "package_identifier" {
                        return node_text(&inner, source).to_string();
                    }
                }
            }
        }
        String::new()
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        package: &str,
        path_prefix: &str,
        file_suffix: &str,
        receiver: Option<String>,
    ) -> Option<Function> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        // Build the base prefix (path_prefix preferred, fallback to package)
        let base = if !path_prefix.is_empty() {
            path_prefix
        } else {
            package
        };

        // Build qualified name
        let qualified_name = if name == "init" && !file_suffix.is_empty() {
            format!("{}.init@{}", base, file_suffix)
        } else if let Some(ref recv) = receiver {
            if base.is_empty() {
                format!("{}.{}", recv, name)
            } else {
                format!("{}.{}.{}", base, recv, name)
            }
        } else if base.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", base, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        // Build signature from parameters and result
        let signature = self.build_function_signature(node, source, &name);

        // In Go, public = starts with uppercase
        let scope = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Scope::Public
        } else {
            Scope::Internal
        };

        // Extract call sites from function body
        let calls = if let Some(body) = node.child_by_field_name("body") {
            extract_calls(&body, source)
        } else {
            Vec::new()
        };

        // Compute AST hash from the function's source bytes
        let func_source = &source[node.start_byte()..node.end_byte()];
        let ast_hash = format!("{:016x}", hash_bytes(func_source));

        Some(Function {
            name,
            qualified_name,
            ast_hash,
            line_start,
            line_end,
            signature,
            summary: None,
            receiver,
            scope,
            calls,
            called_by: Vec::new(),
        })
    }

    fn extract_method(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        package: &str,
        path_prefix: &str,
    ) -> Option<Function> {
        let receiver_node = node.child_by_field_name("receiver")?;
        let receiver_type = self.extract_receiver_type(&receiver_node, source)?;
        self.extract_function(node, source, package, path_prefix, "", Some(receiver_type))
    }

    fn extract_receiver_type(&self, receiver_node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        // receiver is a parameter_list with one parameter
        let mut cursor = receiver_node.walk();
        for child in receiver_node.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                // Get the type (last child that's a type)
                if let Some(type_node) = child.child_by_field_name("type") {
                    return Some(self.extract_type_name(&type_node, source));
                }
            }
        }
        None
    }

    fn extract_type_name(&self, type_node: &tree_sitter::Node, source: &[u8]) -> String {
        match type_node.kind() {
            "pointer_type" => {
                // *Type -> extract inner type
                if let Some(inner) = type_node.child(1) {
                    self.extract_type_name(&inner, source)
                } else {
                    node_text(type_node, source).to_string()
                }
            }
            "type_identifier" => node_text(type_node, source).to_string(),
            _ => node_text(type_node, source).to_string(),
        }
    }

    fn build_function_signature(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        name: &str,
    ) -> String {
        let params = node
            .child_by_field_name("parameters")
            .map(|n| node_text(&n, source))
            .unwrap_or("()");

        let result = node
            .child_by_field_name("result")
            .map(|n| format!(" {}", node_text(&n, source)))
            .unwrap_or_default();

        format!("func {}{}{}", name, params, result)
    }

    fn extract_types(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        package: &str,
        path_prefix: &str,
        types: &mut Vec<TypeDef>,
    ) {
        // type_declaration contains type_spec children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_spec" {
                if let Some(type_def) = self.extract_type_spec(&child, source, package, path_prefix) {
                    types.push(type_def);
                }
            }
        }
    }

    fn extract_type_spec(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        package: &str,
        path_prefix: &str,
    ) -> Option<TypeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let type_node = node.child_by_field_name("type")?;
        let kind = match type_node.kind() {
            "struct_type" => TypeKind::Struct,
            "interface_type" => TypeKind::Interface,
            _ => TypeKind::Typedef,
        };

        // Use path_prefix (directory path) to disambiguate packages with same name
        let qualified_name = if !path_prefix.is_empty() {
            format!("{}.{}", path_prefix, name)
        } else if package.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", package, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(), // TODO: populate from method declarations
        })
    }

}

fn node_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// Extract all call sites from an AST node (shared across all parsers)
fn extract_calls(node: &tree_sitter::Node, source: &[u8]) -> Vec<CallSite> {
    let mut calls = Vec::new();
    collect_calls(node, source, &mut calls);
    calls
}

/// Recursively collect call_expression nodes
fn collect_calls(node: &tree_sitter::Node, source: &[u8], calls: &mut Vec<CallSite>) {
    if node.kind() == "call_expression" {
        if let Some(func_node) = node.child_by_field_name("function") {
            let raw = node_text(&func_node, source).to_string();
            let line = node.start_position().row as u32 + 1;
            calls.push(CallSite {
                target: "[unresolved]".to_string(),
                raw,
                line,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls(&child, source, calls);
    }
}

/// Convert a file path to a prefix for qualified names.
/// e.g., "./cmd/foo/main.go" -> "cmd/foo"
/// e.g., "internal/bar/main.go" -> "internal/bar"
fn path_to_prefix(path: &str) -> String {
    // Remove leading "./"
    let path = path.strip_prefix("./").unwrap_or(path);

    // Remove the filename, keep directory
    if let Some(dir) = std::path::Path::new(path).parent() {
        let dir_str = dir.to_string_lossy();
        if dir_str.is_empty() || dir_str == "." {
            // File is in root, use filename without extension
            std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            dir_str.to_string()
        }
    } else {
        String::new()
    }
}

/// Convert a file path to a suffix for init function disambiguation.
/// e.g., "./internal/foo/bar.go" -> "bar"
fn path_to_file_suffix(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn hash_bytes(input: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// Rust Parser
// ============================================================================

pub struct RustParser {
    parser: Parser,
}

impl RustParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("failed to load Rust grammar");
        Self { parser }
    }

    pub fn parse_file(&mut self, source: &str, path: &str) -> Option<FileEntry> {
        let tree = self.parser.parse(source, None)?;
        let root = tree.root_node();

        let mut functions = Vec::new();
        let mut types = Vec::new();

        // Use module path from file location for qualified names
        // e.g., "src/parser.rs" -> "parser", "src/commands/index.rs" -> "commands::index"
        let module_path = rust_path_to_module(path);

        // Walk top-level declarations
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_item" => {
                    if let Some(func) = self.extract_function(&child, source.as_bytes(), &module_path, None) {
                        functions.push(func);
                    }
                }
                "impl_item" => {
                    self.extract_impl_functions(&child, source.as_bytes(), &module_path, &mut functions);
                }
                "struct_item" => {
                    if let Some(t) = self.extract_struct(&child, source.as_bytes(), &module_path) {
                        types.push(t);
                    }
                }
                "enum_item" => {
                    if let Some(t) = self.extract_enum(&child, source.as_bytes(), &module_path) {
                        types.push(t);
                    }
                }
                "trait_item" => {
                    if let Some(t) = self.extract_trait(&child, source.as_bytes(), &module_path) {
                        types.push(t);
                    }
                }
                "mod_item" => {
                    // Handle inline modules: mod foo { ... }
                    self.extract_mod_contents(&child, source.as_bytes(), &module_path, &mut functions, &mut types);
                }
                _ => {}
            }
        }

        let ast_hash = format!("{:016x}", hash_bytes(source.as_bytes()));

        Some(FileEntry {
            ast_hash,
            functions,
            types,
            variables: Vec::new(),
        })
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        module_path: &str,
        impl_type: Option<&str>,
    ) -> Option<Function> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        // Build qualified name
        let qualified_name = match impl_type {
            Some(t) => {
                if module_path.is_empty() {
                    format!("{}::{}", t, name)
                } else {
                    format!("{}::{}::{}", module_path, t, name)
                }
            }
            None => {
                if module_path.is_empty() {
                    name.clone()
                } else {
                    format!("{}::{}", module_path, name)
                }
            }
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        // Build signature
        let signature = self.build_rust_signature(node, source, &name);

        // Determine visibility
        let scope = self.extract_visibility(node);

        // Extract call sites from function body
        let calls = if let Some(body) = node.child_by_field_name("body") {
            extract_calls(&body, source)
        } else {
            Vec::new()
        };

        // Compute AST hash
        let func_source = &source[node.start_byte()..node.end_byte()];
        let ast_hash = format!("{:016x}", hash_bytes(func_source));

        Some(Function {
            name,
            qualified_name,
            ast_hash,
            line_start,
            line_end,
            signature,
            summary: None,
            receiver: impl_type.map(String::from),
            scope,
            calls,
            called_by: Vec::new(),
        })
    }

    fn extract_impl_functions(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        module_path: &str,
        functions: &mut Vec<Function>,
    ) {
        // Get the type being implemented
        let impl_type = node
            .child_by_field_name("type")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_default();

        // Strip pointer/reference from type if present (e.g., "&mut Foo" -> "Foo")
        let impl_type = impl_type
            .trim_start_matches('&')
            .trim_start_matches("mut ")
            .trim()
            .to_string();

        // Find the body (declaration_list)
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" {
                if let Some(func) = self.extract_function(&child, source, module_path, Some(&impl_type)) {
                    functions.push(func);
                }
            }
        }
    }

    fn extract_struct(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        module_path: &str,
    ) -> Option<TypeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let qualified_name = if module_path.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", module_path, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Struct,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_enum(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        module_path: &str,
    ) -> Option<TypeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let qualified_name = if module_path.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", module_path, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Enum,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_trait(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        module_path: &str,
    ) -> Option<TypeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let qualified_name = if module_path.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", module_path, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Interface, // Trait is closest to Interface
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_mod_contents(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        parent_module: &str,
        functions: &mut Vec<Function>,
        types: &mut Vec<TypeDef>,
    ) {
        // Get module name
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let mod_name = node_text(&name_node, source);

        // Build nested module path
        let nested_path = if parent_module.is_empty() {
            mod_name.to_string()
        } else {
            format!("{}::{}", parent_module, mod_name)
        };

        // Find the body (declaration_list)
        let Some(body) = node.child_by_field_name("body") else {
            return; // External mod declaration (mod foo;)
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_item" => {
                    if let Some(func) = self.extract_function(&child, source, &nested_path, None) {
                        functions.push(func);
                    }
                }
                "impl_item" => {
                    self.extract_impl_functions(&child, source, &nested_path, functions);
                }
                "struct_item" => {
                    if let Some(t) = self.extract_struct(&child, source, &nested_path) {
                        types.push(t);
                    }
                }
                "enum_item" => {
                    if let Some(t) = self.extract_enum(&child, source, &nested_path) {
                        types.push(t);
                    }
                }
                "trait_item" => {
                    if let Some(t) = self.extract_trait(&child, source, &nested_path) {
                        types.push(t);
                    }
                }
                "mod_item" => {
                    self.extract_mod_contents(&child, source, &nested_path, functions, types);
                }
                _ => {}
            }
        }
    }

    fn build_rust_signature(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        name: &str,
    ) -> String {
        let params = node
            .child_by_field_name("parameters")
            .map(|n| node_text(&n, source))
            .unwrap_or("()");

        let return_type = node
            .child_by_field_name("return_type")
            .map(|n| format!(" -> {}", node_text(&n, source)))
            .unwrap_or_default();

        format!("fn {}{}{}", name, params, return_type)
    }

    fn extract_visibility(&self, node: &tree_sitter::Node) -> Scope {
        // Check for visibility modifier (pub, pub(crate), etc.)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                return Scope::Public;
            }
        }
        Scope::Internal
    }
}

/// Convert Rust file path to module path
/// "src/parser.rs" -> "parser"
/// "src/commands/index.rs" -> "commands::index"
/// "src/lib.rs" -> ""
/// "src/main.rs" -> ""
fn rust_path_to_module(path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    let path = path.strip_prefix("src/").unwrap_or(path);

    // Remove .rs extension
    let path = path.strip_suffix(".rs").unwrap_or(path);

    // lib.rs and main.rs are crate roots
    if path == "lib" || path == "main" {
        return String::new();
    }

    // mod.rs files use parent directory name
    if path.ends_with("/mod") {
        let parent = &path[..path.len() - 4];
        return parent.replace('/', "::");
    }

    path.replace('/', "::")
}

// ============================================================================
// C Parser
// ============================================================================

pub struct CParser {
    parser: Parser,
}

impl CParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .expect("failed to load C grammar");
        Self { parser }
    }

    pub fn parse_file(&mut self, source: &str, path: &str) -> Option<FileEntry> {
        let tree = self.parser.parse(source, None)?;
        let root = tree.root_node();

        let mut functions = Vec::new();
        let mut types = Vec::new();
        let mut variables = Vec::new();

        // Use file path as prefix for qualified names (C has no module/package system)
        // e.g., "src/auth/login.c" -> "src/auth/login"
        let path_prefix = c_path_to_prefix(path);

        // Walk declarations (recursively enters preprocessor blocks)
        self.extract_declarations(&root, source.as_bytes(), &path_prefix, &mut functions, &mut types, &mut variables);

        let ast_hash = format!("{:016x}", hash_bytes(source.as_bytes()));

        Some(FileEntry {
            ast_hash,
            functions,
            types,
            variables,
        })
    }

    fn extract_declarations(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        path_prefix: &str,
        functions: &mut Vec<Function>,
        types: &mut Vec<TypeDef>,
        variables: &mut Vec<Variable>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    if let Some(func) = self.extract_function(&child, source, path_prefix) {
                        functions.push(func);
                    }
                }
                "struct_specifier" => {
                    if let Some(t) = self.extract_struct(&child, source, path_prefix) {
                        types.push(t);
                    }
                }
                "enum_specifier" => {
                    if let Some(t) = self.extract_enum(&child, source, path_prefix) {
                        types.push(t);
                    }
                }
                "type_definition" => {
                    if let Some(t) = self.extract_typedef(&child, source, path_prefix) {
                        types.push(t);
                    }
                }
                "declaration" => {
                    // Check for struct/enum inside declaration
                    self.extract_types_from_declaration(&child, source, path_prefix, types);
                    // Extract global variables
                    if let Some(var) = self.extract_variable(&child, source, path_prefix) {
                        variables.push(var);
                    }
                }
                // Recurse into preprocessor blocks
                "preproc_ifdef" | "preproc_ifndef" | "preproc_if" | "preproc_else" | "preproc_elif" => {
                    self.extract_declarations(&child, source, path_prefix, functions, types, variables);
                }
                _ => {}
            }
        }
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        path_prefix: &str,
    ) -> Option<Function> {
        // function_definition has declarator field which contains the function name
        let declarator = node.child_by_field_name("declarator")?;
        let name = self.extract_declarator_name(&declarator, source)?;

        // Build qualified name using file path as prefix
        // Use "." separator for consistency with resolver (Go-style)
        let qualified_name = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", path_prefix, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        // Build signature from return type + declarator
        let signature = self.build_c_signature(node, source);

        // Determine scope: static = Internal (file-local), otherwise Public (external linkage)
        let scope = self.extract_c_scope(node, source);

        // Extract call sites from function body
        let calls = if let Some(body) = node.child_by_field_name("body") {
            extract_calls(&body, source)
        } else {
            Vec::new()
        };

        // Compute AST hash
        let func_source = &source[node.start_byte()..node.end_byte()];
        let ast_hash = format!("{:016x}", hash_bytes(func_source));

        Some(Function {
            name,
            qualified_name,
            ast_hash,
            line_start,
            line_end,
            signature,
            summary: None,
            receiver: None,
            scope,
            calls,
            called_by: Vec::new(),
        })
    }

    fn extract_declarator_name(&self, declarator: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        // Handle function_declarator which wraps the identifier
        match declarator.kind() {
            "function_declarator" => {
                // function_declarator has 'declarator' field which is typically an identifier
                let inner = declarator.child_by_field_name("declarator")?;
                self.extract_declarator_name(&inner, source)
            }
            "pointer_declarator" => {
                // pointer_declarator has 'declarator' field
                let inner = declarator.child_by_field_name("declarator")?;
                self.extract_declarator_name(&inner, source)
            }
            "identifier" => Some(node_text(declarator, source).to_string()),
            _ => {
                // Try to find an identifier child
                let mut cursor = declarator.walk();
                for child in declarator.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        return Some(node_text(&child, source).to_string());
                    }
                }
                None
            }
        }
    }

    fn build_c_signature(&self, node: &tree_sitter::Node, source: &[u8]) -> String {
        // Get return type from 'type' field
        let return_type = node
            .child_by_field_name("type")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_default();

        // Get the full declarator (includes name and parameters)
        let declarator = node
            .child_by_field_name("declarator")
            .map(|n| node_text(&n, source).to_string())
            .unwrap_or_default();

        if return_type.is_empty() {
            declarator
        } else {
            format!("{} {}", return_type, declarator)
        }
    }

    fn extract_c_scope(&self, node: &tree_sitter::Node, source: &[u8]) -> Scope {
        // Check for storage_class_specifier with "static"
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "storage_class_specifier" {
                let text = node_text(&child, source);
                if text == "static" {
                    return Scope::Internal;
                }
            }
        }
        // Default: external linkage = Public
        Scope::Public
    }

    fn extract_struct(&self, node: &tree_sitter::Node, source: &[u8], path_prefix: &str) -> Option<TypeDef> {
        // struct_specifier has optional 'name' field (type_identifier)
        // For definitions, the name is in a type_identifier child
        // struct_specifier -> "struct" type_identifier? field_declaration_list?
        let mut name = None;
        let mut has_body = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" => {
                    name = Some(node_text(&child, source).to_string());
                }
                "field_declaration_list" => {
                    has_body = true;
                }
                _ => {}
            }
        }

        // Only extract struct definitions (with body), not type references
        if !has_body {
            return None;
        }

        let name = name?;

        let qualified_name = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", path_prefix, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Struct,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_enum(&self, node: &tree_sitter::Node, source: &[u8], path_prefix: &str) -> Option<TypeDef> {
        // enum_specifier has optional 'name' field (type_identifier)
        // enum_specifier -> "enum" type_identifier? enumerator_list?
        let mut name = None;
        let mut has_body = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" => {
                    name = Some(node_text(&child, source).to_string());
                }
                "enumerator_list" => {
                    has_body = true;
                }
                _ => {}
            }
        }

        // Only extract enum definitions (with body), not type references
        if !has_body {
            return None;
        }

        let name = name?;

        let qualified_name = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", path_prefix, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Enum,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_typedef(&self, node: &tree_sitter::Node, source: &[u8], path_prefix: &str) -> Option<TypeDef> {
        // type_definition: typedef <type> <name>
        // The name is a type_identifier child (not a field)
        let mut cursor = node.walk();
        let mut name = None;
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" {
                name = Some(node_text(&child, source).to_string());
                break;
            }
        }
        let name = name?;

        let qualified_name = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", path_prefix, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        Some(TypeDef {
            name,
            qualified_name,
            kind: TypeKind::Typedef,
            line_start,
            line_end,
            summary: None,
            methods: Vec::new(),
        })
    }

    fn extract_types_from_declaration(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        path_prefix: &str,
        types: &mut Vec<TypeDef>,
    ) {
        // Declarations may contain struct/enum definitions
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "struct_specifier" => {
                    if let Some(t) = self.extract_struct(&child, source, path_prefix) {
                        types.push(t);
                    }
                }
                "enum_specifier" => {
                    if let Some(t) = self.extract_enum(&child, source, path_prefix) {
                        types.push(t);
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_variable(&self, node: &tree_sitter::Node, source: &[u8], path_prefix: &str) -> Option<Variable> {
        // declaration has: type specifier(s), declarator(s)
        // We look for init_declarator or plain declarator with an identifier
        let mut cursor = node.walk();

        // Collect type parts and find declarator
        let mut type_parts = Vec::new();
        let mut var_name = None;
        let mut is_static = false;
        let mut is_extern = false;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "storage_class_specifier" => {
                    let text = node_text(&child, source);
                    if text == "static" {
                        is_static = true;
                    } else if text == "extern" {
                        is_extern = true;
                    }
                }
                "type_qualifier" | "primitive_type" => {
                    type_parts.push(node_text(&child, source).to_string());
                }
                "struct_specifier" => {
                    // e.g., "struct cr_fd_desc_tmpl"
                    let struct_text = node_text(&child, source);
                    type_parts.push(struct_text.to_string());
                }
                "enum_specifier" => {
                    let enum_text = node_text(&child, source);
                    type_parts.push(enum_text.to_string());
                }
                "type_identifier" => {
                    type_parts.push(node_text(&child, source).to_string());
                }
                "init_declarator" => {
                    // init_declarator contains the variable name (possibly nested in array_declarator, pointer_declarator)
                    var_name = self.extract_variable_name_from_declarator(&child, source);
                }
                "identifier" => {
                    // Simple declaration without initializer: "int x;"
                    if var_name.is_none() {
                        var_name = Some(node_text(&child, source).to_string());
                    }
                }
                "array_declarator" | "pointer_declarator" => {
                    // Declaration without init: "int arr[10];" or "int *ptr;"
                    if var_name.is_none() {
                        var_name = self.extract_variable_name_from_declarator(&child, source);
                    }
                }
                _ => {}
            }
        }

        let name = var_name?;

        // Skip extern declarations (only index definitions)
        if is_extern {
            return None;
        }

        // Skip if no type info (probably a function pointer or something complex)
        if type_parts.is_empty() {
            return None;
        }

        let type_name = type_parts.join(" ");

        let qualified_name = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", path_prefix, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        let scope = if is_static { Scope::Static } else { Scope::Public };

        Some(Variable {
            name,
            qualified_name,
            type_name,
            line_start,
            line_end,
            scope,
        })
    }

    fn extract_variable_name_from_declarator(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        // Recursively find the identifier inside a declarator
        match node.kind() {
            "identifier" => Some(node_text(node, source).to_string()),
            "init_declarator" | "array_declarator" | "pointer_declarator" => {
                // Look for declarator field or identifier child
                if let Some(decl) = node.child_by_field_name("declarator") {
                    return self.extract_variable_name_from_declarator(&decl, source);
                }
                // Otherwise search children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.extract_variable_name_from_declarator(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
            _ => None,
        }
    }

}

/// Convert C file path to a prefix for qualified names.
/// e.g., "src/auth/login.c" -> "src/auth/login"
/// e.g., "./include/utils.h" -> "include/utils"
fn c_path_to_prefix(path: &str) -> String {
    // Remove leading "./"
    let path = path.strip_prefix("./").unwrap_or(path);

    // Remove .c or .h extension
    let path = path.strip_suffix(".c").or_else(|| path.strip_suffix(".h")).unwrap_or(path);

    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let source = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}
"#;
        let mut parser = GoParser::new();
        let entry = parser.parse_file(source, "main.go").unwrap();

        assert_eq!(entry.functions.len(), 1);
        let f = &entry.functions[0];
        assert_eq!(f.name, "Hello");
        assert_eq!(f.qualified_name, "main.Hello");
        assert_eq!(f.scope, Scope::Public);
        assert!(f.signature.contains("func Hello(name string) string"));
    }

    #[test]
    fn test_parse_method() {
        let source = r#"
package server

type Server struct {
    addr string
}

func (s *Server) Start() error {
    return nil
}
"#;
        let mut parser = GoParser::new();
        let entry = parser.parse_file(source, "server.go").unwrap();

        assert_eq!(entry.functions.len(), 1);
        let f = &entry.functions[0];
        assert_eq!(f.name, "Start");
        assert_eq!(f.qualified_name, "server.Server.Start");
        assert_eq!(f.receiver, Some("Server".to_string()));

        assert_eq!(entry.types.len(), 1);
        let t = &entry.types[0];
        assert_eq!(t.name, "Server");
        assert_eq!(t.kind, TypeKind::Struct);
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
package main

import "fmt"

func greet(name string) {
    fmt.Println("Hello, " + name)
}

func main() {
    greet("world")
    fmt.Printf("Done\n")
}
"#;
        let mut parser = GoParser::new();
        let entry = parser.parse_file(source, "main.go").unwrap();

        assert_eq!(entry.functions.len(), 2);

        // greet has one call: fmt.Println
        let greet = entry.functions.iter().find(|f| f.name == "greet").unwrap();
        assert_eq!(greet.calls.len(), 1);
        assert_eq!(greet.calls[0].raw, "fmt.Println");
        assert_eq!(greet.calls[0].target, "[unresolved]");

        // main has two calls: greet and fmt.Printf
        let main_fn = entry.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(main_fn.calls.len(), 2);
        assert_eq!(main_fn.calls[0].raw, "greet");
        assert_eq!(main_fn.calls[1].raw, "fmt.Printf");
    }

    #[test]
    fn test_extract_method_calls() {
        let source = r#"
package server

type Server struct {
    logger Logger
}

func (s *Server) Start() error {
    s.logger.Info("starting")
    s.init()
    return nil
}

func (s *Server) init() {}
"#;
        let mut parser = GoParser::new();
        let entry = parser.parse_file(source, "server.go").unwrap();

        let start = entry.functions.iter().find(|f| f.name == "Start").unwrap();
        assert_eq!(start.calls.len(), 2);
        assert_eq!(start.calls[0].raw, "s.logger.Info");
        assert_eq!(start.calls[1].raw, "s.init");
    }

    // ========================================================================
    // Rust Parser Tests
    // ========================================================================

    #[test]
    fn test_rust_parse_simple_function() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}
"#;
        let mut parser = RustParser::new();
        let entry = parser.parse_file(source, "src/lib.rs").unwrap();

        assert_eq!(entry.functions.len(), 1);
        let f = &entry.functions[0];
        assert_eq!(f.name, "hello");
        assert_eq!(f.qualified_name, "hello");
        assert_eq!(f.scope, Scope::Public);
        assert!(f.signature.contains("fn hello"));
    }

    #[test]
    fn test_rust_parse_impl_methods() {
        let source = r#"
pub struct Server {
    addr: String,
}

impl Server {
    pub fn new(addr: String) -> Self {
        Self { addr }
    }

    pub fn start(&self) -> Result<(), Error> {
        Ok(())
    }

    fn internal_method(&self) {}
}
"#;
        let mut parser = RustParser::new();
        let entry = parser.parse_file(source, "src/server.rs").unwrap();

        assert_eq!(entry.functions.len(), 3);
        assert_eq!(entry.types.len(), 1);

        let new_fn = entry.functions.iter().find(|f| f.name == "new").unwrap();
        assert_eq!(new_fn.qualified_name, "server::Server::new");
        assert_eq!(new_fn.receiver, Some("Server".to_string()));
        assert_eq!(new_fn.scope, Scope::Public);

        let start_fn = entry.functions.iter().find(|f| f.name == "start").unwrap();
        assert_eq!(start_fn.qualified_name, "server::Server::start");

        let internal = entry.functions.iter().find(|f| f.name == "internal_method").unwrap();
        assert_eq!(internal.scope, Scope::Internal);

        let server_type = &entry.types[0];
        assert_eq!(server_type.name, "Server");
        assert_eq!(server_type.kind, TypeKind::Struct);
    }

    #[test]
    fn test_rust_extract_calls() {
        let source = r#"
fn greet(name: &str) {
    println!("Hello, {}", name);
}

fn main() {
    greet("world");
    println!("Done");
}
"#;
        let mut parser = RustParser::new();
        let entry = parser.parse_file(source, "src/main.rs").unwrap();

        assert_eq!(entry.functions.len(), 2);

        // greet has one macro call (println!) which isn't a call_expression
        let greet = entry.functions.iter().find(|f| f.name == "greet").unwrap();
        assert_eq!(greet.calls.len(), 0); // macros aren't call_expressions

        // main has one function call: greet
        let main_fn = entry.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(main_fn.calls.len(), 1);
        assert_eq!(main_fn.calls[0].raw, "greet");
    }

    #[test]
    fn test_rust_parse_enum_and_trait() {
        let source = r#"
pub enum Status {
    Active,
    Inactive,
}

pub trait Handler {
    fn handle(&self);
}
"#;
        let mut parser = RustParser::new();
        let entry = parser.parse_file(source, "src/types.rs").unwrap();

        assert_eq!(entry.types.len(), 2);

        let status = entry.types.iter().find(|t| t.name == "Status").unwrap();
        assert_eq!(status.kind, TypeKind::Enum);
        assert_eq!(status.qualified_name, "types::Status");

        let handler = entry.types.iter().find(|t| t.name == "Handler").unwrap();
        assert_eq!(handler.kind, TypeKind::Interface);
        assert_eq!(handler.qualified_name, "types::Handler");
    }

    #[test]
    fn test_rust_module_path() {
        assert_eq!(rust_path_to_module("src/lib.rs"), "");
        assert_eq!(rust_path_to_module("src/main.rs"), "");
        assert_eq!(rust_path_to_module("src/parser.rs"), "parser");
        assert_eq!(rust_path_to_module("src/commands/index.rs"), "commands::index");
        assert_eq!(rust_path_to_module("./src/foo/bar.rs"), "foo::bar");
        assert_eq!(rust_path_to_module("src/utils/mod.rs"), "utils");
    }

    // ========================================================================
    // C Parser Tests
    // ========================================================================

    #[test]
    fn test_c_parse_simple_function() {
        let source = r#"
int add(int a, int b) {
    return a + b;
}
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "src/math.c").unwrap();

        assert_eq!(entry.functions.len(), 1);
        let f = &entry.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(f.qualified_name, "src/math.add");
        assert_eq!(f.scope, Scope::Public);
        assert!(f.signature.contains("int"));
        assert!(f.signature.contains("add"));
    }

    #[test]
    fn test_c_parse_static_function() {
        let source = r#"
static void helper(void) {
    // internal helper
}

void public_func(void) {
    helper();
}
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "utils.c").unwrap();

        assert_eq!(entry.functions.len(), 2);

        let helper = entry.functions.iter().find(|f| f.name == "helper").unwrap();
        assert_eq!(helper.scope, Scope::Internal);

        let public_fn = entry.functions.iter().find(|f| f.name == "public_func").unwrap();
        assert_eq!(public_fn.scope, Scope::Public);
    }

    #[test]
    fn test_c_parse_struct() {
        let source = r#"
struct Point {
    int x;
    int y;
};
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "src/types.h").unwrap();

        assert_eq!(entry.types.len(), 1);
        let t = &entry.types[0];
        assert_eq!(t.name, "Point");
        assert_eq!(t.qualified_name, "src/types.Point");
        assert_eq!(t.kind, TypeKind::Struct);
    }

    #[test]
    fn test_c_parse_enum() {
        let source = r#"
enum Status {
    ACTIVE,
    INACTIVE,
    PENDING
};
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "status.h").unwrap();

        assert_eq!(entry.types.len(), 1);
        let t = &entry.types[0];
        assert_eq!(t.name, "Status");
        assert_eq!(t.kind, TypeKind::Enum);
    }

    #[test]
    fn test_c_parse_typedef() {
        let source = r#"
typedef unsigned int uint32;
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "types.h").unwrap();

        assert_eq!(entry.types.len(), 1);
        let t = &entry.types[0];
        assert_eq!(t.name, "uint32");
        assert_eq!(t.kind, TypeKind::Typedef);
    }

    #[test]
    fn test_c_extract_calls() {
        let source = r#"
void greet(const char* name) {
    printf("Hello, %s\n", name);
}

int main(void) {
    greet("world");
    return 0;
}
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "main.c").unwrap();

        assert_eq!(entry.functions.len(), 2);

        let greet = entry.functions.iter().find(|f| f.name == "greet").unwrap();
        assert_eq!(greet.calls.len(), 1);
        assert_eq!(greet.calls[0].raw, "printf");

        let main_fn = entry.functions.iter().find(|f| f.name == "main").unwrap();
        assert_eq!(main_fn.calls.len(), 1);
        assert_eq!(main_fn.calls[0].raw, "greet");
    }

    #[test]
    fn test_c_path_to_prefix() {
        assert_eq!(c_path_to_prefix("main.c"), "main");
        assert_eq!(c_path_to_prefix("src/utils.c"), "src/utils");
        assert_eq!(c_path_to_prefix("./include/types.h"), "include/types");
        assert_eq!(c_path_to_prefix("src/auth/login.c"), "src/auth/login");
    }

    #[test]
    fn test_c_parse_struct_with_include_guards() {
        let source = r#"
#ifndef __MY_HEADER_H__
#define __MY_HEADER_H__

struct cr_fd_desc_tmpl {
    const char *fmt;
    int magic;
    int oflags;
};

#endif
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "include/image-desc.h").unwrap();

        assert_eq!(entry.types.len(), 1, "Should find struct inside include guards");
        let t = &entry.types[0];
        assert_eq!(t.name, "cr_fd_desc_tmpl");
        assert_eq!(t.kind, TypeKind::Struct);
        assert_eq!(t.line_start, 5);
        assert_eq!(t.line_end, 9);
    }

    #[test]
    fn test_c_parse_global_variables() {
        let source = r#"
struct cr_fd_desc_tmpl {
    const char *fmt;
    int magic;
};

struct cr_fd_desc_tmpl imgset_template[10] = {
    { "inventory", 1 },
    { "fdinfo", 2 },
};

int global_count = 42;
static char *secret_key = "abc";
"#;
        let mut parser = CParser::new();
        let entry = parser.parse_file(source, "criu/image-desc.c").unwrap();

        // Should find struct
        assert_eq!(entry.types.len(), 1);
        assert_eq!(entry.types[0].name, "cr_fd_desc_tmpl");

        // Should find 3 global variables
        assert_eq!(entry.variables.len(), 3, "Should find 3 global variables");

        let imgset = entry.variables.iter().find(|v| v.name == "imgset_template").unwrap();
        assert_eq!(imgset.qualified_name, "criu/image-desc.imgset_template");
        assert!(imgset.type_name.contains("struct cr_fd_desc_tmpl"));
        assert_eq!(imgset.scope, Scope::Public);

        let count = entry.variables.iter().find(|v| v.name == "global_count").unwrap();
        assert_eq!(count.type_name, "int");
        assert_eq!(count.scope, Scope::Public);

        let secret = entry.variables.iter().find(|v| v.name == "secret_key").unwrap();
        assert_eq!(secret.scope, Scope::Static);
    }
}
