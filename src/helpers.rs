use crate::code_window::CodeViewKind;

pub fn matches_view(a: &CodeViewKind, b: &CodeViewKind) -> bool {
    match (a, b) {
        (CodeViewKind::FullFile, CodeViewKind::FullFile) => true,
        (
            CodeViewKind::SingleFn { start: s1, end: e1 },
            CodeViewKind::SingleFn { start: s2, end: e2 },
        ) => s1 == s2 && e1 == e2,
        _ => false,
    }
}
