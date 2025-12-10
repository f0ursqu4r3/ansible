use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, Theme};
use syntect::parsing::SyntaxSet;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

use crate::theme::Palette;
use raylib::prelude::Color as RayColor;

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub module_path: String,
    pub line: usize,
    pub col: usize,
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
    pub color: RayColor,
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
    fn parse(&self, path: &Path, content: &str) -> anyhow::Result<ParsedComponents>;
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

    fn parse(&self, _path: &Path, _content: &str) -> anyhow::Result<ParsedComponents> {
        Ok(ParsedComponents {
            defs: Vec::new(),
            calls: Vec::new(),
        })
    }
}

pub struct TreeSitterPlugin {
    pub exts: &'static [&'static str],
    pub language: Language,
    pub def_query: &'static str,
    pub call_query: &'static str,
}

impl LanguagePlugin for TreeSitterPlugin {
    fn matches(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.exts.contains(&ext))
            .unwrap_or(false)
    }

    fn parse(&self, path: &Path, content: &str) -> anyhow::Result<ParsedComponents> {
        parse_tree_sitter(
            path,
            content,
            &self.language,
            self.def_query,
            self.call_query,
        )
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
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let spans = highlight_file(file, &lines);
            let pf = match plugins.iter().find(|p| p.matches(file)) {
                Some(plugin) => {
                    let parts = plugin.parse(file, &content)?;
                    ParsedFile {
                        path: file.clone(),
                        lines,
                        defs: parts.defs,
                        calls: parts.calls,
                        spans,
                    }
                }
                None => continue,
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

static SYNTAX: Lazy<(SyntaxSet, Theme)> = Lazy::new(|| {
    let ss = SyntaxSet::load_defaults_newlines();
    let theme = load_theme().unwrap_or_else(|| {
        syntect::highlighting::ThemeSet::load_defaults()
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap()
    });
    (ss, theme)
});

fn syntect_color_to_ray(c: syntect::highlighting::Color) -> RayColor {
    RayColor::new(c.r, c.g, c.b, c.a)
}

fn load_theme() -> Option<Theme> {
    let path = std::env::var("TM_THEME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let p = PathBuf::from("data/themes/Tomorrow-Night-Eighties.tmtheme");
            if p.exists() { Some(p) } else { None }
        })?;
    let folder = path.parent()?;
    let ts = syntect::highlighting::ThemeSet::load_from_folder(folder).ok()?;
    let name = path.file_stem()?.to_string_lossy();
    ts.themes.get(name.as_ref()).cloned()
}

fn highlight_file(path: &Path, lines: &[String]) -> Vec<Vec<HighlightSpan>> {
    let (ss, theme) = &*SYNTAX;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let syntax = ss
        .find_syntax_by_extension(ext)
        .or_else(|| ss.find_syntax_for_file(path).ok().flatten())
        .or_else(|| ss.find_syntax_by_token("Rust"))
        .or_else(|| Some(ss.find_syntax_plain_text()))
        .unwrap();

    lines
        .iter()
        .map(|line| {
            let mut h = HighlightLines::new(syntax, theme);
            let highlights: Vec<(Style, &str)> = h.highlight_line(line, ss).unwrap_or_default();
            let mut spans = Vec::new();
            let mut idx = 0;
            for (style, text) in highlights {
                let len = text.len();
                spans.push(HighlightSpan {
                    start: idx,
                    end: idx + len,
                    color: syntect_color_to_ray(style.foreground),
                });
                idx += len;
            }
            spans
        })
        .collect()
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
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.start, s.end, s.color))
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
) -> anyhow::Result<ParsedComponents> {
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
            let pos = cap.node.start_position();
            defs.push(FunctionDef {
                name: text.to_string(),
                module_path: module_for_path(path),
                line: pos.row,
                col: pos.column,
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
                len: text.chars().count(),
            });
        }
    }
    defs.sort_by_key(|d| d.line);
    calls.sort_by_key(|c| c.line);
    Ok(ParsedComponents { defs, calls })
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
    let target_name = &defs[idx].name;

    let first_idx = defs.iter().position(|d| d.name == *target_name)?;
    let last_idx = defs.iter().rposition(|d| d.name == *target_name)?;

    let start = defs[first_idx].line;
    let end = defs
        .iter()
        .skip(last_idx + 1)
        .find(|d| d.line > start)
        .map(|d| d.line.saturating_sub(1))
        .unwrap_or_else(|| pf.lines.len().saturating_sub(1));
    Some((start, end.min(pf.lines.len().saturating_sub(1))))
}
