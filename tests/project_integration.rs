use accrete::{config::Project, project, store};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn prepared() -> (TempDir, Project, TempDir) {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("local-state");
    project::init_with_context(
        root.path(),
        Some("Template project"),
        "America/Los_Angeles",
        None,
        false,
        None,
        Some(&state),
        Some("fixture"),
    )
    .unwrap();
    let project = Project::discover(Some(root.path()), Some(&state), Some("fixture")).unwrap();
    let template = tempfile::tempdir().unwrap();
    (root, project, template)
}

fn snapshot(path: &Path, version: &str, body: &str) {
    fs::create_dir_all(path.join("files")).unwrap();
    fs::write(path.join("files/AGENTS.md"), body).unwrap();
    fs::write(
        path.join("accrete-template.json"),
        serde_json::to_vec_pretty(&json!({
            "format":1,"asset_version":version,
            "requires":{"cli_min":"0.1.0","cli_max_exclusive":"0.2.0","project_format":1},
            "files":[{"source":"files/AGENTS.md","destination":"AGENTS.md"}]
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn installs_repeats_and_upgrades_owned_guidance() {
    let (_root, project, template) = prepared();
    snapshot(template.path(), "guidance-1", "# Original guidance\n");
    let preview = project::upgrade(&project, template.path(), true).unwrap();
    assert_eq!(preview["files"][0]["status"], "create");
    assert!(!project.root.join("AGENTS.md").exists());
    let first = project::upgrade(&project, template.path(), false).unwrap();
    assert_eq!(first["committed"], true);
    assert_eq!(
        fs::read_to_string(project.root.join("AGENTS.md")).unwrap(),
        "# Original guidance\n"
    );
    let again = project::upgrade(&project, template.path(), false).unwrap();
    assert_eq!(again["files"][0]["status"], "unchanged");
    snapshot(template.path(), "guidance-2", "# Updated guidance\n");
    project::upgrade(&project, template.path(), false).unwrap();
    assert_eq!(
        fs::read_to_string(project.root.join("AGENTS.md")).unwrap(),
        "# Updated guidance\n"
    );
    let inventory: Value = serde_json::from_slice(
        &fs::read(project.root.join(".accrete/template-install.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(inventory["asset_version"], "guidance-2");
    assert_eq!(
        inventory["files"]["AGENTS.md"]["sha256"],
        store::revision(b"# Updated guidance\n")
    );
}

#[test]
fn preserves_owner_edits_and_stops_before_other_asset_changes() {
    let (_root, project, template) = prepared();
    snapshot(template.path(), "1", "# Base\n");
    project::upgrade(&project, template.path(), false).unwrap();
    fs::write(project.root.join("AGENTS.md"), "# Owner text\n").unwrap();
    snapshot(template.path(), "2", "# Candidate\n");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(template.path().join("accrete-template.json")).unwrap())
            .unwrap();
    fs::write(template.path().join("files/manual.md"), "manual\n").unwrap();
    manifest["files"]
        .as_array_mut()
        .unwrap()
        .push(json!({"source":"files/manual.md","destination":"manual.md"}));
    fs::write(
        template.path().join("accrete-template.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let error = project::upgrade(&project, template.path(), false).unwrap_err();
    assert_eq!(error.kind, "conflict");
    assert_eq!(
        fs::read_to_string(project.root.join("AGENTS.md")).unwrap(),
        "# Owner text\n"
    );
    assert!(!project.root.join("manual.md").exists());
    assert!(
        !project
            .state_dir
            .join("receipts/template-upgrade.json")
            .exists()
    );
}

#[test]
fn rejects_unowned_existing_guidance_and_incompatible_template() {
    let (_root, project, template) = prepared();
    snapshot(template.path(), "1", "# Guidance\n");
    fs::write(
        project.root.join("AGENTS.md"),
        "# Existing owner instructions\n",
    )
    .unwrap();
    assert_eq!(
        project::upgrade(&project, template.path(), false)
            .unwrap_err()
            .kind,
        "conflict"
    );
    let manifest_path = template.path().join("accrete-template.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["requires"]["cli_min"] = json!("0.2.0");
    manifest["requires"]["cli_max_exclusive"] = json!("0.3.0");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        project::upgrade(&project, template.path(), true)
            .unwrap_err()
            .kind,
        "unsupported_format"
    );
    assert_eq!(
        fs::read_to_string(project.root.join("AGENTS.md")).unwrap(),
        "# Existing owner instructions\n"
    );
}

#[test]
fn resumes_asset_published_before_its_inventory() {
    let (_root, project, template) = prepared();
    snapshot(template.path(), "1", "# Complete guidance\n");
    let preview = project::upgrade(&project, template.path(), true).unwrap();
    let manifest_digest =
        store::revision(&fs::read(template.path().join("accrete-template.json")).unwrap());
    let receipt = json!({"format":1,"operation":"template_upgrade","source":template.path(),
        "manifest_sha256":manifest_digest,"inventory_sha256":null,"files":preview["files"]});
    fs::create_dir_all(project.state_dir.join("receipts")).unwrap();
    fs::write(
        project.state_dir.join("receipts/template-upgrade.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(project.root.join("AGENTS.md"), "# Complete guidance\n").unwrap();
    let completed = project::upgrade(&project, template.path(), false).unwrap();
    assert_eq!(completed["committed"], true);
    assert!(project.root.join(".accrete/template-install.json").exists());
    assert!(
        !project
            .state_dir
            .join("receipts/template-upgrade.json")
            .exists()
    );
}

#[test]
fn cli_init_uses_global_state_and_host_options_and_preserves_marker() {
    let root = tempfile::tempdir().unwrap();
    let project_root = root.path().join("project");
    let state = root.path().join("host-state");
    let output = Command::new(env!("CARGO_BIN_EXE_accrete"))
        .args(["--json", "--state-dir"])
        .arg(&state)
        .args(["--host", "test-machine", "init"])
        .arg(&project_root)
        .args([
            "--name",
            "Named project",
            "--timezone",
            "Asia/Shanghai",
            "--no-git",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project =
        Project::discover(Some(&project_root), Some(&state), Some("test-machine")).unwrap();
    assert!(
        project
            .state_dir
            .starts_with(fs::canonicalize(&state).unwrap())
    );
    assert_eq!(project.host, "test-machine");
    assert!(project.state_dir.join("view.json").is_file());
    let before = fs::read(project_root.join(".accrete/project.toml")).unwrap();
    project::init_with_context(
        &project_root,
        None,
        "UTC",
        None,
        false,
        None,
        Some(&state),
        Some("test-machine"),
    )
    .unwrap();
    assert_eq!(
        fs::read(project_root.join(".accrete/project.toml")).unwrap(),
        before
    );
    assert!(!project_root.join(".git").exists());
}
