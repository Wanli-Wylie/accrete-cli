//! File-primary publication shared by every object family.
use crate::config::Project;
use crate::error::{Error, Result};
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

pub const KINDS: [&str; 5] = ["model", "iteration", "tool", "data", "result"];

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 180
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err(Error::new(
            "invalid",
            "Object IDs must be 1–180 ASCII letters, digits, '.', '_' or '-' and cannot be '.' or '..'",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Publication {
    pub published: bool,
    pub durability: String,
    pub warning: Option<String>,
}

pub fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Creates missing directory entries and synchronizes their parents on Unix.
pub fn create_dirs(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        if parent != path && !parent.as_os_str().is_empty() {
            create_dirs(parent)?;
        }
    }
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
        Err(e) => return Err(e),
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            sync_directory(parent)?;
        }
    }
    sync_directory(path)
}

/// Writes a flushed temporary sibling, then atomically publishes it.
/// A directory sync failure after publication is an explicit visible outcome.
pub fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<Publication> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("invalid", "Publication needs a parent directory"))?;
    create_dirs(parent)?;
    if path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(Error::new(
            "conflict",
            format!("Refusing to publish over symlink {}", path.display()),
        ));
    }
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if replace {
        temp.persist(path).map_err(|e| Error::from(e.error))?;
    } else {
        temp.persist_noclobber(path).map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new(
                    "conflict",
                    format!("Destination already exists: {}", path.display()),
                )
            } else {
                Error::from(e.error)
            }
        })?;
    }
    match sync_directory(parent) {
        Ok(()) => Ok(Publication {
            published: true,
            durability: "durable".into(),
            warning: None,
        }),
        Err(e) => Ok(Publication {
            published: true,
            durability: "uncertain".into(),
            warning: Some(e.to_string()),
        }),
    }
}

pub struct ViewLock {
    _file: File,
}
impl Drop for ViewLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._file);
    }
}

#[derive(Debug, Clone)]
pub struct Stored {
    pub doc: Value,
    pub path: PathBuf,
    pub revision: String,
}

pub struct Store<'a> {
    pub project: &'a Project,
}
impl<'a> Store<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    pub fn lock(&self) -> Result<ViewLock> {
        create_dirs(&self.project.state_dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.project.state_dir.join("mutation.lock"))?;
        FileExt::lock_exclusive(&file)?;
        let lock = ViewLock { _file: file };
        let binding = json!({"format":1,"view_key":self.project.view_key,"project_id":self.project.project_id,
            "metadata":self.project.metadata,"results":self.project.results,"memory":self.project.memory});
        let binding_path = self.project.state_dir.join("view.json");
        if binding_path.exists() {
            let current: Value = serde_json::from_slice(&fs::read(&binding_path)?)?;
            if current != binding {
                return Err(Error::new(
                    "config",
                    "Local state directory is bound to a different metadata view",
                )
                .detail(json!({"path":binding_path,"expected":binding,"actual":current})));
            }
        } else {
            let result = atomic_write(&binding_path, &json_bytes(&binding)?, false)?;
            if result.durability == "uncertain" {
                return Err(Error::new(
                    "durability",
                    "Metadata-view binding was published but directory synchronization failed",
                )
                .detail(json!({"published":true,"durability":"uncertain","path":binding_path})));
            }
        }
        Ok(lock)
    }

    pub fn directory(&self, kind: &str) -> Result<PathBuf> {
        match kind {
            "model" => Ok(self.project.metadata.join("models")),
            "iteration" => Ok(self.project.metadata.join("iterations")),
            "tool" => Ok(self.project.metadata.join("tools")),
            "data" => Ok(self.project.metadata.join("data")),
            "result" => Ok(self.project.results.clone()),
            _ => Err(Error::new(
                "invalid",
                format!("Unsupported document kind: {kind}"),
            )),
        }
    }

    pub fn path(&self, kind: &str, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.directory(kind)?.join(format!("{id}.json")))
    }

    pub fn read(&self, kind: &str, selector: &str) -> Result<Stored> {
        let dir = self.directory(kind)?;
        let mut exact = Vec::new();
        let mut aliases = Vec::new();
        for path in document_paths(&dir, "json")? {
            let bytes = fs::read(&path)?;
            let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
                Error::new(
                    "invalid",
                    format!("Malformed JSON at {}: {e}", path.display()),
                )
            })?;
            validate_document(&doc, Some(kind))?;
            let exact_match = doc.get("id").and_then(Value::as_str) == Some(selector);
            let matched_alias = doc.get("name").and_then(Value::as_str) == Some(selector)
                || doc
                    .get("aliases")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().any(|v| v.as_str() == Some(selector)))
                    .unwrap_or(false);
            if exact_match {
                exact.push(Stored {
                    doc,
                    path,
                    revision: revision(&bytes),
                });
            } else if matched_alias {
                aliases.push(Stored {
                    doc,
                    path,
                    revision: revision(&bytes),
                });
            }
        }
        let mut found = if exact.is_empty() { aliases } else { exact };
        match found.len() {
            0 => Err(Error::new(
                "not_found",
                format!("No {kind} named {selector}"),
            )),
            1 => Ok(found.remove(0)),
            _ => Err(Error::new(
                "conflict",
                format!("Ambiguous {kind} ID or alias {selector}"),
            )
            .detail(json!({"paths":found.iter().map(|v| &v.path).collect::<Vec<_>>()}))),
        }
    }

    pub fn create(&self, kind: &str, payload: Value) -> Result<Value> {
        let lock = self.lock()?;
        self.create_locked(kind, payload, &lock)
    }

    pub fn create_locked(&self, kind: &str, mut payload: Value, _lock: &ViewLock) -> Result<Value> {
        let map = payload
            .as_object_mut()
            .ok_or_else(|| Error::new("invalid", "Object payload must be a JSON object"))?;
        let id = match map.get("id") {
            Some(Value::String(id)) => id.clone(),
            Some(_) => return Err(Error::new("invalid", "Object id must be a string")),
            None => uuid::Uuid::new_v4().to_string(),
        };
        valid_id(&id)?;
        for (key, expected) in [
            ("format", json!(1)),
            ("kind", json!(kind)),
            ("project_id", json!(self.project.project_id)),
        ] {
            if let Some(value) = map.get(key) {
                if value != &expected {
                    return Err(Error::new(
                        "invalid",
                        format!("Reserved field {key} does not match this operation"),
                    ));
                }
            }
            map.insert(key.into(), expected);
        }
        map.insert("id".into(), json!(id));
        let path = self.path(kind, &id)?;
        let existing = match self.read(kind, &id) {
            Ok(v) if v.doc["id"].as_str() == Some(id.as_str()) => Some(v),
            Ok(_) => None,
            Err(e) if e.kind == "not_found" => None,
            Err(e) => return Err(e),
        };
        if let Some(existing) = existing {
            if !payload.as_object().unwrap().contains_key("created_at") {
                if let Some(value) = existing.doc.get("created_at") {
                    payload["created_at"] = value.clone();
                }
            }
            if existing.doc == payload {
                return self.existing(&existing.doc, &existing.path, &existing.revision);
            }
            return Err(Error::new("conflict", format!("Object {id} already exists with different content; use an amendment with --expect")).detail(json!({"object_id":id,"path":existing.path,"revision":existing.revision})));
        }
        if has_id(self.project, &id)? {
            return Err(Error::new(
                "conflict",
                format!("Object ID {id} already belongs to another document"),
            ));
        }
        if payload.get("created_at").is_none() {
            payload["created_at"] = json!(now());
        }
        validate_document(&payload, Some(kind))?;
        let bytes = json_bytes(&payload)?;
        let publication = atomic_write(&path, &bytes, false)?;
        self.finish(&payload, &path, &bytes, publication)
    }

    pub fn amend(&self, kind: &str, selector: &str, patch: Value, expect: &str) -> Result<Value> {
        if !patch.is_object() {
            return Err(Error::new("invalid", "An amendment must be a JSON object"));
        }
        self.update(kind, selector, Some(expect), |doc| {
            merge_patch(doc, &patch);
            Ok(())
        })
    }

    pub fn update<F>(
        &self,
        kind: &str,
        selector: &str,
        expected: Option<&str>,
        apply: F,
    ) -> Result<Value>
    where
        F: FnOnce(&mut Value) -> Result<()>,
    {
        let _lock = self.lock()?;
        let mut existing = self.read(kind, selector)?;
        if let Some(expected) = expected {
            if expected != existing.revision {
                return Err(Error::new("conflict", "Document changed since the supplied revision").detail(json!({"object_id":existing.doc["id"],"path":existing.path,"expected":expected,"revision":existing.revision})));
            }
        }
        let original = existing.doc.clone();
        apply(&mut existing.doc)?;
        for key in ["id", "kind", "format", "project_id", "created_at"] {
            if existing.doc.get(key) != original.get(key) {
                return Err(Error::new(
                    "invalid",
                    format!("An amendment cannot change reserved field {key}"),
                ));
            }
        }
        validate_document(&existing.doc, Some(kind))?;
        if existing.doc == original {
            return self.existing(&existing.doc, &existing.path, &existing.revision);
        }
        existing.doc["updated_at"] = json!(now());
        let bytes = json_bytes(&existing.doc)?;
        let publication = atomic_write(&existing.path, &bytes, true)?;
        self.finish(&existing.doc, &existing.path, &bytes, publication)
    }

    /// Must be called while this view's metadata lock is held.
    pub fn finish(
        &self,
        doc: &Value,
        path: &Path,
        bytes: &[u8],
        publication: Publication,
    ) -> Result<Value> {
        let revision = revision(bytes);
        let mut response = json!({"object_id":doc["id"],"path":path,"revision":revision,
            "committed":publication.durability == "durable","published":publication.published,
            "durability":publication.durability,"index_pending":false,"unchanged":false});
        if let Some(warning) = publication.warning {
            response["warning"] = json!(warning);
        }
        if response["durability"] == "uncertain" {
            let receipt = json!({"format":1,"operation":"publication_sync","object_id":doc["id"],"path":path,"revision":revision});
            let receipt_path = self.project.state_dir.join("receipts").join(format!(
                "publication-{}.json",
                doc["id"].as_str().unwrap_or("unknown")
            ));
            if let Some(parent) = receipt_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(bytes) = json_bytes(&receipt) {
                let _ = fs::write(&receipt_path, bytes);
            }
            response["receipt"] = json!(receipt_path);
        }
        if let Err(error) = crate::index::refresh_locked(self.project) {
            response["index_pending"] = json!(true);
            response["index_error"] = json!(error.to_string());
        }
        Ok(response)
    }

    /// Reconcile the disposable projection when acknowledging an unchanged object.
    pub fn existing(&self, doc: &Value, path: &Path, revision: &str) -> Result<Value> {
        let mut response = existing_response(doc, path, revision);
        if let Err(error) = crate::index::refresh_locked(self.project) {
            response["index_pending"] = json!(true);
            response["index_error"] = json!(error.to_string());
        }
        Ok(response)
    }
}

pub fn existing_response(doc: &Value, path: &Path, revision: &str) -> Value {
    json!({"object_id":doc["id"],"path":path,"revision":revision,"committed":true,
        "published":true,"durability":"existing","index_pending":false,"unchanged":true})
}

pub fn json_bytes(doc: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(doc)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn validate_document(doc: &Value, expected_kind: Option<&str>) -> Result<()> {
    let map = doc
        .as_object()
        .ok_or_else(|| Error::new("invalid", "Document must be an object"))?;
    if map.get("format").and_then(Value::as_u64) != Some(1) {
        return Err(Error::new(
            "unsupported_format",
            "Only document format 1 is supported; use an explicit migration for older documents",
        ));
    }
    let kind = map
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("invalid", "Document kind is missing"))?;
    if let Some(expected) = expected_kind {
        if kind != expected {
            return Err(Error::new(
                "invalid",
                format!("Expected {expected}, found {kind}"),
            ));
        }
    }
    if !KINDS.contains(&kind) && kind != "memory" {
        return Err(Error::new("invalid", format!("Unknown kind {kind}")));
    }
    valid_id(
        map.get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("invalid", "Document ID is missing"))?,
    )?;
    if map
        .get("project_id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return Err(Error::new("invalid", "Document project_id is missing"));
    }
    Ok(())
}

pub fn merge_patch(target: &mut Value, patch: &Value) {
    if let Some(map) = patch.as_object() {
        if !target.is_object() {
            *target = json!({});
        }
        for (key, value) in map {
            if value.is_null() {
                target.as_object_mut().unwrap().remove(key);
            } else {
                merge_patch(
                    target
                        .as_object_mut()
                        .unwrap()
                        .entry(key.clone())
                        .or_insert(Value::Null),
                    value,
                );
            }
        }
    } else {
        *target = patch.clone();
    }
}

pub fn document_paths(root: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for item in walkdir::WalkDir::new(root).follow_links(false) {
        let item = item.map_err(|e| Error::new("storage", e.to_string()))?;
        if item.file_type().is_file()
            && item.path().extension().and_then(|s| s.to_str()) == Some(extension)
        {
            paths.push(item.into_path());
            if paths.len() > 100_000 {
                return Err(Error::new(
                    "storage",
                    "Metadata snapshot exceeds the initial 100,000-document bound",
                ));
            }
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn has_id(project: &Project, id: &str) -> Result<bool> {
    let store = Store::new(project);
    for kind in KINDS {
        for path in document_paths(&store.directory(kind)?, "json")? {
            let doc: Value = serde_json::from_slice(&fs::read(&path)?)?;
            if doc["id"].as_str() == Some(id) {
                return Ok(true);
            }
        }
    }
    for path in document_paths(&project.memory, "md")? {
        if let Some(doc) = crate::memory::parse_optional(&path)? {
            if doc.metadata["id"].as_str() == Some(id) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Complete only directory synchronization of already-published documents.
pub fn recover_publications(project: &Project) -> Result<Value> {
    let store = Store::new(project);
    let _lock = store.lock()?;
    let mut outcomes = Vec::new();
    for path in document_paths(&project.state_dir.join("receipts"), "json")? {
        let receipt: Value = serde_json::from_slice(&fs::read(&path)?)?;
        if receipt["operation"].as_str() != Some("publication_sync") {
            continue;
        }
        let outcome = (|| -> Result<Value> {
            let destination = PathBuf::from(
                receipt["path"]
                    .as_str()
                    .ok_or_else(|| Error::new("recovery", "Publication receipt has no path"))?,
            );
            let canonical = fs::canonicalize(&destination)?;
            if ![&project.metadata, &project.results, &project.memory]
                .iter()
                .any(|root| canonical.starts_with(root))
            {
                return Err(Error::new(
                    "recovery",
                    "Publication receipt does not name this metadata view",
                ));
            }
            let current = fs::read(&canonical)?;
            if receipt["revision"].as_str() != Some(revision(&current).as_str()) {
                return Err(Error::new(
                    "conflict",
                    "Published document changed since the durability receipt; retain both for manual reconciliation",
                ));
            }
            File::open(&canonical)?.sync_all()?;
            sync_directory(canonical.parent().unwrap())?;
            fs::remove_file(&path)?;
            sync_directory(path.parent().unwrap())?;
            Ok(
                json!({"object_id":receipt["object_id"],"path":canonical,"recovered":true,"durability":"durable"}),
            )
        })();
        match outcome {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => outcomes.push(json!({"recovered":false,"receipt":path,"error":{"kind":error.kind,"message":error.message,"details":error.details}})),
        }
    }
    let failed = outcomes.iter().filter(|v| v["recovered"] != true).count();
    let mut response = json!({"outcomes":outcomes,"failed":failed,"index_pending":false});
    if let Err(error) = crate::index::refresh_locked(project) {
        response["index_pending"] = json!(true);
        response["index_error"] = json!(error.to_string());
    }
    Ok(response)
}
