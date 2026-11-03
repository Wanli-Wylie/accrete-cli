use accrete::{config::Project, data, memory, migration, store::Store};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tempfile::TempDir;

fn project() -> (TempDir, Project) {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("project with spaces");
    fs::create_dir_all(root.join(".accrete")).unwrap();
    fs::write(
        root.join(".accrete/project.toml"),
        format!(
            "format = 1\nproject_id = {:?}\ntimezone = \"UTC\"\n",
            uuid::Uuid::new_v4().to_string()
        ),
    )
    .unwrap();
    let project = Project::discover(
        Some(&root),
        Some(&temporary.path().join("state")),
        Some("test-host"),
    )
    .unwrap();
    (temporary, project)
}

#[test]
fn two_data_identities_can_share_bytes_and_only_managed_replica_is_evicted() {
    let (_temp, project) = project();
    let source = project.root.join("year 1.nc");
    fs::write(&source, b"small scientific fixture\n").unwrap();
    let first = data::register(
        &project,
        json!({"id":"first","name":"forcing"}),
        Some(&source),
        None,
        true,
    )
    .unwrap();
    let repeat = data::register(
        &project,
        json!({"id":"first","name":"forcing"}),
        Some(&source),
        None,
        true,
    )
    .unwrap();
    assert_eq!(repeat["unchanged"], true);
    assert_eq!(repeat["revision"], first["revision"]);
    let other = data::register(
        &project,
        json!({"id":"second","name":"initial state"}),
        Some(&source),
        None,
        true,
    )
    .unwrap();
    assert_ne!(first["object_id"], other["object_id"]);
    let original = Store::new(&project).read("data", "first").unwrap();
    let borrowed_id = original.doc["locations"][0]["id"].as_str().unwrap();
    assert!(data::evict(&project, "first", borrowed_id, None).is_err());
    let result = data::materialize(&project, "first", "working", None, None).unwrap();
    let replica = result["location"]["path"].as_str().unwrap();
    assert_eq!(fs::read(replica).unwrap(), fs::read(&source).unwrap());
    assert_eq!(result["content_checks"][0]["algorithm"], "sha256");
    let again = data::materialize(&project, "first", "working", None, None).unwrap();
    assert_eq!(again["unchanged"], true);
    assert_eq!(again["location"], result["location"]);
    data::evict(
        &project,
        "first",
        result["location"]["id"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert!(!Path::new(replica).exists());
    assert!(source.exists());
    let after = Store::new(&project).read("data", "first").unwrap();
    assert_eq!(after.doc["locations"][1]["availability"], "evicted");
    assert_eq!(after.doc["content"], original.doc["content"]);
    assert_eq!(data::recover(&project, None).unwrap()["pending"], 0);
}

#[test]
fn eviction_selects_a_location_by_id_or_name_and_refuses_an_ambiguous_name() {
    let (_temp, project) = project();
    let source = project.root.join("input.nc");
    fs::write(&source, b"bytes\n").unwrap();
    data::register(&project, json!({"id":"one"}), Some(&source), None, false).unwrap();
    let replica = data::materialize(&project, "one", "working", None, None).unwrap();
    let stored = Store::new(&project).read("data", "one").unwrap();
    // A second current location with the same name, as a merge could produce.
    let mut doc = stored.doc.clone();
    let mut duplicate = replica["location"].clone();
    duplicate["id"] = json!("another-location");
    doc["locations"].as_array_mut().unwrap().push(duplicate);
    fs::write(&stored.path, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(
        data::evict(&project, "one", "working", None)
            .unwrap_err()
            .kind,
        "conflict"
    );
    assert_eq!(
        data::evict(&project, "one", "unknown", None)
            .unwrap_err()
            .kind,
        "invalid"
    );
    fs::write(&stored.path, serde_json::to_vec(&stored.doc).unwrap()).unwrap();
    let evicted = data::evict(&project, "one", "working", None).unwrap();
    assert_eq!(evicted["evicted_location"], replica["location"]["id"]);
}

#[test]
fn directory_registration_is_metadata_only_and_materialization_preserves_tree() {
    let (_temp, project) = project();
    let source = project.root.join("native.zarr");
    fs::create_dir_all(source.join("temperature")).unwrap();
    fs::write(source.join("zarr.json"), b"{}").unwrap();
    fs::write(source.join("temperature/0"), [1, 2, 3]).unwrap();
    let out = data::register(
        &project,
        json!({"id":"collection"}),
        Some(&source),
        Some("collection"),
        false,
    )
    .unwrap();
    let doc: Value =
        serde_json::from_slice(&fs::read(out["path"].as_str().unwrap()).unwrap()).unwrap();
    assert!(doc["content"].get("sha256").is_none());
    assert!(doc["content"].get("size_bytes").is_none());
    let out = data::materialize(&project, "collection", "working", None, None).unwrap();
    let copy = Path::new(out["location"]["path"].as_str().unwrap());
    assert_eq!(fs::read(copy.join("temperature/0")).unwrap(), vec![1, 2, 3]);
    assert!(copy.join("zarr.json").is_file());
    assert_eq!(out["content_checks"], json!([]));
}

#[test]
fn failed_digest_has_no_published_replica_and_recovery_does_not_repeat_transfer() {
    let (_temp, project) = project();
    let source = project.root.join("output.nc");
    fs::write(&source, b"new bytes").unwrap();
    data::register(
        &project,
        json!({"id":"bad-digest","content":{"sha256":"0".repeat(64)}}),
        Some(&source),
        None,
        false,
    )
    .unwrap();
    let error = data::materialize(&project, "bad-digest", "working", None, None).unwrap_err();
    assert_eq!(error.kind, "incomplete");
    assert!(error.message.contains("SHA-256 mismatch"));
    let doc = Store::new(&project).read("data", "bad-digest").unwrap();
    assert_eq!(doc.doc["locations"].as_array().unwrap().len(), 1);
    let recovery = data::recover(&project, None).unwrap();
    assert_eq!(recovery["pending"], 1);
    assert_eq!(fs::read(&source).unwrap(), b"new bytes");
}

#[cfg(unix)]
#[test]
fn explicit_directory_transfer_rejects_symlink_without_deleting_target() {
    let (_temp, project) = project();
    let source = project.root.join("linked-output");
    fs::create_dir(&source).unwrap();
    let external = project.root.join("external.nc");
    fs::write(&external, b"retained elsewhere").unwrap();
    std::os::unix::fs::symlink(&external, source.join("input.nc")).unwrap();
    data::register(&project, json!({"id":"linked"}), Some(&source), None, false).unwrap();
    assert!(data::materialize(&project, "linked", "working", None, None).is_err());
    assert_eq!(fs::read(external).unwrap(), b"retained elsewhere");
    assert_eq!(
        Store::new(&project).read("data", "linked").unwrap().doc["locations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn scaffold_migration_preserves_identity_large_values_body_dates_and_payloads() {
    let (_temp, project) = project();
    let root = &project.root;
    fs::create_dir_all(root.join("data")).unwrap();
    fs::create_dir_all(root.join("results")).unwrap();
    fs::create_dir_all(root.join("memory/2024/01/02")).unwrap();
    fs::create_dir_all(root.join("tools/analysis")).unwrap();
    fs::write(
        root.join("tools/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: An existing analysis tool\n---\nRead the data.\n",
    )
    .unwrap();
    fs::write(root.join(".gitmodules"),"[submodule \"target\"]\n\tpath = models/target\n\turl = https://example.invalid/model.git\n").unwrap();
    let payload = b"archive bytes are never unpacked or repacked";
    fs::write(root.join("data/one.tar.gz"), payload).unwrap();
    let legacy_index = format!(
        "key\torigin\tbytes\tsha256\none.tar.gz\trun-legacy\t{}\t{}\ntwo.tar.gz\tdifferent meaning\t{}\t{}\n",
        payload.len(),
        "a".repeat(64),
        payload.len(),
        "a".repeat(64)
    );
    fs::write(root.join("data/index.tsv"), &legacy_index).unwrap();
    let result = r#"{"id":"run-legacy","recorded_at":"2024-01-01T00:00:00Z","inputs":["one.tar.gz"],"models":{"target":{"name":"target","commit":"abc123"}},"tools":[{"name":"tools/analysis","commit":null}],"extra":{"integer":123456789012345678901234567890,"items":[null,true,{"a":1}]}}"#;
    fs::write(root.join("results/run-legacy.json"), result).unwrap();
    let body = "# Existing reasoning\n\nUnicode: 温度。 Commit abc123 was observed.\n";
    fs::write(root.join("memory/2024/01/02/003.md"), body).unwrap();
    fs::write(root.join("memory/2024/01/02/004.md"), "").unwrap();
    let preview = migration::migrate(&project, root, true).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(
        fs::read_to_string(root.join("results/run-legacy.json")).unwrap(),
        result
    );
    let migrated = migration::migrate(&project, root, false).unwrap();
    assert_eq!(migrated["status"], "complete");
    assert_eq!(migrated["legacy_reservations"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(root.join("data/one.tar.gz")).unwrap(), payload);
    assert_eq!(
        fs::read_to_string(root.join("data/index.tsv")).unwrap(),
        legacy_index
    );
    let record = Store::new(&project).read("result", "run-legacy").unwrap();
    assert_eq!(
        record.doc["extra"]["integer"].to_string(),
        "123456789012345678901234567890"
    );
    assert_eq!(record.doc["inputs"], json!(["one.tar.gz"]));
    assert_eq!(record.doc["models"]["target"]["commit"], "abc123");
    let note = memory::parse(&root.join("memory/2024/01/02/003.md")).unwrap();
    assert_eq!(note.body, body);
    assert!(note.metadata.get("authored_at").is_none());
    assert_eq!(
        fs::read(root.join("memory/2024/01/02/004.md")).unwrap(),
        b""
    );
    let repeated = migration::migrate(&project, root, false).unwrap();
    assert_eq!(repeated["identity_map"], migrated["identity_map"]);
    let ids = &migrated["identity_map"];
    assert_ne!(ids["data:one.tar.gz"], ids["data:two.tar.gz"]);
    let bytes = fs::read(&record.path).unwrap();
    let mut altered: Value = serde_json::from_slice(&bytes).unwrap();
    altered["owner_note"] = json!("keep this edit");
    fs::write(&record.path, serde_json::to_vec_pretty(&altered).unwrap()).unwrap();
    assert_eq!(
        migration::migrate(&project, root, false).unwrap_err().kind,
        "conflict"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(record.path).unwrap()).unwrap()["owner_note"],
        "keep this edit"
    );
}
