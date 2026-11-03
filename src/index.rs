//! A disposable, per-view SQLite projection. Documents remain primary.
use crate::config::Project;
use crate::error::{Error, Result};
use crate::memory;
use crate::store::{self, Store};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

pub const INDEX_FORMAT: u32 = 1;

pub(crate) struct SnapshotDocument {
    pub kind: String,
    pub doc: Value,
    pub path: PathBuf,
    pub revision: String,
    pub body: Option<String>,
}

pub(crate) fn snapshot(project: &Project) -> Result<Vec<SnapshotDocument>> {
    let store = Store::new(project);
    let mut docs = Vec::new();
    let mut seen = HashMap::<String, PathBuf>::new();
    for kind in store::KINDS {
        for path in store::document_paths(&store.directory(kind)?, "json")? {
            let bytes = fs::read(&path)?;
            let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
                Error::new("invalid", format!("Cannot project {}: {e}", path.display()))
            })?;
            store::validate_document(&doc, Some(kind))?;
            let id = doc["id"].as_str().unwrap().to_owned();
            if let Some(previous) = seen.insert(id.clone(), path.clone()) {
                return Err(Error::new(
                    "conflict",
                    format!("Duplicate object ID {id}; previous projection is preserved"),
                )
                .detail(json!({"paths":[previous,path]})));
            }
            docs.push(SnapshotDocument {
                kind: kind.into(),
                doc,
                path,
                revision: store::revision(&bytes),
                body: None,
            });
        }
    }
    for path in store::document_paths(&project.memory, "md")? {
        if let Some(entry) = memory::parse_optional(&path)? {
            let id = entry.metadata["id"].as_str().unwrap().to_owned();
            if let Some(previous) = seen.insert(id.clone(), path.clone()) {
                return Err(Error::new(
                    "conflict",
                    format!("Duplicate object ID {id}; previous projection is preserved"),
                )
                .detail(json!({"paths":[previous,path]})));
            }
            docs.push(SnapshotDocument {
                kind: "memory".into(),
                doc: entry.metadata,
                path,
                revision: entry.revision,
                body: Some(entry.body),
            });
        }
    }
    if docs.len() > 100_000 {
        return Err(Error::new(
            "storage",
            "Metadata snapshot exceeds 100,000 documents",
        ));
    }
    Ok(docs)
}

pub fn refresh(project: &Project) -> Result<Value> {
    let store = Store::new(project);
    let _lock = store.lock()?;
    refresh_locked(project)
}

/// Caller holds the project's metadata mutation lock.
pub fn refresh_locked(project: &Project) -> Result<Value> {
    // Finish reading/validating before touching a previously complete projection.
    let docs = snapshot(project)?;
    if let Some(parent) = project.index_path.parent() {
        store::create_dirs(parent)?;
    }
    let mut conn = Connection::open(&project.index_path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // Between invocations the index is one closed file, which a read-only
    // connection can open. A WAL database needs its -wal and -shm files recreated.
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 0 && version != INDEX_FORMAT {
        return Err(Error::new(
            "unsupported_format",
            format!(
                "Unsupported projection format {version}; discard this disposable index before rebuilding"
            ),
        ));
    }
    let state_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='index_state')",
        [],
        |row| row.get(0),
    )?;
    if state_exists {
        let binding: Option<String> = conn
            .query_row(
                "SELECT view_key FROM index_state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if binding
            .as_deref()
            .is_some_and(|key| key != project.view_key)
        {
            return Err(Error::new(
                "config",
                "SQLite projection belongs to another metadata view",
            ));
        }
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(SCHEMA)?;
    for table in [
        "models",
        "iterations",
        "tools",
        "data_objects",
        "data_locations",
        "results",
        "result_models",
        "result_tools",
        "result_data",
        "memory_entries",
        "memory_links",
    ] {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }
    for item in &docs {
        let doc = &item.doc;
        let id = doc["id"].as_str().unwrap();
        let document = serde_json::to_string(doc)?;
        let name = first_str(doc, &["name", "title", "activity"]);
        let status = doc.get("status").and_then(Value::as_str);
        let project_id = doc["project_id"].as_str().unwrap();
        let path = item.path.to_string_lossy();
        match item.kind.as_str() {
            "model" | "tool" | "data" | "result" => {
                let table = match item.kind.as_str() {
                    "model" => "models",
                    "tool" => "tools",
                    "data" => "data_objects",
                    _ => "results",
                };
                tx.execute(&format!("INSERT INTO {table}(id,name,project_id,status,path,revision,document) VALUES(?1,?2,?3,?4,?5,?6,?7)"),
                    params![id,name,project_id,status,path.as_ref(),item.revision,document])?;
            }
            "iteration" => {
                tx.execute("INSERT INTO iterations(id,name,project_id,status,path,revision,document,model_id,starting_revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![id,name,project_id,status,path.as_ref(),item.revision,document,first_str(doc,&["model_id","model"]),first_str(doc,&["starting_revision","start_revision"])])?;
            }
            "memory" => {
                tx.execute("INSERT INTO memory_entries(id,name,project_id,path,revision,document,authored_at,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![id,name,project_id,path.as_ref(),item.revision,document,doc["authored_at"].as_str(),item.body.as_deref().unwrap_or("")])?;
                for (ordinal, link) in references(doc.get("links"), None).into_iter().enumerate() {
                    tx.execute("INSERT INTO memory_links(memory_id,ordinal,target_id,kind,relation,document) VALUES(?1,?2,?3,?4,?5,?6)",
                        params![id,ordinal as i64,first_str(&link,&["id","target_id","object_id"]),first_str(&link,&["kind","type"]),first_str(&link,&["relation","role"]),serde_json::to_string(&link)?])?;
                }
            }
            _ => unreachable!(),
        }
        if item.kind == "data" {
            for (ordinal, location) in references(doc.get("locations"), None)
                .into_iter()
                .enumerate()
            {
                tx.execute("INSERT INTO data_locations(data_id,ordinal,location_id,name,kind,host,path,managed,availability,document) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![id,ordinal as i64,first_str(&location,&["id","location_id"]),location["name"].as_str(),location["kind"].as_str(),location["host"].as_str(),first_str(&location,&["path","uri"]),location["managed"].as_bool().unwrap_or(false),location["availability"].as_str(),serde_json::to_string(&location)?])?;
            }
        }
        if item.kind == "result" {
            for (ordinal, link) in references(doc.get("models"), None).into_iter().enumerate() {
                tx.execute("INSERT INTO result_models(result_id,ordinal,model_id,role,commit_id,document) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![id,ordinal as i64,first_str(&link,&["model_id","id","model"]),link["role"].as_str(),first_str(&link,&["commit","commit_id","revision"]),serde_json::to_string(&link)?])?;
            }
            for (ordinal, link) in references(doc.get("tools"), None).into_iter().enumerate() {
                tx.execute("INSERT INTO result_tools(result_id,ordinal,tool_id,revision_used,document) VALUES(?1,?2,?3,?4,?5)",
                    params![id,ordinal as i64,first_str(&link,&["tool_id","id","tool"]),first_str(&link,&["commit","revision","source_revision"]),serde_json::to_string(&link)?])?;
            }
            let mut links = Vec::new();
            links.extend(references(doc.get("inputs"), Some("input")));
            links.extend(references(doc.get("outputs"), Some("output")));
            links.extend(references(doc.get("data"), None));
            for (ordinal, link) in links.into_iter().enumerate() {
                tx.execute("INSERT INTO result_data(result_id,ordinal,data_id,role,document) VALUES(?1,?2,?3,?4,?5)",
                    params![id,ordinal as i64,first_str(&link,&["data_id","id","data"]),first_str(&link,&["role","direction"]),serde_json::to_string(&link)?])?;
            }
        }
    }
    let refreshed_at = store::now();
    tx.execute("INSERT INTO index_state(singleton,format,view_key,project_id,refreshed_at,object_count) VALUES(1,?1,?2,?3,?4,?5) ON CONFLICT(singleton) DO UPDATE SET format=excluded.format,view_key=excluded.view_key,project_id=excluded.project_id,refreshed_at=excluded.refreshed_at,object_count=excluded.object_count",
        params![INDEX_FORMAT,project.view_key,project.project_id,refreshed_at,docs.len() as i64])?;
    tx.pragma_update(None, "user_version", INDEX_FORMAT)?;
    tx.commit()?;
    Ok(
        json!({"committed":true,"index_pending":false,"path":project.index_path,"view_key":project.view_key,"format":INDEX_FORMAT,"objects":docs.len(),"refreshed_at":refreshed_at}),
    )
}

fn first_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
}

/// Canonical arrays plus legacy role-keyed mappings; original shape stays in document.
fn references(value: Option<&Value>, role: Option<&str>) -> Vec<Value> {
    let mut entries = Vec::new();
    match value {
        Some(Value::Array(items)) => entries.extend(items.iter().cloned()),
        Some(Value::Object(map))
            if map.contains_key("id")
                || map.contains_key("model_id")
                || map.contains_key("tool_id")
                || map.contains_key("data_id")
                || map.contains_key("target_id")
                || map.contains_key("path") =>
        {
            entries.push(Value::Object(map.clone()))
        }
        Some(Value::Object(map)) => {
            for (key, value) in map {
                let mut entry = if value.is_object() {
                    value.clone()
                } else if value.is_string() {
                    json!({"id":value})
                } else {
                    continue;
                };
                if entry.get("role").is_none() {
                    entry["role"] = json!(key);
                }
                entries.push(entry);
            }
        }
        Some(Value::String(id)) => entries.push(json!({"id":id})),
        _ => {}
    }
    entries
        .into_iter()
        .filter_map(|mut entry| {
            if let Some(id) = entry.as_str() {
                entry = json!({"id":id});
            }
            if !entry.is_object() {
                return None;
            }
            if let Some(role) = role {
                if entry.get("role").is_none() {
                    entry["role"] = json!(role);
                }
            }
            Some(entry)
        })
        .collect()
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS index_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),format INTEGER NOT NULL,view_key TEXT NOT NULL,project_id TEXT NOT NULL,refreshed_at TEXT NOT NULL,object_count INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS models(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,status TEXT,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS iterations(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,status TEXT,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL,model_id TEXT,starting_revision TEXT);
CREATE TABLE IF NOT EXISTS tools(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,status TEXT,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS data_objects(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,status TEXT,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS data_locations(data_id TEXT NOT NULL,ordinal INTEGER NOT NULL,location_id TEXT,name TEXT,kind TEXT,host TEXT,path TEXT,managed INTEGER,availability TEXT,document TEXT NOT NULL,PRIMARY KEY(data_id,ordinal));
CREATE TABLE IF NOT EXISTS results(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,status TEXT,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS result_models(result_id TEXT NOT NULL,ordinal INTEGER NOT NULL,model_id TEXT,role TEXT,commit_id TEXT,document TEXT NOT NULL,PRIMARY KEY(result_id,ordinal));
CREATE TABLE IF NOT EXISTS result_tools(result_id TEXT NOT NULL,ordinal INTEGER NOT NULL,tool_id TEXT,revision_used TEXT,document TEXT NOT NULL,PRIMARY KEY(result_id,ordinal));
CREATE TABLE IF NOT EXISTS result_data(result_id TEXT NOT NULL,ordinal INTEGER NOT NULL,data_id TEXT,role TEXT,document TEXT NOT NULL,PRIMARY KEY(result_id,ordinal));
CREATE TABLE IF NOT EXISTS memory_entries(id TEXT PRIMARY KEY,name TEXT,project_id TEXT NOT NULL,path TEXT NOT NULL,revision TEXT NOT NULL,document TEXT NOT NULL,authored_at TEXT,body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS memory_links(memory_id TEXT NOT NULL,ordinal INTEGER NOT NULL,target_id TEXT,kind TEXT,relation TEXT,document TEXT NOT NULL,PRIMARY KEY(memory_id,ordinal));
CREATE INDEX IF NOT EXISTS result_models_model ON result_models(model_id);
CREATE INDEX IF NOT EXISTS result_tools_tool ON result_tools(tool_id);
CREATE INDEX IF NOT EXISTS result_data_data ON result_data(data_id);
CREATE INDEX IF NOT EXISTS memory_links_target ON memory_links(target_id);
"#;
