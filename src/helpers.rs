use crate::{CodeViewKind, ParsedFile};

pub fn matches_view(a: &CodeViewKind, b: &CodeViewKind) -> bool {
    match (a, b) {
        (CodeViewKind::FullFile, CodeViewKind::FullFile) => true,
        (CodeViewKind::SingleFn { start: s1, end: e1 }, CodeViewKind::SingleFn { start: s2, end: e2 }) => s1 == s2 && e1 == e2,
        _ => false,
    }
}

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
