//! A Workspace as far as the Kernel is concerned. Creating one is the generator's job,
//! so tests write the marker directly.
use std::{fs, path::Path};

/// The fields the generator writes.
pub const MARKER: &str = r#"format = 1
project_id = "fixture-workspace"
name = "fixture"
timezone = "UTC"

[paths]
metadata = ".accrete/objects"
results = "results"
memory = "memory"
tool_sources = "tools"
scratch = "scratch"

[locations.working]
kind = "filesystem"
"#;

pub fn write_marker(root: &Path, marker: &str) {
    fs::create_dir_all(root.join(".accrete")).unwrap();
    fs::write(root.join(".accrete/project.toml"), marker).unwrap();
}
