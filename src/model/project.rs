use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::plugins::default_plugins;
use super::types::{DefinitionLocation, ParsedFile};

pub struct ProjectModel {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub parsed: HashMap<PathBuf, ParsedFile>,
    pub defs: HashMap<String, Vec<DefinitionLocation>>,
}

impl ProjectModel {
    pub fn load(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let plugins = default_plugins();
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
