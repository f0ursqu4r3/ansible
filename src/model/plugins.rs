use std::path::Path;

use tree_sitter::Language;

use super::highlight::{default_highlights, highlight_tree_sitter};
use super::parse::parse_tree_sitter;
use super::types::{HighlightSpan, ParsedComponents};

pub trait LanguagePlugin {
    fn matches(&self, path: &Path) -> bool;
    fn parse_and_highlight(
        &self,
        path: &Path,
        content: &str,
        lines: &[String],
    ) -> anyhow::Result<(ParsedComponents, Vec<Vec<HighlightSpan>>)>;
}

pub struct FallbackPlugin {
    pub exts: Option<&'static [&'static str]>,
}

impl LanguagePlugin for FallbackPlugin {
    fn matches(&self, path: &Path) -> bool {
        match self.exts {
            Some(exts) => path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| exts.contains(&e))
                .unwrap_or(false),
            None => true,
        }
    }

    fn parse_and_highlight(
        &self,
        _path: &Path,
        _content: &str,
        lines: &[String],
    ) -> anyhow::Result<(ParsedComponents, Vec<Vec<HighlightSpan>>)> {
        Ok((
            ParsedComponents {
                defs: Vec::new(),
                calls: Vec::new(),
            },
            default_highlights(lines),
        ))
    }
}

pub struct TreeSitterPlugin {
    pub exts: &'static [&'static str],
    pub language: Language,
    pub def_query: &'static str,
    pub call_query: &'static str,
    pub highlight_query: Option<&'static str>,
    pub jsx_highlight_query: Option<&'static str>,
}

impl TreeSitterPlugin {
    fn highlight_query_for(&self, path: &Path) -> Option<&'static str> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if ext.ends_with('x') {
            if let Some(q) = self.jsx_highlight_query {
                return Some(q);
            }
        }
        self.highlight_query
    }
}

impl LanguagePlugin for TreeSitterPlugin {
    fn matches(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.exts.contains(&ext))
            .unwrap_or(false)
    }

    fn parse_and_highlight(
        &self,
        path: &Path,
        content: &str,
        lines: &[String],
    ) -> anyhow::Result<(ParsedComponents, Vec<Vec<HighlightSpan>>)> {
        let (parts, tree) = parse_tree_sitter(
            path,
            content,
            &self.language,
            self.def_query,
            self.call_query,
        )?;
        let Some(query_src) = self.highlight_query_for(path) else {
            return Ok((parts, default_highlights(lines)));
        };
        let spans = highlight_tree_sitter(&self.language, &tree, query_src, content, lines)
            .unwrap_or_else(|_| default_highlights(lines));
        Ok((parts, spans))
    }
}

pub fn default_plugins() -> Vec<Box<dyn LanguagePlugin>> {
    vec![
        Box::new(TreeSitterPlugin {
            exts: &["rs"],
            language: tree_sitter_rust::LANGUAGE.into(),
            def_query: "
              (function_item name: (identifier) @name)
              (struct_item name: (type_identifier) @name)
              (enum_item name: (type_identifier) @name)
              (union_item name: (type_identifier) @name)
              (type_item name: (type_identifier) @name)
              (trait_item name: (type_identifier) @name)
              (impl_item type: (type_identifier) @name)
              (impl_item type: (scoped_type_identifier) @name)
            ",
            call_query: "
              (call_expression function: (identifier) @call)
              (call_expression function: (scoped_identifier name: (identifier) @call))
              (call_expression function: (field_expression field: (field_identifier) @call))
              (struct_expression name: (type_identifier) @call)
              (struct_expression name: (scoped_type_identifier) @call)
              (parameter type: (type_identifier) @call)
              (parameter type: (scoped_type_identifier) @call)
              (generic_type type: (type_identifier) @call)
              (generic_type type: (scoped_type_identifier) @call)
              (reference_type type: (type_identifier) @call)
              (reference_type type: (scoped_type_identifier) @call)
              (function_item return_type: (type_identifier) @call)
              (function_item return_type: (scoped_type_identifier) @call)
            ",
            highlight_query: Some(tree_sitter_rust::HIGHLIGHTS_QUERY),
            jsx_highlight_query: None,
        }),
        Box::new(TreeSitterPlugin {
            exts: &["py"],
            language: tree_sitter_python::LANGUAGE.into(),
            def_query: "
              (function_definition name: (identifier) @name)
              (class_definition name: (identifier) @name)
            ",
            call_query: "
              (call function: (identifier) @call)
              (call function: (attribute attribute: (identifier) @call))
            ",
            highlight_query: Some(tree_sitter_python::HIGHLIGHTS_QUERY),
            jsx_highlight_query: None,
        }),
        Box::new(TreeSitterPlugin {
            exts: &["js", "jsx"],
            language: tree_sitter_javascript::LANGUAGE.into(),
            def_query: "
              (function_declaration name: (identifier) @name)
              (method_definition name: (property_identifier) @name)
              (class_declaration name: (identifier) @name)
              (class name: (identifier) @name)
            ",
            call_query: "
              (call_expression function: (identifier) @call)
              (call_expression function: (member_expression property: (property_identifier) @call))
              (new_expression constructor: (identifier) @call)
              (new_expression constructor: (member_expression property: (property_identifier) @call))
            ",
            highlight_query: Some(tree_sitter_javascript::HIGHLIGHT_QUERY),
            jsx_highlight_query: Some(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY),
        }),
        Box::new(TreeSitterPlugin {
            exts: &["ts", "tsx"],
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            def_query: "
              (function_declaration name: (identifier) @name)
              (method_signature name: (property_identifier) @name)
              (method_definition name: (property_identifier) @name)
              (class_declaration name: (type_identifier) @name)
              (abstract_class_declaration name: (type_identifier) @name)
              (interface_declaration name: (type_identifier) @name)
              (enum_declaration name: (identifier) @name)
              (type_alias_declaration name: (type_identifier) @name)
            ",
            call_query: "
              (call_expression function: (identifier) @call)
              (call_expression function: (member_expression property: (property_identifier) @call))
              (new_expression constructor: (identifier) @call)
              (new_expression constructor: (member_expression property: (property_identifier) @call))
              (type_annotation (type_identifier) @call)
            ",
            highlight_query: Some(tree_sitter_typescript::HIGHLIGHTS_QUERY),
            jsx_highlight_query: None,
        }),
        Box::new(FallbackPlugin {
            exts: Some(&["go"]),
        }),
        Box::new(FallbackPlugin {
            exts: Some(&["c", "h", "cpp", "cc", "hpp", "cxx"]),
        }),
        Box::new(FallbackPlugin {
            exts: Some(&["kt", "kts"]),
        }),
        Box::new(FallbackPlugin {
            exts: Some(&["java"]),
        }),
        Box::new(FallbackPlugin {
            exts: Some(&["swift"]),
        }),
    ]
}
