use std::path::Path;

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

pub fn module_for_path(path: &Path) -> String {
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");

    // For lib.rs/main.rs, use the crate directory name instead of the generic file stem.
    if file_stem == "lib" || file_stem == "main" || file_stem == "mod" {
        if let Some(dir) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
        {
            return strip_version_suffix(dir);
        }
    }

    file_stem.to_string()
}

fn strip_version_suffix(name: &str) -> String {
    // For registry dirs like "resvg-0.45.1", drop the version suffix.
    if let Some((base, last)) = name.rsplit_once('-') {
        let looks_like_version = last.chars().all(|c| c.is_ascii_digit() || c == '.');
        if looks_like_version {
            return base.to_string();
        }
    }
    name.to_string()
}
