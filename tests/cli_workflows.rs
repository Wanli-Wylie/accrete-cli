use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn cli(root: &Path, args: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_accrete"))
        .arg("--json")
        .arg("--project")
        .arg(root)
        .arg("--state-dir")
        .arg(root.join("local-state"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "args={args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = if code == 0 || code == 7 || code == 8 {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(bytes)))
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn setup() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    cli(
        root,
        &["init", root.to_str().unwrap(), "--name", "example"],
        0,
    );
    git(root, &["config", "user.name", "ACCRETE test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    fs::write(root.join("program.txt"), "initial\n").unwrap();
    git(root, &["add", "program.txt"]);
    git(root, &["commit", "-m", "Initial model"]);
    t
}
#[test]
fn models_use_actual_repository_worktrees_and_keep_native_dirty_refusal() {
    let t = setup();
    let root = t.path();
    let registered = cli(
        root,
        &[
            "model",
            "register",
            "--id",
            "model-one",
            "--name",
            "target",
            "--checkout",
            ".",
        ],
        0,
    );
    assert_eq!(registered["object_id"], "model-one");
    assert_eq!(
        cli(
            root,
            &[
                "model",
                "register",
                "--id",
                "model-one",
                "--name",
                "target",
                "--checkout",
                "."
            ],
            0
        )["unchanged"],
        true
    );
    let worktree = root.join("model-worktree");
    let opened = cli(
        root,
        &[
            "model",
            "iteration",
            "open",
            "target",
            "--name",
            "solver-change",
            "--branch",
            "solver-change",
            "--worktree",
            worktree.to_str().unwrap(),
        ],
        0,
    );
    let id = opened["object_id"].as_str().unwrap();
    assert_eq!(
        fs::read_to_string(worktree.join("program.txt")).unwrap(),
        "initial\n"
    );
    fs::write(worktree.join("program.txt"), "changed\n").unwrap();
    cli(
        root,
        &[
            "model",
            "iteration",
            "close",
            id,
            "--outcome",
            "retained separately",
            "--expect",
            opened["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert!(worktree.exists());
    // Git itself owns refusal to remove edited working content.
    let failed = Command::new(env!("CARGO_BIN_EXE_accrete"))
        .arg("--project")
        .arg(root)
        .args(["model", "iteration", "remove-worktree", id])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(worktree.join("program.txt").exists());
}
#[test]
fn tool_wrappers_point_to_source_and_retirement_preserves_custom_edits() {
    let t = setup();
    let root = t.path();
    let source = root.join("tools/diagnostic");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"),"---\nname: diagnostic\ndescription: Explain model diagnostics.\n---\nRead references/guide.md.\n").unwrap();
    let admitted = cli(
        root,
        &[
            "tool",
            "admit",
            "--id",
            "tool-one",
            "--name",
            "diagnostic",
            "--source",
            "tools/diagnostic",
        ],
        0,
    );
    let exposed = cli(
        root,
        &["tool", "expose", "diagnostic", "--agent", "codex"],
        0,
    );
    let path = Path::new(exposed["path"].as_str().unwrap()).join("SKILL.md");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(source.to_str().unwrap()));
    assert!(text.contains("Resolve all relative"));
    cli(root, &["tool", "refresh", "diagnostic"], 0);
    let export = root.join("exported-tool");
    cli(
        root,
        &[
            "tool",
            "export",
            "diagnostic",
            "--to",
            export.to_str().unwrap(),
        ],
        0,
    );
    assert!(export.join("tool.json").is_file());
    assert!(export.join("source/SKILL.md").is_file());
    let linked = cli(
        root,
        &[
            "tool",
            "expose",
            "diagnostic",
            "--agent",
            "claude",
            "--mode",
            "symlink",
        ],
        0,
    );
    assert!(
        fs::symlink_metadata(linked["path"].as_str().unwrap())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::write(&path, format!("{text}\nMy custom note\n")).unwrap();
    let retired = cli(
        root,
        &[
            "tool",
            "retire",
            "tool-one",
            "--expect",
            admitted["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert_eq!(retired["exposures"]["exposure_pending"], true);
    assert!(path.exists());
    assert!(source.join("SKILL.md").exists());
    assert!(fs::symlink_metadata(linked["path"].as_str().unwrap()).is_err());
}
#[test]
fn cli_result_observation_retry_cas_and_sql_links() {
    let t = setup();
    let root = t.path();
    cli(
        root,
        &["model", "register", "--id", "model-one", "--checkout", "."],
        0,
    );
    let commit = git(root, &["rev-parse", "HEAD"]);
    let input = root.join("result-input.json");
    fs::write(&input,serde_json::to_vec(&json!({"id":"result-one","models":[{"model_id":"model-one","commit":commit,"role":"target"}],"inputs":["unresolved-data"],"metrics":{"loss":1.2}})).unwrap()).unwrap();
    let recorded = cli(
        root,
        &[
            "result",
            "record",
            "--from",
            input.to_str().unwrap(),
            "--observe-checkout",
            ".",
        ],
        0,
    );
    assert_eq!(
        cli(
            root,
            &[
                "result",
                "record",
                "--from",
                input.to_str().unwrap(),
                "--observe-checkout",
                "."
            ],
            0
        )["unchanged"],
        true
    );
    let note = root.join("observation.md");
    fs::write(&note, "Step completed with the selected program.\n").unwrap();
    let observed = cli(
        root,
        &[
            "result",
            "observe",
            "result-one",
            "--text-file",
            note.to_str().unwrap(),
            "--operation-id",
            "obs-one",
        ],
        0,
    );
    assert_eq!(
        cli(
            root,
            &[
                "result",
                "observe",
                "result-one",
                "--text-file",
                note.to_str().unwrap(),
                "--operation-id",
                "obs-one"
            ],
            0
        )["revision"],
        observed["revision"]
    );
    let patch = root.join("patch.json");
    fs::write(&patch, "{\"metrics\":{\"loss\":1.1}}").unwrap();
    cli(
        root,
        &[
            "result",
            "amend",
            "result-one",
            "--from",
            patch.to_str().unwrap(),
            "--expect",
            recorded["revision"].as_str().unwrap(),
        ],
        4,
    );
    let amended = cli(
        root,
        &[
            "result",
            "amend",
            "result-one",
            "--from",
            patch.to_str().unwrap(),
            "--expect",
            observed["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert!(amended["committed"].as_bool().unwrap());
    let cfg = cli(root, &["config", "resolve"], 0);
    let database = cfg["local_state"]["index"].as_str().unwrap();
    let conn = rusqlite::Connection::open(database).unwrap();
    let link: (String, String) = conn
        .query_row("SELECT model_id,commit_id FROM result_models", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(link, ("model-one".into(), commit));
    let inputs: i64 = conn
        .query_row(
            "SELECT count(*) FROM result_data WHERE data_id='unresolved-data'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(inputs, 1);
}
#[test]
fn data_flags_reject_invalid_nested_fields_without_panicking() {
    let t = setup();
    let root = t.path();
    let input = root.join("bad.json");
    fs::write(&input, "{\"origin\":\"legacy origin\"}").unwrap();
    let error = cli(
        root,
        &[
            "data",
            "register",
            "--from",
            input.to_str().unwrap(),
            "--result",
            "r1",
        ],
        2,
    );
    assert_eq!(error["error"]["kind"], "invalid");
}

#[test]
fn retained_tools_need_matching_clean_source_without_switching_branches() {
    let t = setup();
    let root = t.path();
    let source = tempfile::tempdir().unwrap();
    git(source.path(), &["init"]);
    git(source.path(), &["config", "user.name", "ACCRETE test"]);
    git(
        source.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    fs::write(
        source.path().join("SKILL.md"),
        "---\nname: retained\ndescription: Retained source.\n---\nUse source.\n",
    )
    .unwrap();
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "-m", "Capability"]);
    let commit = git(source.path(), &["rev-parse", "HEAD"]);
    cli(
        root,
        &[
            "tool",
            "admit",
            "--id",
            "retained-tool",
            "--name",
            "retained",
            "--source",
            source.path().to_str().unwrap(),
            "--source-mode",
            "retained",
            "--revision",
            &commit,
        ],
        0,
    );
    cli(
        root,
        &["tool", "expose", "retained-tool", "--agent", "codex"],
        0,
    );
    fs::write(source.path().join("SKILL.md"), "changed source\n").unwrap();
    cli(
        root,
        &["tool", "expose", "retained-tool", "--agent", "codex"],
        4,
    );
    assert_eq!(git(source.path(), &["rev-parse", "HEAD"]), commit);
}
