use raylib::prelude::Color as RayColor;
use tree_sitter::{Language, Point, Query, QueryCursor, StreamingIterator, Tree};

use crate::theme::Palette;

use super::types::{FunctionCall, HighlightKind, HighlightSpan, ParsedFile};

#[derive(Clone, Debug)]
struct HighlightCapture {
    start_byte: usize,
    end_byte: usize,
    start_point: Point,
    end_point: Point,
    kind: HighlightKind,
}

pub(crate) fn default_highlights(lines: &[String]) -> Vec<Vec<HighlightSpan>> {
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

pub(crate) fn highlight_tree_sitter(
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
