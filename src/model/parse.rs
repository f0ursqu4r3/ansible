use std::path::Path;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use super::types::{FunctionCall, FunctionDef, ParsedComponents, ParsedFile};

pub(crate) fn parse_tree_sitter(
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
            let (node, kind) = def_node_and_kind(cap.node);
            let start = node.start_position();
            let end = node.end_position();
            defs.push(FunctionDef {
                name: text.to_string(),
                module_path: module_for_path(path),
                line: start.row,
                end_line: end.row,
                col: start.column,
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

fn def_node_and_kind(node: Node) -> (Node, String) {
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
            return (n, k);
        }
        if let Some(p) = n.parent() {
            n = p;
        } else {
            break;
        }
    }
    let kind = n.kind().to_string();
    (n, kind)
}

fn node_text<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let range = node.byte_range();
    source.get(range)
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
    let last_line = pf.lines.len().saturating_sub(1);

    let type_kinds = [
        "struct_item",
        "enum_item",
        "trait_item",
        "type_item",
        "impl_item",
    ];
    let is_type = type_kinds.contains(&target.kind.as_str());

    let mut start = target.line.min(last_line);
    let mut end = target.end_line.min(last_line);

    if is_type {
        for d in defs.iter().skip(idx + 1) {
            if type_kinds.contains(&d.kind.as_str()) && d.name != target.name {
                break;
            }
            if d.kind == "impl_item" && d.name == target.name {
                end = end.max(d.end_line.min(last_line));
            }
        }
    }

    start = extend_span_upwards(&pf.lines, start);
    if end < start {
        end = start;
    }

    Some((start, end))
}

fn extend_span_upwards(lines: &[String], start: usize) -> usize {
    if start == 0 || lines.is_empty() {
        return start;
    }
    let mut idx = start;
    while idx > 0 {
        let prev = lines[idx - 1].trim_start();
        if prev.starts_with("#[") || prev.starts_with("///") || prev.starts_with("//!") {
            idx -= 1;
            continue;
        }
        if prev.contains("*/") {
            idx -= 1;
            while idx > 0 {
                let line = &lines[idx - 1];
                idx -= 1;
                if line.contains("/*") {
                    break;
                }
            }
            continue;
        }
        if prev.is_empty() {
            break;
        }
        break;
    }
    idx
}
