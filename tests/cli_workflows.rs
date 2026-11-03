mod common;

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
    common::write_marker(root, common::MARKER);
    git(root, &["init"]);
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

#[test]
fn repository_checkout_binds_locally_and_stays_out_of_the_descriptor() {
    let t = setup();
    let root = t.path();
    let input = root.join("model.json");
    fs::write(
        &input,
        r#"{"id":"model-one","name":"target","repository":{"checkout":"."}}"#,
    )
    .unwrap();
    let registered = cli(
        root,
        &["model", "register", "--from", input.to_str().unwrap()],
        0,
    );
    assert_eq!(registered["object_id"], "model-one");
    let text = fs::read_to_string(registered["path"].as_str().unwrap()).unwrap();
    assert!(!text.contains("checkout"), "{text}");
    let local = fs::read_to_string(root.join(".accrete/local.toml")).unwrap();
    assert!(local.contains("model-one"));

    // --checkout wins over the document.
    let other = root.join("other-repository");
    fs::create_dir(&other).unwrap();
    git(&other, &["init"]);
    fs::write(
        &input,
        r#"{"id":"model-two","repository":{"checkout":"missing","locator":"elsewhere"}}"#,
    )
    .unwrap();
    let second = cli(
        root,
        &[
            "model",
            "register",
            "--from",
            input.to_str().unwrap(),
            "--checkout",
            other.to_str().unwrap(),
        ],
        0,
    );
    let doc: Value =
        serde_json::from_slice(&fs::read(second["path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(doc["repository"], json!({"locator":"elsewhere"}));
    assert!(
        fs::read_to_string(root.join(".accrete/local.toml"))
            .unwrap()
            .contains("other-repository")
    );
}

#[test]
fn iteration_input_documents_follow_the_flags() {
    let t = setup();
    let root = t.path();
    cli(
        root,
        &["model", "register", "--id", "model-one", "--checkout", "."],
        0,
    );
    let input = root.join("open.json");
    fs::write(
        &input,
        r#"{"id":"from-file","name":"file name","purpose":"Question","note":"kept"}"#,
    )
    .unwrap();
    let plain = cli(
        root,
        &[
            "model",
            "iteration",
            "open",
            "model-one",
            "--from",
            input.to_str().unwrap(),
            "--name",
            "flag name",
        ],
        0,
    );
    assert_eq!(plain["object_id"], "from-file");
    let doc: Value =
        serde_json::from_slice(&fs::read(plain["path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(doc["name"], "flag name");
    assert_eq!(doc["purpose"], "Question");
    assert_eq!(doc["note"], "kept");
    assert!(plain.get("worktree").is_none(), "no storage is allocated");

    let worktree = root.join("from-file-worktree");
    fs::write(
        &input,
        format!(
            r#"{{"id":"with-tree","worktree":{:?},"branch":"file-branch"}}"#,
            worktree.to_str().unwrap()
        ),
    )
    .unwrap();
    cli(
        root,
        &[
            "model",
            "iteration",
            "open",
            "model-one",
            "--from",
            input.to_str().unwrap(),
        ],
        0,
    );
    assert!(worktree.join("program.txt").is_file());

    // existing without a worktree fails the same way from a flag and from a file.
    let by_flag = cli(
        root,
        &["model", "iteration", "open", "model-one", "--existing"],
        2,
    );
    assert_eq!(by_flag["error"]["kind"], "invalid");
    fs::write(&input, r#"{"existing":true}"#).unwrap();
    let by_file = cli(
        root,
        &[
            "model",
            "iteration",
            "open",
            "model-one",
            "--from",
            input.to_str().unwrap(),
        ],
        2,
    );
    assert_eq!(by_file["error"]["kind"], "invalid");
    assert_eq!(by_file["error"]["message"], by_flag["error"]["message"]);
}

#[test]
fn iteration_close_stores_the_document_and_retains_every_commit() {
    let t = setup();
    let root = t.path();
    cli(
        root,
        &["model", "register", "--id", "model-one", "--checkout", "."],
        0,
    );
    let first = git(root, &["rev-parse", "HEAD"]);
    fs::write(root.join("program.txt"), "second\n").unwrap();
    git(root, &["commit", "-am", "Second"]);
    let second = git(root, &["rev-parse", "HEAD"]);
    let opened = cli(
        root,
        &["model", "iteration", "open", "model-one", "--id", "it-one"],
        0,
    );
    let expect = opened["revision"].as_str().unwrap();
    // The outcome is required from one source or the other.
    let error = cli(
        root,
        &["model", "iteration", "close", "it-one", "--expect", expect],
        2,
    );
    assert_eq!(error["error"]["kind"], "invalid");
    let input = root.join("close.json");
    fs::write(
        &input,
        serde_json::to_vec(
            &json!({"outcome":"retained_separately","summary":"Why","commits":[first,second]}),
        )
        .unwrap(),
    )
    .unwrap();
    let closed = cli(
        root,
        &[
            "model",
            "iteration",
            "close",
            "it-one",
            "--from",
            input.to_str().unwrap(),
            "--expect",
            expect,
        ],
        0,
    );
    let doc: Value =
        serde_json::from_slice(&fs::read(closed["path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(doc["outcome"], "retained_separately");
    assert_eq!(doc["summary"], "Why");
    assert_eq!(doc["commits"], json!([first, second]));
    assert_eq!(doc["status"], "closed");
    for commit in [&first, &second] {
        let reference = format!("refs/accrete/retained/it-one/{commit}");
        assert_eq!(&git(root, &["rev-parse", &reference]), commit);
    }
    assert_eq!(closed["retained_refs"].as_array().unwrap().len(), 2);

    // Convenience flags are the same operation.
    let opened = cli(
        root,
        &["model", "iteration", "open", "model-one", "--id", "it-two"],
        0,
    );
    let closed = cli(
        root,
        &[
            "model",
            "iteration",
            "close",
            "it-two",
            "--outcome",
            "merged",
            "--commit",
            &first,
            "--commit",
            &second,
            "--expect",
            opened["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert_eq!(closed["retained_refs"].as_array().unwrap().len(), 2);
}

#[test]
fn retire_stores_a_reason_for_every_family_that_retires() {
    let t = setup();
    let root = t.path();
    let reason = root.join("reason.json");
    fs::write(&reason, r#"{"reason":"Superseded"}"#).unwrap();
    let source = root.join("tools/diagnostic");
    fs::create_dir_all(&source).unwrap();
    let tool = cli(
        root,
        &[
            "tool",
            "admit",
            "--id",
            "tool-one",
            "--source",
            "tools/diagnostic",
        ],
        0,
    );
    let model = cli(
        root,
        &["model", "register", "--id", "model-one", "--checkout", "."],
        0,
    );
    let data = cli(
        root,
        &[
            "data",
            "register",
            "--id",
            "data-one",
            "--path",
            "program.txt",
        ],
        0,
    );
    for (family, id, created) in [
        ("tool", "tool-one", &tool),
        ("model", "model-one", &model),
        ("data", "data-one", &data),
    ] {
        let retired = cli(
            root,
            &[
                family,
                "retire",
                id,
                "--from",
                reason.to_str().unwrap(),
                "--expect",
                created["revision"].as_str().unwrap(),
            ],
            0,
        );
        let doc: Value =
            serde_json::from_slice(&fs::read(retired["path"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(doc["reason"], "Superseded");
        assert_eq!(doc["status"], "retired");
    }
    // The file is optional.
    let plain = cli(
        root,
        &[
            "data",
            "register",
            "--id",
            "data-two",
            "--path",
            "program.txt",
        ],
        0,
    );
    let retired = cli(
        root,
        &[
            "data",
            "retire",
            "data-two",
            "--expect",
            plain["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert_eq!(retired["committed"], true);
}

#[test]
fn tool_refresh_can_select_one_agent_and_exposure_stays_out_of_shared_git() {
    let t = setup();
    let root = t.path();
    let source = root.join("tools/diagnostic");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: diagnostic\ndescription: Explain diagnostics.\n---\n",
    )
    .unwrap();
    cli(
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
    let exclude_path = root.join(git(root, &["rev-parse", "--git-path", "info/exclude"]));
    let ignore = b"/.accrete/local.toml\n";
    fs::write(root.join(".gitignore"), ignore).unwrap();
    cli(
        root,
        &["tool", "expose", "diagnostic", "--agent", "codex"],
        0,
    );
    cli(
        root,
        &["tool", "expose", "diagnostic", "--agent", "claude"],
        0,
    );
    // The repository-local exclude file holds the host's paths; .gitignore is untouched.
    let exclude = fs::read_to_string(&exclude_path).unwrap();
    for pattern in ["/.agents/skills/diagnostic", "/.claude/skills/diagnostic"] {
        assert!(exclude.lines().any(|line| line == pattern), "{exclude}");
    }
    assert_eq!(fs::read(root.join(".gitignore")).unwrap(), ignore);
    let status = git(root, &["status", "--porcelain", "--untracked-files=all"]);
    assert!(!status.contains("skills/diagnostic"), "{status}");
    let refreshed = cli(
        root,
        &["tool", "refresh", "diagnostic", "--agent", "claude"],
        0,
    );
    let exposures = refreshed["exposures"].as_array().unwrap();
    assert_eq!(exposures.len(), 1);
    assert_eq!(exposures[0]["host"], "claude");
    assert_eq!(
        cli(root, &["tool", "refresh", "diagnostic"], 0)["exposures"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn migrate_without_a_source_reports_a_current_project() {
    let t = setup();
    let root = t.path();
    let marker = fs::read(root.join(".accrete/project.toml")).unwrap();
    for args in [&["migrate"][..], &["migrate", "--dry-run"][..]] {
        let response = cli(root, args, 0);
        assert_eq!(response["current"], true);
        assert_eq!(response["changed"], false);
    }
    assert_eq!(
        fs::read(root.join(".accrete/project.toml")).unwrap(),
        marker
    );
}

#[test]
fn result_inputs_use_documented_names_and_activity_names_the_index() {
    let t = setup();
    let root = t.path();
    let input = root.join("result.json");
    fs::write(
        &input,
        r#"{"id":"result-one","activity":"Compare runs","execution":{"ranks":16},"observations":[]}"#,
    )
    .unwrap();
    cli(
        root,
        &["result", "record", "--from", input.to_str().unwrap()],
        0,
    );
    fs::write(&input, r#"{"id":"result-two"}"#).unwrap();
    cli(
        root,
        &["result", "record", "--from", input.to_str().unwrap()],
        0,
    );
    fs::write(&input, r#"{"id":"result-three","observations":{"loss":1}}"#).unwrap();
    let error = cli(
        root,
        &["result", "record", "--from", input.to_str().unwrap()],
        2,
    );
    assert_eq!(error["error"]["kind"], "invalid");
    let cfg = cli(root, &["config", "resolve"], 0);
    let conn = rusqlite::Connection::open(cfg["local_state"]["index"].as_str().unwrap()).unwrap();
    let name: Option<String> = conn
        .query_row("SELECT name FROM results WHERE id='result-one'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(name.as_deref(), Some("Compare runs"));
    let name: Option<String> = conn
        .query_row("SELECT name FROM results WHERE id='result-two'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(name, None);
}

#[test]
fn index_opens_read_only_after_a_write_and_leaves_one_closed_file() {
    use rusqlite::{Connection, OpenFlags};
    let t = setup();
    let root = t.path();
    let input = root.join("result.json");
    fs::write(&input, r#"{"id":"result-one","name":"first"}"#).unwrap();
    cli(
        root,
        &["result", "record", "--from", input.to_str().unwrap()],
        0,
    );
    let cfg = cli(root, &["config", "resolve"], 0);
    let index = cfg["local_state"]["index"].as_str().unwrap().to_owned();
    let count = || -> i64 {
        Connection::open_with_flags(&index, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT count(*) FROM results", [], |r| r.get(0))
            .unwrap()
    };
    let no_sidecars = || {
        for suffix in ["-wal", "-shm"] {
            assert!(!Path::new(&format!("{index}{suffix}")).exists(), "{suffix}");
        }
    };
    assert_eq!(count(), 1);
    no_sidecars();
    // The next write converts an index an earlier build left in WAL mode.
    Connection::open(&index)
        .unwrap()
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    fs::write(&input, r#"{"id":"result-two","name":"second"}"#).unwrap();
    cli(
        root,
        &["result", "record", "--from", input.to_str().unwrap()],
        0,
    );
    assert_eq!(count(), 2);
    no_sidecars();
}
