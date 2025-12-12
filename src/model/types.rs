use raylib::prelude::Color as RayColor;
use std::cell::RefCell;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub module_path: String,
    pub line: usize,
    pub end_line: usize,
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
    pub name_refs: Vec<NameReference>,
    pub spans: Vec<Vec<HighlightSpan>>,
    pub color_cache: RefCell<Option<(u64, Arc<Vec<Vec<(Range<usize>, RayColor)>>>)>>,
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
    #[allow(dead_code)]
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct NameReference {
    pub name: String,
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub target: Option<DefinitionLocation>,
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

pub struct ParsedComponents {
    pub defs: Vec<FunctionDef>,
    pub calls: Vec<FunctionCall>,
}
