//! The project catalogue: `projects.discover` from `imperatorium/projects.py`.
//! Every folder under the configured roots, plus every folder a session has
//! run in, one entry per normalised path, sorted by lowercased name.

use std::path::Path;

use serde_json::{json, Value};

use crate::paths;

const SKIP: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "__pycache__",
    ".idea",
    ".vs",
];

/// projects.discover(roots, extra).
pub fn discover(roots: &[String], extra: &[String]) -> Vec<Value> {
    let mut seen: indexmap::IndexMap<String, Value> = indexmap::IndexMap::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if SKIP.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            let path = Path::new(root).join(&name);
            if !path.is_dir() {
                continue;
            }
            let p = path.to_string_lossy().into_owned();
            let (Some(key), Some(abs)) = (paths::abs_normcase(&p), paths::abspath(&p)) else {
                continue;
            };
            seen.insert(key, json!({"name": name, "path": abs, "source": "root"}));
        }
    }
    for path in extra {
        if path.is_empty() {
            continue;
        }
        let (Some(key), Some(abs)) = (paths::abs_normcase(path), paths::abspath(path)) else {
            continue;
        };
        if !seen.contains_key(&key) {
            let name = paths::basename(&paths::normpath(path)).to_string();
            seen.insert(key, json!({"name": name, "path": abs, "source": "session"}));
        }
    }
    let mut out: Vec<Value> = seen.into_values().collect();
    out.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_lowercase());
    out
}
