use tree_sitter::Parser;

use crate::index::{FileEntry, Function, Scope, TypeDef, TypeKind};

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

    pub fn parse_file(&mut self, source: &str, _path: &str) -> Option<FileEntry> {
        let tree = self.parser.parse(source, None)?;
        let root = tree.root_node();

        let mut functions = Vec::new();
        let mut types = Vec::new();

        // Extract package name for qualified names
        let package_name = self.extract_package_name(&root, source.as_bytes());

        // Walk top-level declarations
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(func) = self.extract_function(&child, source.as_bytes(), &package_name, None) {
                        functions.push(func);
                    }
                }
                "method_declaration" => {
                    if let Some(func) = self.extract_method(&child, source.as_bytes(), &package_name) {
                        functions.push(func);
                    }
                }
                "type_declaration" => {
                    self.extract_types(&child, source.as_bytes(), &package_name, &mut types);
                }
                _ => {}
            }
        }

        // TODO: compute actual AST hash
        let ast_hash = format!("{:x}", md5_hash(source));

        Some(FileEntry {
            ast_hash,
            functions,
            types,
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
        receiver: Option<String>,
    ) -> Option<Function> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let qualified_name = if package.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", package, name)
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

        // TODO: extract calls
        let calls = Vec::new();

        Some(Function {
            name,
            qualified_name,
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
    ) -> Option<Function> {
        // Get receiver
        let receiver_node = node.child_by_field_name("receiver")?;
        let receiver_type = self.extract_receiver_type(&receiver_node, source)?;

        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let qualified_name = if package.is_empty() {
            format!("{}.{}", receiver_type, name)
        } else {
            format!("{}.{}.{}", package, receiver_type, name)
        };

        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        let signature = self.build_function_signature(node, source, &name);

        let scope = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Scope::Public
        } else {
            Scope::Internal
        };

        Some(Function {
            name,
            qualified_name,
            line_start,
            line_end,
            signature,
            summary: None,
            receiver: Some(receiver_type),
            scope,
            calls: Vec::new(),
            called_by: Vec::new(),
        })
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
        types: &mut Vec<TypeDef>,
    ) {
        // type_declaration contains type_spec children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_spec" {
                if let Some(type_def) = self.extract_type_spec(&child, source, package) {
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
    ) -> Option<TypeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source).to_string();

        let type_node = node.child_by_field_name("type")?;
        let kind = match type_node.kind() {
            "struct_type" => TypeKind::Struct,
            "interface_type" => TypeKind::Interface,
            _ => TypeKind::Typedef,
        };

        let qualified_name = if package.is_empty() {
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

fn md5_hash(input: &str) -> u64 {
    // Simple hash for now - will replace with proper implementation
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
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
}
