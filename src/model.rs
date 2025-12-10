use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use syn::visit::Visit;
use walkdir::WalkDir;

use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, Theme};
use syntect::parsing::SyntaxSet;

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

impl ProjectModel {
    pub fn load(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut files = Vec::new();
        for entry in WalkDir::new(&root) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .map(|ext| ext == "rs")
                    .unwrap_or(false)
            {
                files.push(entry.into_path());
            }
        }
        files.sort();

        let mut parsed = HashMap::new();
        let mut defs: HashMap<String, Vec<DefinitionLocation>> = HashMap::new();
        for file in &files {
            let pf = parse_rust_file(file)?;
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

fn highlight_file(lines: &[String]) -> Vec<Vec<HighlightSpan>> {
    let (ss, theme) = &*SYNTAX;
    let syntax = ss
        .find_syntax_by_extension("rs")
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

pub fn parse_rust_file(path: &Path) -> anyhow::Result<ParsedFile> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let spans = highlight_file(&lines);

    let file = syn::parse_file(&content)?;
    let mut collector = SyntaxCollector::new(path, &content);
    collector.visit_file(&file);

    Ok(ParsedFile {
        path: path.to_path_buf(),
        lines,
        defs: collector.defs,
        calls: collector.calls,
        spans,
    })
}

fn span_to_line_col(span: Span) -> Option<(usize, usize)> {
    let start = span.start();
    Some((start.line.saturating_sub(1), start.column))
}

fn span_to_len(span: Span, source: &str) -> Option<usize> {
    let start = span.start();
    let end = span.end();
    if start.line != end.line {
        return None;
    }
    let line_idx = start.line.saturating_sub(1);
    let line = source.lines().nth(line_idx)?;
    let start_idx = start.column.min(line.len());
    let end_idx = end.column.min(line.len());
    Some(end_idx.saturating_sub(start_idx))
}

struct SyntaxCollector<'a> {
    file: &'a Path,
    source: &'a str,
    defs: Vec<FunctionDef>,
    calls: Vec<FunctionCall>,
    module_stack: Vec<String>,
    keywords: HashSet<&'static str>,
}

impl<'a> SyntaxCollector<'a> {
    fn new(file: &'a Path, source: &'a str) -> Self {
        let keywords: HashSet<&'static str> = KEYWORDS.into_iter().collect();

        Self {
            file,
            source,
            defs: Vec::new(),
            calls: Vec::new(),
            module_stack: Vec::new(),
            keywords,
        }
    }

    fn module_path(&self) -> String {
        if self.module_stack.is_empty() {
            "crate".to_string()
        } else {
            format!("crate::{}", self.module_stack.join("::"))
        }
    }

    fn push_mod(&mut self, name: &str) {
        self.module_stack.push(name.to_string());
    }

    fn pop_mod(&mut self) {
        self.module_stack.pop();
    }

    fn add_def(&mut self, ident: &syn::Ident, span: Span) {
        if let Some((line, col)) = span_to_line_col(span) {
            self.defs.push(FunctionDef {
                name: ident.to_string(),
                module_path: self.module_path(),
                line,
                col,
            });
        }
    }

    fn add_call(&mut self, name: String, span: Span) {
        if let Some((line, col)) = span_to_line_col(span) {
            self.calls.push(FunctionCall {
                name: name.clone(),
                module_path: self.module_path(),
                line,
                col,
                len: span_to_len(span, self.source).unwrap_or(name.len()),
            });
        }
    }
}

impl<'ast> Visit<'ast> for SyntaxCollector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.add_def(&node.sig.ident, node.sig.ident.span());
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.add_def(&node.sig.ident, node.sig.ident.span());
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.push_mod(&node.ident.to_string());
        syn::visit::visit_item_mod(self, node);
        self.pop_mod();
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(ref path) = *node.func {
            if let Some(segment) = path.path.segments.last() {
                let name = segment.ident.to_string();
                if !self.keywords.contains(name.as_str()) {
                    self.add_call(name, segment.ident.span());
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if !self.keywords.contains(name.as_str()) {
            self.add_call(name, node.method.span());
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

pub const KEYWORDS: [&str; 21] = [
    "if", "else", "for", "while", "loop", "match", "fn", "pub", "impl", "struct", "enum", "use",
    "let", "in", "where", "return", "async", "mod", "trait", "const", "static",
];

pub fn find_function_span(pf: &ParsedFile, line: usize) -> Option<(usize, usize)> {
    let start = pf.defs.iter().find(|d| d.line == line).map(|d| d.line)?;
    let mut end = pf.lines.len().saturating_sub(1);
    for def in &pf.defs {
        if def.line > start {
            end = def.line.saturating_sub(1);
            break;
        }
    }
    Some((start, end))
}
