use accrete::{
    config::Project,
    exchange, index, memory,
    store::{self, Store},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{collections::HashSet, fs, path::Path};
use tempfile::TempDir;

fn project() -> (TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".accrete")).unwrap();
    fs::write(dir.path().join(".accrete/project.toml"), format!("format = 1\nproject_id = \"{}\"\ntimezone = \"Asia/Shanghai\"\n[local_state]\nbase = \".local-state\"\n",uuid::Uuid::new_v4())).unwrap();
    let project = Project::discover(Some(dir.path()), None, Some("test-host")).unwrap();
    (dir, project)
}

#[test]
fn creation_retries_and_byte_revisions_preserve_unknown_fields() {
    let (_dir, p) = project();
    let store = Store::new(&p);
    let payload: Value = serde_json::from_str(r#"{"id":"result-1","name":"trial","custom":{"integer":1234567890123456789012345678901234567890,"list":[null,true,3]}}"#).unwrap();
    let first = store.create("result", payload.clone()).unwrap();
    let retry = store.create("result", payload).unwrap();
    assert_eq!(retry["unchanged"], true);
    assert_eq!(first["revision"], retry["revision"]);
    let first_path = Path::new(first["path"].as_str().unwrap());
    let bytes = fs::read(first_path).unwrap();
    assert_eq!(first["revision"], store::revision(&bytes));
    let mut external: Value = serde_json::from_slice(&bytes).unwrap();
    external["external_note"] = json!("native editing happened");
    fs::write(first_path, serde_json::to_vec(&external).unwrap()).unwrap();
    let stale = store
        .amend(
            "result",
            "result-1",
            json!({"name":"lost update"}),
            first["revision"].as_str().unwrap(),
        )
        .unwrap_err();
    assert_eq!(stale.kind, "conflict");
    let current = store.read("result", "result-1").unwrap();
    store
        .amend(
            "result",
            "result-1",
            json!({"name":"merged title"}),
            &current.revision,
        )
        .unwrap();
    let new = store.read("result", "result-1").unwrap().doc;
    assert_eq!(new["external_note"], "native editing happened");
    assert_eq!(
        new["custom"]["integer"].to_string(),
        "1234567890123456789012345678901234567890"
    );
}

#[test]
fn concurrent_memory_publications_allocate_unique_complete_files() {
    let (_dir, p) = project();
    let responses: Vec<Value> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..12)
            .map(|n| {
                let p = &p;
                scope.spawn(move || {
                    memory::append(
                        p,
                        &format!("# Entry {n}\n\nComplete authored text.\n"),
                        memory::AppendOptions::default(),
                    )
                    .unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let paths: HashSet<_> = responses
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    let ids: HashSet<_> = responses
        .iter()
        .map(|r| r["object_id"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 12);
    assert_eq!(ids.len(), 12);
    for path in paths {
        let raw = fs::read_to_string(path).unwrap();
        assert!(raw.contains("\nformat: 1\n"));
        assert!(!raw.contains("$serde_json::private::Number"));
        let parsed = memory::parse(Path::new(path)).unwrap();
        assert!(parsed.body.contains("Complete authored text."));
        assert_eq!(parsed.metadata["project_id"], p.project_id);
    }
    let conn = Connection::open(&p.index_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM memory_entries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 12);
}

#[test]
fn memory_frontmatter_preserves_numbers_without_private_serializer_tokens() {
    let metadata: Value = serde_json::from_str(r#"{"format":1,"kind":"memory","id":"huge-number","project_id":"project","extension":{"number":1234567890123456789012345678901234567890}}"#).unwrap();
    let bytes = memory::encode(&metadata, "Original prose.\n").unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains("$serde_json::private::Number"));
    let parsed = memory::decode(&bytes, Path::new("memory.md"))
        .unwrap()
        .unwrap();
    assert_eq!(parsed.metadata, metadata);
    assert_eq!(parsed.body, "Original prose.\n");
}

#[test]
fn index_unavailable_does_not_reverse_publication_or_duplicate_retry() {
    let (_dir, p) = project();
    fs::create_dir_all(&p.index_path).unwrap();
    let payload = json!({"id":"published","name":"finished simulation"});
    let response = Store::new(&p).create("result", payload.clone()).unwrap();
    assert_eq!(response["committed"], true);
    assert_eq!(response["index_pending"], true);
    assert!(Path::new(response["path"].as_str().unwrap()).is_file());
    let retry = Store::new(&p).create("result", payload.clone()).unwrap();
    assert_eq!(retry["unchanged"], true);
    assert_eq!(retry["index_pending"], true);
    fs::remove_dir(&p.index_path).unwrap();
    let retry = Store::new(&p).create("result", payload).unwrap();
    assert_eq!(retry["index_pending"], false);
    let conn = Connection::open(&p.index_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM results", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn failed_projection_keeps_previous_transaction_for_native_readers() {
    let (_dir, p) = project();
    let response = Store::new(&p).create("result",json!({"id":"one","inputs":["remote-input"],"models":[{"id":"external-model","commit":"abcdef","role":"reference"}]})).unwrap();
    let original = fs::read(response["path"].as_str().unwrap()).unwrap();
    let bad = p.results.join("bad.json");
    for content in [b"{bad".to_vec(), original.clone()] {
        fs::write(&bad, content).unwrap();
        assert!(index::refresh(&p).is_err());
        let conn = Connection::open(&p.index_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM results", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let model: String = conn
            .query_row("SELECT model_id FROM result_models", [], |row| row.get(0))
            .unwrap();
        assert_eq!(model, "external-model");
    }
    let mut future: Value = serde_json::from_slice(&original).unwrap();
    future["id"] = json!("future");
    future["format"] = json!(99);
    fs::write(&bad, serde_json::to_vec(&future).unwrap()).unwrap();
    assert_eq!(index::refresh(&p).unwrap_err().kind, "unsupported_format");
    fs::remove_file(&bad).unwrap();
    assert_eq!(index::refresh(&p).unwrap()["objects"], 1);
}

#[test]
fn memory_import_renumbers_filename_and_retries_keep_identity() {
    let (_source_dir, source) = project();
    let (_dest_dir, dest) = project();
    let body = "# Imported understanding\n\nThe original prose stays byte-for-byte.\n";
    let source_response = memory::append(
        &source,
        body,
        memory::AppendOptions {
            id: Some("remote-memory".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let source_path = Path::new(source_response["path"].as_str().unwrap());
    let relative = source_path.strip_prefix(&source.memory).unwrap();
    let collision = dest.memory.join(relative);
    fs::create_dir_all(collision.parent().unwrap()).unwrap();
    fs::write(&collision, "Unmanaged historical note.\n").unwrap();
    let bundle = source.root.join("exchange.json");
    exchange::export(&source, &bundle, &[]).unwrap();
    assert!(exchange::import(&dest, &bundle, false).is_err());
    let imported = exchange::import(&dest, &bundle, true).unwrap();
    assert_eq!(imported["failed"], 0);
    let imported_path = Path::new(imported["outcomes"][0]["path"].as_str().unwrap());
    assert_ne!(imported_path, collision);
    let parsed = memory::parse(imported_path).unwrap();
    assert_eq!(parsed.body, body);
    assert_eq!(parsed.metadata["id"], "remote-memory");
    assert_eq!(
        fs::read_to_string(&collision).unwrap(),
        "Unmanaged historical note.\n"
    );
    let retry = exchange::import(&dest, &bundle, true).unwrap();
    assert_eq!(retry["failed"], 0);
    assert_eq!(retry["outcomes"][0]["unchanged"], true);
    assert_eq!(
        retry["outcomes"][0]["path"],
        imported["outcomes"][0]["path"]
    );
}

#[test]
fn divergent_import_preserves_current_object_and_can_apply_explicit_predecessor() {
    let (_dir, p) = project();
    let result = Store::new(&p)
        .create("result", json!({"id":"stable","name":"current"}))
        .unwrap();
    let bundle_path = p.root.join("bundle.json");
    exchange::export(&p, &bundle_path, &[]).unwrap();
    let mut bundle: Value = serde_json::from_slice(&fs::read(&bundle_path).unwrap()).unwrap();
    let entry = &mut bundle["objects"][0];
    let mut doc: Value = serde_json::from_str(entry["content"].as_str().unwrap()).unwrap();
    doc["name"] = json!("incoming");
    let content = String::from_utf8(store::json_bytes(&doc).unwrap()).unwrap();
    entry["revision"] = json!(store::revision(content.as_bytes()));
    entry["content"] = json!(content);
    fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    let outcome = exchange::import(&p, &bundle_path, false).unwrap();
    assert_eq!(outcome["failed"], 1);
    assert_eq!(
        Store::new(&p).read("result", "stable").unwrap().doc["name"],
        "current"
    );
    bundle["objects"][0]["expected_revision"] = result["revision"].clone();
    fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(
        exchange::import(&p, &bundle_path, false).unwrap()["failed"],
        0
    );
    assert_eq!(
        Store::new(&p).read("result", "stable").unwrap().doc["name"],
        "incoming"
    );
}

#[test]
fn memory_migration_preserves_historical_path_and_body_without_inventing_time() {
    let (_dir, p) = project();
    let path = p.memory.join("2020/01/01/003.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body = "Model abc123\n\nAn old observation.\n";
    fs::write(&path, body).unwrap();
    let meta = json!({"format":1,"kind":"memory","id":"legacy-mem","project_id":p.project_id,"legacy_path":"2020/01/01/003.md"});
    let result = memory::publish_migration(
        &p,
        meta.clone(),
        body,
        &path,
        &store::revision(body.as_bytes()),
    )
    .unwrap();
    assert_eq!(result["path"], path.to_string_lossy().as_ref());
    let parsed = memory::parse(&path).unwrap();
    assert_eq!(parsed.body, body);
    assert!(parsed.metadata.get("authored_at").is_none());
    let retry = memory::publish_migration(&p, meta, body, &path, &store::revision(body.as_bytes()))
        .unwrap();
    assert_eq!(retry["unchanged"], true);
}
