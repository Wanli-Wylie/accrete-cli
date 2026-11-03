//! Explicit portable exchange of small metadata documents, without payload transfer.
use crate::config::Project;
use crate::error::{Error, Result};
use crate::store::{self, Store};
use crate::{index, memory};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn export(project: &Project, destination: &Path, ids: &[String]) -> Result<Value> {
    let store = Store::new(project);
    let _lock = store.lock()?;
    let docs = index::snapshot(project)?;
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut found = HashSet::new();
    let mut objects = Vec::new();
    for item in docs {
        let id = item.doc["id"].as_str().unwrap();
        if !selected.is_empty() && !selected.contains(id) {
            continue;
        }
        found.insert(id.to_owned());
        let relative = if item.kind == "memory" {
            item.path
                .strip_prefix(&project.memory)
                .unwrap_or(&item.path)
        } else {
            item.path.strip_prefix(&project.root).unwrap_or(&item.path)
        };
        objects.push(json!({"kind":item.kind,"id":id,"path":relative,"revision":item.revision,"content":fs::read_to_string(&item.path)?}));
    }
    let missing: Vec<_> = selected.iter().filter(|id| !found.contains(**id)).collect();
    if !missing.is_empty() {
        return Err(
            Error::new("not_found", "Some requested export IDs are absent")
                .detail(json!({"missing":missing})),
        );
    }
    let bundle = json!({"format":1,"type":"accrete-metadata","project_id":project.project_id,"exported_at":store::now(),"objects":objects});
    let bytes = store::json_bytes(&bundle)?;
    let publication = store::atomic_write(destination, &bytes, false)?;
    Ok(
        json!({"committed":publication.durability=="durable","published":true,"durability":publication.durability,"path":destination,"objects":objects.len(),"revision":store::revision(&bytes),"warning":publication.warning}),
    )
}

pub fn import(project: &Project, source: &Path, allow_foreign: bool) -> Result<Value> {
    let bundle: Value = serde_json::from_slice(&fs::read(source)?)?;
    if bundle["format"].as_u64() != Some(1) || bundle["type"].as_str() != Some("accrete-metadata") {
        return Err(Error::new(
            "invalid",
            "Expected an ACCRETE metadata exchange bundle, format 1",
        ));
    }
    if !allow_foreign && bundle["project_id"].as_str() != Some(project.project_id.as_str()) {
        return Err(Error::new(
            "conflict",
            "Bundle belongs to another project; use --allow-foreign for an explicit cross-project import",
        ));
    }
    let entries = bundle["objects"]
        .as_array()
        .ok_or_else(|| Error::new("invalid", "Bundle objects must be an array"))?;
    let mut outcomes = Vec::new();
    let mut failed = 0usize;
    for entry in entries {
        match import_one(project, entry, allow_foreign) {
            Ok(mut outcome) => {
                outcome["ok"] = json!(true);
                outcomes.push(outcome);
            }
            Err(error) => {
                failed += 1;
                outcomes.push(json!({"ok":false,"object_id":entry["id"],"error":{"kind":error.kind,"message":error.message,"details":error.details}}));
            }
        }
    }
    Ok(
        json!({"committed":failed < entries.len() || entries.is_empty(),"atomic":false,"partial":failed>0 && failed<entries.len(),"failed":failed,"succeeded":entries.len()-failed,"outcomes":outcomes}),
    )
}

fn import_one(project: &Project, entry: &Value, allow_foreign: bool) -> Result<Value> {
    let kind = entry["kind"]
        .as_str()
        .ok_or_else(|| Error::new("invalid", "Exchange entry kind is missing"))?;
    let content = entry["content"]
        .as_str()
        .ok_or_else(|| Error::new("invalid", "Exchange entry content is missing"))?;
    if let Some(expected_hash) = entry["revision"].as_str() {
        if expected_hash != store::revision(content.as_bytes()) {
            return Err(Error::new(
                "conflict",
                "Exchange content does not match its revision",
            ));
        }
    }
    let expect = entry.get("expected_revision").and_then(Value::as_str);
    if kind == "memory" {
        let preferred = entry.get("path").and_then(Value::as_str).map(Path::new);
        let decoded = memory::decode(
            content.as_bytes(),
            preferred.unwrap_or(Path::new("import.md")),
        )?
        .ok_or_else(|| {
            Error::new(
                "invalid",
                "Exchange Memory content is not a managed Memory document",
            )
        })?;
        validate_identity(project, &decoded.metadata, entry, allow_foreign)?;
        return memory::import_entry(project, decoded.metadata, &decoded.body, preferred, expect);
    }
    let doc: Value = serde_json::from_str(content)?;
    store::validate_document(&doc, Some(kind))?;
    validate_identity(project, &doc, entry, allow_foreign)?;
    let id = doc["id"].as_str().unwrap();
    let store = Store::new(project);
    let _lock = store.lock()?;
    let path = store.path(kind, id)?;
    match store.read(kind, id) {
        Ok(existing) => {
            if existing.doc == doc {
                return store.existing(&doc, &existing.path, &existing.revision);
            }
            if expect != Some(existing.revision.as_str()) {
                return Err(Error::new(
                    "conflict",
                    "Incoming document diverges from an existing stable ID",
                )
                .detail(
                    json!({"object_id":id,"path":existing.path,"revision":existing.revision}),
                ));
            }
            for key in ["id", "kind", "format", "project_id", "created_at"] {
                if existing.doc.get(key) != doc.get(key) {
                    return Err(Error::new(
                        "conflict",
                        format!("Import replacement cannot change {key}"),
                    ));
                }
            }
            let publication = store::atomic_write(&existing.path, content.as_bytes(), true)?;
            store.finish(&doc, &existing.path, content.as_bytes(), publication)
        }
        Err(error) if error.kind == "not_found" => {
            if expect.is_some() {
                return Err(Error::new("conflict", "Expected predecessor is absent"));
            }
            if store::has_id(project, id)? {
                return Err(Error::new(
                    "conflict",
                    "Imported ID belongs to another object family",
                ));
            }
            let publication = store::atomic_write(&path, content.as_bytes(), false)?;
            store.finish(&doc, &path, content.as_bytes(), publication)
        }
        Err(error) => Err(error),
    }
}

fn validate_identity(
    project: &Project,
    doc: &Value,
    entry: &Value,
    allow_foreign: bool,
) -> Result<()> {
    if doc["id"] != entry["id"] || doc["kind"] != entry["kind"] {
        return Err(Error::new(
            "invalid",
            "Exchange envelope and content identity disagree",
        ));
    }
    if !allow_foreign && doc["project_id"].as_str() != Some(project.project_id.as_str()) {
        return Err(Error::new(
            "conflict",
            "Exchange document belongs to another project",
        ));
    }
    Ok(())
}
