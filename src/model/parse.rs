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

    let type_kinds = [
        "struct_item",
        "enum_item",
        "trait_item",
        "type_item",
        "impl_item",
    ];
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
