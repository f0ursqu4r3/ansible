use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde::Deserialize;
use walkdir::WalkDir;

use super::plugins::default_plugins;
use super::rust_symbols::analyze_rust_symbols;
use super::types::{DefinitionLocation, ParsedFile};

const MAX_DEP_FILES: usize = 800;

pub struct ProjectModel {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub parsed: HashMap<PathBuf, ParsedFile>,
    pub defs: HashMap<String, Vec<DefinitionLocation>>,
}

impl ProjectModel {
    pub fn load(root: impl AsRef<Path>, include_deps: bool) -> anyhow::Result<Self> {
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
        let parse_file = |file: PathBuf,
                          parsed: &mut HashMap<PathBuf, ParsedFile>,
                          defs: &mut HashMap<String, Vec<DefinitionLocation>>|
         -> anyhow::Result<()> {
            let bytes = match fs::read(&file) {
                Ok(b) => b,
                Err(e) => {
                    anyhow::bail!("{}: {}", file.display(), e);
                }
            };
            let content = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    anyhow::bail!("{}: non-UTF8 file", file.display());
                }
            };
            let plugin = match plugins.iter().find(|p| p.matches(&file)) {
                Some(plugin) => plugin,
                None => return Ok(()),
            };
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let name_refs = if file
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("rs"))
                .unwrap_or(false)
            {
                analyze_rust_symbols(&file, &content)
            } else {
                Vec::new()
            };
            let (parts, spans) = plugin.parse_and_highlight(&file, &content, &lines)?;
            let pf = ParsedFile {
                path: file.clone(),
                lines,
                defs: parts.defs,
                calls: parts.calls,
                name_refs,
                spans,
                color_cache: std::cell::RefCell::new(None),
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
            parsed.insert(file, pf);
            Ok(())
        };

        for file in &files {
            if let Err(err) = parse_file(file.clone(), &mut parsed, &mut defs) {
                eprintln!("Skipping {}: {}", file.display(), err);
            }
        }

        if include_deps {
            let dep_files = collect_dependency_files(&root)?;
            for file in dep_files {
                if parsed.contains_key(&file) {
                    continue;
                }
                if let Err(err) = parse_file(file.clone(), &mut parsed, &mut defs) {
                    eprintln!("Skipping dep {}: {}", file.display(), err);
                }
            }
        }

        Ok(Self {
            root,
            files,
            parsed,
            defs,
        })
    }

    pub fn display_name(&self, path: &Path) -> String {
        if let Ok(local) = path.strip_prefix(&self.root) {
            return local.to_string_lossy().to_string();
        }
        if let Some(home) = cargo_home() {
            let registry = home.join("registry/src");
            if let Ok(registry_rel) = path.strip_prefix(registry) {
                return registry_rel.to_string_lossy().to_string();
            }
        }
        path.to_string_lossy().to_string()
    }
}

#[derive(Deserialize)]
struct CargoLock {
    package: Vec<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    source: Option<String>,
}

fn collect_dependency_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let lock_path = root.join("Cargo.lock");
    let lock_text = match fs::read_to_string(&lock_path) {
        Ok(text) => text,
        Err(_) => return Ok(Vec::new()),
    };
    let lock: CargoLock = match toml::from_str(&lock_text) {
        Ok(lock) => lock,
        Err(err) => {
            eprintln!("Cargo.lock parse error: {err}");
            return Ok(Vec::new());
        }
    };

    let Some(cargo_home) = cargo_home() else {
        return Ok(Vec::new());
    };
    let registry_src = cargo_home.join("registry/src");
    if !registry_src.exists() {
        return Ok(Vec::new());
    }

    let mut crate_roots: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let registry_entries: Vec<PathBuf> = match fs::read_dir(&registry_src) {
        Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => Vec::new(),
    };

    for pkg in lock.package {
        // Skip workspace members (no registry source) to avoid double indexing.
        if pkg.source.is_none() {
            continue;
        }
        if let Some(src) = pkg.source.as_deref() {
            if !src.starts_with("registry+") {
                continue;
            }
        }
        if !seen.insert((pkg.name.clone(), pkg.version.clone())) {
            continue;
        }
        let dir_name = format!("{}-{}", pkg.name, pkg.version);
        for reg in &registry_entries {
            let candidate = reg.join(&dir_name);
            if candidate.is_dir() {
                crate_roots.push(candidate);
                break;
            }
        }
    }

    crate_roots.sort();
    crate_roots.dedup();

    let mut dep_files = Vec::new();
    for crate_root in crate_roots {
        let src_dir = crate_root.join("src");
        if !src_dir.exists() {
            continue;
        }
        for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
            if dep_files.len() >= MAX_DEP_FILES {
                break;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.into_path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                dep_files.push(path);
            }
        }
        if dep_files.len() >= MAX_DEP_FILES {
            break;
        }
    }

    dep_files.sort();
    dep_files.dedup();
    Ok(dep_files)
}

fn cargo_home() -> Option<PathBuf> {
    if let Ok(custom) = env::var("CARGO_HOME") {
        return Some(PathBuf::from(custom));
    }
    env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".cargo"))
}
