//! How the Kernel reaches a Workspace it did not create.
mod common;

use accrete::{config::Project, store};
use common::{MARKER, write_marker};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn accrete() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_accrete"));
    command.env_remove("ACCRETE_PROJECT");
    command
}

#[test]
fn format_and_project_id_are_a_complete_marker_and_everything_else_appears_on_first_use() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let state = dir.path().join("state");
    write_marker(&root, "format = 1\nproject_id = \"minimal\"\n");

    let project = Project::discover(Some(&root), Some(&state), None).unwrap();
    assert_eq!(project.timezone, "UTC");
    for (store, resolved) in [
        (".accrete/objects", &project.metadata),
        ("results", &project.results),
        ("memory", &project.memory),
        ("tools", &project.tool_sources),
        ("scratch", &project.scratch),
    ] {
        assert!(resolved.ends_with(store), "{store}");
        assert!(!root.join(store).exists(), "{store} should be lazy");
    }
    assert!(!root.join(".accrete/data").exists());
    assert!(!state.exists(), "resolving creates no host-local state");

    store::Store::new(&project)
        .create("result", json!({"id":"first"}))
        .unwrap();
    assert!(root.join("results/first.json").is_file());
    assert!(project.state_dir.join("view.json").is_file());
    assert!(!root.join("memory").exists(), "unused stores stay absent");
    let entries: Vec<_> = fs::read_dir(root.join(".accrete"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries, ["project.toml"]);
}

#[test]
fn global_state_and_host_options_apply_to_a_marker_with_a_safe_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let state = dir.path().join("host-state");
    write_marker(
        &root,
        &MARKER
            .replace("fixture-workspace", "named-project.1")
            .replace("\"UTC\"", "\"Asia/Shanghai\""),
    );
    let output = accrete()
        .args(["--json", "--project"])
        .arg(&root)
        .arg("--state-dir")
        .arg(&state)
        .args([
            "--host",
            "test-machine",
            "result",
            "record",
            "--id",
            "first",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project = Project::discover(Some(&root), Some(&state), Some("test-machine")).unwrap();
    assert_eq!(project.project_id, "named-project.1");
    assert_eq!(project.timezone, "Asia/Shanghai");
    assert!(
        project
            .state_dir
            .starts_with(fs::canonicalize(&state).unwrap())
    );
    assert_eq!(project.host, "test-machine");
    assert!(project.state_dir.join("view.json").is_file());
    assert!(!root.join(".git").exists());
}

#[test]
fn the_nearest_marker_and_the_environment_select_a_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let near = dir.path().join("near");
    let other = dir.path().join("other");
    write_marker(&near, MARKER);
    write_marker(
        &other,
        &MARKER.replace("fixture-workspace", "selected-by-environment"),
    );
    let nested = near.join("some/deep/directory");
    fs::create_dir_all(&nested).unwrap();
    let resolve = |environment: Option<&Path>| -> Value {
        let mut command = accrete();
        command.current_dir(&nested).arg("--state-dir").arg(&state);
        if let Some(path) = environment {
            command.env("ACCRETE_PROJECT", path);
        }
        let output = command
            .args(["--json", "config", "resolve"])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice(&output.stdout).unwrap()
    };
    assert_eq!(resolve(None)["project_id"], "fixture-workspace");
    assert_eq!(
        resolve(Some(&other))["project_id"],
        "selected-by-environment"
    );
}

#[test]
fn outside_a_workspace_the_error_says_how_to_create_one() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent");
    let by_ancestor = accrete()
        .current_dir(dir.path())
        .args(["--json", "config", "resolve"])
        .output()
        .unwrap();
    let by_flag = accrete()
        .arg("--project")
        .arg(&absent)
        .args(["--json", "config", "resolve"])
        .output()
        .unwrap();
    let by_environment = accrete()
        .env("ACCRETE_PROJECT", &absent)
        .args(["--json", "config", "resolve"])
        .output()
        .unwrap();
    for output in [by_ancestor, by_flag, by_environment] {
        assert_eq!(output.status.code(), Some(3));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "project");
        let message = error["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("npx accrete-kit@latest create"),
            "{message}"
        );
    }
    assert!(!absent.exists());
}

#[test]
fn a_marker_outside_the_kernels_rules_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let refused = |marker: &str| {
        let root = dir.path().join("project");
        write_marker(&root, marker);
        Project::discover(Some(&root), Some(&state), None)
            .unwrap_err()
            .kind
    };
    assert_eq!(
        refused("format = 2\nproject_id = \"later\"\n"),
        "unsupported_format"
    );
    assert_eq!(
        refused("project_id = \"unformatted\"\n"),
        "unsupported_format"
    );
    assert_eq!(refused("format = 1\n"), "config");
    assert_eq!(
        refused("format = 1\nproject_id = \"../escape\"\n"),
        "config"
    );
    assert_eq!(
        refused("format = 1\nproject_id = \"p\"\ntimezone = \"Mars/Olympus\"\n"),
        "config"
    );
}

#[test]
fn interface_reports_the_kernel_without_a_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let output = accrete()
        .current_dir(dir.path())
        .args(["interface", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["cli_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(response["project_formats"], json!([1]));
    assert_eq!(response["project_marker"], ".accrete/project.toml");
    assert!(response.get("template").is_none());
    assert!(response.get("template_inventory").is_none());
}
