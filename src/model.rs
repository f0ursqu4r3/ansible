use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tree_sitter::{
    Language, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};
use walkdir::WalkDir;

use crate::theme::Palette;
use raylib::prelude::Color as RayColor;

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub module_path: String,
    pub line: usize,
    pub col: usize,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub module_path: String,
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

#[derive(Clone, Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub defs: Vec<FunctionDef>,
    pub calls: Vec<FunctionCall>,
    pub spans: Vec<Vec<HighlightSpan>>,
}

impl ParsedFile {
    pub fn calls_on_line(&self, line: usize) -> impl Iterator<Item = &FunctionCall> {
        self.calls.iter().filter(move |c| c.line == line)
    }
}

#[derive(Clone, Debug)]
pub struct DefinitionLocation {
    pub file: PathBuf,
    pub module_path: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub kind: HighlightKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HighlightKind {
    Plain,
    Comment,
    String,
    Keyword,
    Function,
    Type,
    Constant,
    Number,
    Property,
    Operator,
    Builtin,
}

pub struct ProjectModel {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub parsed: HashMap<PathBuf, ParsedFile>,
    pub defs: HashMap<String, Vec<DefinitionLocation>>,
}

pub struct ParsedComponents {
    pub defs: Vec<FunctionDef>,
    pub calls: Vec<FunctionCall>,
}

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
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
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
        let spans =
            highlight_tree_sitter(&self.language, &tree, query_src, content, lines).unwrap_or_else(
                |_| default_highlights(lines),
            );
        Ok((parts, spans))
    }
}

impl ProjectModel {
    pub fn load(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let plugins: Vec<Box<dyn LanguagePlugin>> = vec![
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
        ];
        let mut files = Vec::new();
        for entry in WalkDir::new(&root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(name.starts_with(".git") || name == "target" || name == "data")
        }) {
            let entry = entry?;
            if entry.file_type().is_file() && plugins.iter().any(|p| p.matches(entry.path())) {
                files.push(entry.into_path());
            }
        }
        files.sort();

        let mut parsed = HashMap::new();
        let mut defs: HashMap<String, Vec<DefinitionLocation>> = HashMap::new();
        for file in &files {
            let bytes = match std::fs::read(file) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Skipping {}: {}", file.display(), e);
                    continue;
                }
            };
            let content = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("Skipping non-UTF8 file {}", file.display());
                    continue;
                }
            };
            let plugin = match plugins.iter().find(|p| p.matches(file)) {
                Some(plugin) => plugin,
                None => continue,
            };
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let (parts, spans) = plugin.parse_and_highlight(file, &content, &lines)?;
            let pf = ParsedFile {
                path: file.clone(),
                lines,
                defs: parts.defs,
                calls: parts.calls,
                spans,
            };
            for def in &pf.defs {
                defs.entry(def.name.clone())
                    .or_default()
                    .push(DefinitionLocation {
                        file: file.clone(),
                        module_path: def.module_path.clone(),
                        line: def.line,
                        col: def.col,
                    });
            }
            parsed.insert(file.clone(), pf);
        }

        Ok(Self {
            root,
            files,
            parsed,
            defs,
        })
    }

    pub fn display_name(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }
}

fn def_kind(node: Node) -> String {
    let mut n = node;
    for _ in 0..4 {
        let k = n.kind().to_string();
        if matches!(
            k.as_str(),
            "function_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "type_item"
                | "impl_item"
        ) {
            return k;
        }
        if let Some(p) = n.parent() {
            n = p;
        } else {
            break;
        }
    }
    n.kind().to_string()
}

fn default_highlights(lines: &[String]) -> Vec<Vec<HighlightSpan>> {
    lines
        .iter()
        .map(|line| {
            let len = line.len();
            vec![HighlightSpan {
                start: 0,
                end: len,
                kind: HighlightKind::Plain,
            }]
        })
        .collect()
}

#[derive(Clone, Debug)]
struct HighlightCapture {
    start_byte: usize,
    end_byte: usize,
    start_point: Point,
    end_point: Point,
    kind: HighlightKind,
}

fn highlight_tree_sitter(
    language: &Language,
    tree: &Tree,
    query_src: &str,
    content: &str,
    lines: &[String],
) -> anyhow::Result<Vec<Vec<HighlightSpan>>> {
    let root = tree.root_node();
    let query = Query::new(language, query_src)?;
    let mut cursor = QueryCursor::new();
    let mut captures: Vec<HighlightCapture> = Vec::new();
    let mut capture_iter = cursor.captures(&query, root, content.as_bytes());
    while let Some((m, idx)) = capture_iter.next() {
        let cap = m.captures[*idx];
        let name = query
            .capture_names()
            .get(cap.index as usize)
            .map(|s| *s)
            .unwrap_or_default();
        let kind = match highlight_kind_for_capture(name) {
            Some(k) => k,
            None => continue,
        };
        captures.push(HighlightCapture {
            start_byte: cap.node.start_byte(),
            end_byte: cap.node.end_byte(),
            start_point: cap.node.start_position(),
            end_point: cap.node.end_position(),
            kind,
        });
    }

    if captures.is_empty() {
        return Ok(default_highlights(lines));
    }

    captures.sort_by_key(|c| (c.start_byte, c.end_byte));
    let mut spans = default_highlights(lines);
    let line_starts = line_start_bytes(content);
    for cap in captures {
        apply_capture(&mut spans, lines, &line_starts, &cap);
    }
    Ok(spans)
}

fn line_start_bytes(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn apply_capture(
    spans: &mut Vec<Vec<HighlightSpan>>,
    lines: &[String],
    line_starts: &[usize],
    cap: &HighlightCapture,
) {
    if spans.is_empty() {
        return;
    }
    let start_row = cap.start_point.row;
    let mut end_row = cap.end_point.row;
    if start_row >= spans.len() {
        return;
    }
    if end_row >= spans.len() {
        end_row = spans.len() - 1;
    }

    for line_idx in start_row..=end_row {
        let line = match lines.get(line_idx) {
            Some(l) => l,
            None => continue,
        };
        let line_start_byte = line_starts.get(line_idx).copied().unwrap_or(0);
        let local_start = if line_idx == start_row {
            cap.start_byte.saturating_sub(line_start_byte)
        } else {
            0
        };
        let local_end = if line_idx == end_row {
            cap.end_byte.saturating_sub(line_start_byte)
        } else {
            line.as_bytes().len()
        };

        let line_len = line.as_bytes().len();
        let start_byte = local_start.min(line_len);
        let end_byte = local_end.min(line_len);
        if start_byte >= end_byte {
            continue;
        }
        insert_highlight(&mut spans[line_idx], start_byte, end_byte, cap.kind);
    }
}

fn insert_highlight(spans: &mut Vec<HighlightSpan>, start: usize, end: usize, kind: HighlightKind) {
    if start >= end || spans.is_empty() {
        return;
    }
    let mut new_spans = Vec::new();
    for span in spans.drain(..) {
        if span.end <= start || span.start >= end {
            new_spans.push(span);
            continue;
        }
        if span.start < start {
            new_spans.push(HighlightSpan {
                start: span.start,
                end: start,
                kind: span.kind,
            });
        }
        let mid_end = span.end.min(end);
        new_spans.push(HighlightSpan {
            start: start.max(span.start),
            end: mid_end,
            kind,
        });
        if span.end > end {
            new_spans.push(HighlightSpan {
                start: end,
                end: span.end,
                kind: span.kind,
            });
        }
    }
    *spans = merge_spans(new_spans);
}

fn merge_spans(mut spans: Vec<HighlightSpan>) -> Vec<HighlightSpan> {
    let mut merged: Vec<HighlightSpan> = Vec::new();
    for span in spans.drain(..) {
        if span.start >= span.end {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.end == span.start && last.kind == span.kind {
                last.end = span.end;
                continue;
            }
        }
        merged.push(span);
    }
    merged
}

fn highlight_kind_for_capture(name: &str) -> Option<HighlightKind> {
    if name.starts_with("comment") {
        Some(HighlightKind::Comment)
    } else if name.starts_with("string") {
        Some(HighlightKind::String)
    } else if name.starts_with("keyword") || name.starts_with("storage") || name.contains("macro") {
        Some(HighlightKind::Keyword)
    } else if name.starts_with("function") || name.contains("function") || name.contains("method") {
        Some(HighlightKind::Function)
    } else if name.starts_with("type") || name.contains("type") || name.contains("constructor") {
        Some(HighlightKind::Type)
    } else if name.starts_with("number") || name.contains("constant.numeric") {
        Some(HighlightKind::Number)
    } else if name.starts_with("constant") || name.contains("constant") {
        Some(HighlightKind::Constant)
    } else if name.contains("property") || name.contains("field") || name.contains("attribute") {
        Some(HighlightKind::Property)
    } else if name.contains("operator") || name.starts_with("punctuation") {
        Some(HighlightKind::Operator)
    } else if name.contains("builtin") || name.contains("variable.builtin") {
        Some(HighlightKind::Builtin)
    } else {
        None
    }
}

fn color_for_kind(kind: HighlightKind, palette: &Palette) -> RayColor {
    match kind {
        HighlightKind::Plain => palette.text,
        HighlightKind::Comment => palette.comment,
        HighlightKind::String => palette.string,
        HighlightKind::Keyword => palette.keyword,
        HighlightKind::Function => palette.call,
        HighlightKind::Type => palette.r#type,
        HighlightKind::Constant => palette.constant,
        HighlightKind::Number => palette.number,
        HighlightKind::Property => palette.property,
        HighlightKind::Operator => palette.operator,
        HighlightKind::Builtin => palette.builtin,
    }
}

pub fn colorized_segments_with_calls(
    pf: &ParsedFile,
    line_idx: usize,
    calls: &[&FunctionCall],
    palette: &Palette,
) -> Vec<(String, RayColor)> {
    if line_idx >= pf.lines.len() {
        return Vec::new();
    }
    let line = &pf.lines[line_idx];
    let mut spans: Vec<(usize, usize, RayColor)> = pf
        .spans
        .get(line_idx)
        .cloned()
        .unwrap_or_else(|| {
            vec![HighlightSpan {
                start: 0,
                end: line.len(),
                kind: HighlightKind::Plain,
            }]
        })
        .into_iter()
        .map(|s| (s.start, s.end, color_for_kind(s.kind, palette)))
        .collect();

    let mut call_ranges: Vec<(usize, usize)> = calls.iter().map(|c| (c.col, c.len)).collect();
    call_ranges.sort_by_key(|r| r.0);

    for (call_start, call_len) in call_ranges {
        let call_end = call_start + call_len;
        let mut new_spans = Vec::new();
        for (s, e, col) in spans {
            if e <= call_start || s >= call_end {
                new_spans.push((s, e, col));
                continue;
            }
            if s < call_start {
                new_spans.push((s, call_start, col));
            }
            let mid_end = e.min(call_end);
            new_spans.push((call_start.max(s), mid_end, palette.call));
            if e > call_end {
                new_spans.push((call_end, e, col));
            }
        }
        spans = new_spans;
    }

    let mut segments: Vec<(String, RayColor)> = Vec::new();
    for (s, e, col) in spans {
        if s >= e || s >= line.len() {
            continue;
        }
        let end = e.min(line.len());
        let text = line[s..end].to_string();
        if let Some((last_text, last_col)) = segments.last_mut() {
            if *last_col == col {
                last_text.push_str(&text);
                continue;
            }
        }
        segments.push((text, col));
    }

    segments
}

fn node_text<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let range = node.byte_range();
    source.get(range)
}

fn parse_tree_sitter(
    path: &Path,
    content: &str,
    language: &Language,
    def_query_src: &str,
    call_query_src: &str,
) -> anyhow::Result<(ParsedComponents, Tree)> {
    let mut parser = Parser::new();
    parser.set_language(language)?;
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed"))?;
    let root = tree.root_node();
    let mut defs = Vec::new();
    let mut calls = Vec::new();
    let bytes = content.as_bytes();
    let mut cursor = QueryCursor::new();
    let def_query = Query::new(language, def_query_src)?;
    let mut def_matches = cursor.matches(&def_query, root, bytes);
    while let Some(m) = def_matches.next() {
        for cap in m.captures.iter() {
            let text = match node_text(cap.node, content) {
                Some(t) => t,
                None => continue,
            };
            let kind = def_kind(cap.node);
            let pos = cap.node.start_position();
            defs.push(FunctionDef {
                name: text.to_string(),
                module_path: module_for_path(path),
                line: pos.row,
                col: pos.column,
                kind,
            });
        }
    }
    let call_query = Query::new(language, call_query_src)?;
    let mut call_cursor = QueryCursor::new();
    let mut call_matches = call_cursor.matches(&call_query, root, bytes);
    while let Some(m) = call_matches.next() {
        for cap in m.captures.iter() {
            let text = match node_text(cap.node, content) {
                Some(t) => t,
                None => continue,
            };
            let pos = cap.node.start_position();
            calls.push(FunctionCall {
                name: text.to_string(),
                module_path: module_for_path(path),
                line: pos.row,
                col: pos.column,
                len: text.len(),
            });
        }
    }
    defs.sort_by_key(|d| d.line);
    calls.sort_by_key(|c| c.line);
    Ok((ParsedComponents { defs, calls }, tree))
}

fn module_for_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}
pub fn find_function_span(pf: &ParsedFile, line: usize) -> Option<(usize, usize)> {
    if pf.defs.is_empty() {
        return None;
    }
    let mut defs: Vec<&FunctionDef> = pf.defs.iter().collect();
    defs.sort_by_key(|d| d.line);

    let idx = defs
        .iter()
        .position(|d| d.line == line)
        .or_else(|| defs.iter().rposition(|d| d.line <= line))?;
    let target = defs[idx];

    let type_kinds = ["struct_item", "enum_item", "trait_item", "type_item", "impl_item"];
    let is_type = type_kinds.contains(&target.kind.as_str());

    if is_type {
        let start_idx = defs
            .iter()
            .position(|d| d.name == target.name && type_kinds.contains(&d.kind.as_str()))
            .unwrap_or(idx);
        let start = defs[start_idx].line;

        let end = defs
            .iter()
            .skip(idx + 1)
            .find(|d| {
                if d.name == target.name {
                    return false;
                }
                if type_kinds.contains(&d.kind.as_str()) {
                    return true;
                }
                d.kind != "impl_item"
            })
            .map(|d| d.line.saturating_sub(1))
            .unwrap_or_else(|| pf.lines.len().saturating_sub(1));
        return Some((start, end.min(pf.lines.len().saturating_sub(1))));
    }

    let start = target.line;
    let end = defs
        .iter()
        .skip(idx + 1)
        .find(|d| d.line > start)
        .map(|d| d.line.saturating_sub(1))
        .unwrap_or_else(|| pf.lines.len().saturating_sub(1));
    Some((start, end.min(pf.lines.len().saturating_sub(1))))
}
