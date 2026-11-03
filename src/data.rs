//! Data identity, explicit replicas, and bounded transfer recovery.
//!
//! Registration never takes ownership of the source. Only replicas created in
//! a private destination by `materialize` are eligible for `evict`.
use crate::config::Project;
use crate::error::{Error, Result};
use crate::store::{self, Store};
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalid", message)
}
fn conflict(message: impl Into<String>) -> Error {
    Error::new("conflict", message)
}
fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("missing string field {key}")))
}
fn check_expected(actual: &str, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if expected != actual {
            return Err(conflict("document revision changed"));
        }
    }
    Ok(())
}

/// Explicit SHA-256 hashing is limited to a single regular file. A producer can
/// supply a collection manifest instead of asking registration to traverse it.
pub fn sha256_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid(
            "SHA-256 requires a regular file; supply a manifest for a directory",
        ));
    }
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = vec![0u8; 1024 * 1024];
    loop {
        let count = input.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn check_digest(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(
            "content.sha256 must be a 64-character SHA-256 hex digest",
        ));
    }
    Ok(())
}

fn deterministic_location_id(location: &Value) -> String {
    let basis = json!({"name": location.get("name"), "kind": location.get("kind"),
        "path":location.get("path"), "host":location.get("host"),
        "version":location.get("version"), "s3_version_at":location.get("s3_version_at")});
    format!(
        "location-{}",
        &store::revision(basis.to_string().as_bytes())[..24]
    )
}

fn location_config<'a>(project: &'a Project, name: &str) -> Result<&'a toml::Value> {
    project
        .config
        .get("locations")
        .and_then(|v| v.get(name))
        .ok_or_else(|| Error::new("config", format!("location {name:?} is not configured")))
}

fn same_location(a: &Value, b: &Value) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    for value in [&mut a, &mut b] {
        if let Some(map) = value.as_object_mut() {
            map.remove("observed_at");
        }
    }
    a == b
}

fn normalize_location(project: &Project, mut location: Value) -> Result<Value> {
    let map = location
        .as_object_mut()
        .ok_or_else(|| invalid("a location must be a JSON object"))?;
    if map.get("managed").and_then(Value::as_bool) == Some(true) {
        return Err(invalid(
            "registration cannot claim managed ownership; use data materialize",
        ));
    }
    let name = map.get("name").and_then(Value::as_str).map(str::to_string);
    let configured = name
        .as_deref()
        .and_then(|name| project.config.get("locations").and_then(|v| v.get(name)));
    let kind = map
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| {
            configured
                .and_then(|v| v.get("kind"))
                .and_then(toml::Value::as_str)
        })
        .unwrap_or("filesystem")
        .to_string();
    if !["filesystem", "rclone", "external"].contains(&kind.as_str()) {
        return Err(invalid(format!("unsupported location kind {kind:?}")));
    }
    map.insert("kind".into(), json!(kind));
    map.insert("managed".into(), json!(false));
    if kind == "filesystem" {
        map.entry("host").or_insert_with(|| json!(project.host));
        let path = map
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("filesystem location requires path"))?;
        if Path::new(path).is_relative() {
            let resolved = if let Some(root) = configured
                .and_then(|v| v.get("root"))
                .and_then(toml::Value::as_str)
            {
                let root = project.resolve(root);
                safe_join(&root, path)?
            } else {
                project.resolve(path)
            };
            map.insert("path".into(), json!(resolved));
        }
    } else if !map.contains_key("path") {
        return Err(invalid("external/rclone location requires path"));
    }
    map.entry("availability")
        .or_insert_with(|| json!("observed"));
    map.entry("observed_at")
        .or_insert_with(|| json!(store::now()));
    if !map.contains_key("id") {
        let id = deterministic_location_id(&location);
        location["id"] = json!(id);
    }
    Ok(location)
}

pub fn register(
    project: &Project,
    mut payload: Value,
    path: Option<&Path>,
    content_kind: Option<&str>,
    hash: bool,
) -> Result<Value> {
    if !payload.is_object() {
        return Err(invalid("Data input must be a JSON object"));
    }
    if payload.get("content").is_none() {
        payload["content"] = json!({});
    }
    if !payload["content"].is_object() {
        return Err(invalid("content must be a JSON object"));
    }
    if let Some(kind) = content_kind {
        payload["content"]["kind"] = json!(kind);
    }
    let existing = payload
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Store::new(project).read("data", id).ok());
    if let Some(path) = path {
        let path = project.resolve(path);
        let metadata = fs::symlink_metadata(&path)?;
        let kind = if metadata.is_file() {
            "file"
        } else if metadata.is_dir() {
            "directory"
        } else {
            return Err(invalid(
                "register a regular file or directory; describe symlinks as an external locator",
            ));
        };
        if let Some(wanted) = payload["content"].get("kind").and_then(Value::as_str) {
            if wanted != kind && !(wanted == "collection" && kind == "directory") {
                return Err(invalid(format!(
                    "content kind {wanted:?} does not match local {kind}"
                )));
            }
        } else {
            payload["content"]["kind"] = json!(kind);
        }
        if metadata.is_file() {
            payload["content"]["size_bytes"] = json!(metadata.len());
        }
        if hash {
            payload["content"]["sha256"] = json!(sha256_file(&path)?);
        }
        let mut location = normalize_location(project, json!({"name":"registered", "path":path}))?;
        if let Some(old) = existing
            .as_ref()
            .and_then(|old| old.doc.get("locations"))
            .and_then(Value::as_array)
            .and_then(|locations| {
                locations
                    .iter()
                    .find(|old| old.get("id") == location.get("id"))
            })
        {
            if same_location(old, &location) {
                location = old.clone();
            }
        }
        payload
            .as_object_mut()
            .unwrap()
            .entry("locations")
            .or_insert_with(|| json!([]));
        let locations = payload["locations"]
            .as_array_mut()
            .ok_or_else(|| invalid("locations must be an array"))?;
        if !locations
            .iter()
            .any(|old| old.get("id") == location.get("id"))
        {
            locations.push(location);
        }
    } else if hash {
        return Err(invalid("--hash requires --path to a regular file"));
    }
    if payload["content"].get("kind").is_none() {
        payload["content"]["kind"] = json!("external");
    }
    if payload["content"].get("mode").is_none() {
        payload["content"]["mode"] = json!("closed");
    }
    let kind = str_field(&payload["content"], "kind")?;
    if !["file", "directory", "collection", "external"].contains(&kind) {
        return Err(invalid(
            "content.kind must be file, directory, collection, or external",
        ));
    }
    let mode = str_field(&payload["content"], "mode")?;
    if !["closed", "live"].contains(&mode) {
        return Err(invalid("content.mode must be closed or live"));
    }
    if let Some(digest) = payload["content"].get("sha256").and_then(Value::as_str) {
        check_digest(digest)?;
    }
    if let Some(locations) = payload.get_mut("locations") {
        let locations = locations
            .as_array_mut()
            .ok_or_else(|| invalid("locations must be an array"))?;
        for location in locations.iter_mut() {
            let mut normalized = normalize_location(project, location.clone())?;
            if let Some(old) = existing
                .as_ref()
                .and_then(|old| old.doc.get("locations"))
                .and_then(Value::as_array)
                .and_then(|old_locations| {
                    old_locations
                        .iter()
                        .find(|old| old.get("id") == normalized.get("id"))
                })
            {
                if same_location(old, &normalized) {
                    normalized = old.clone();
                }
            }
            *location = normalized;
        }
    }
    Store::new(project).create("data", payload)
}

pub fn location_add(
    project: &Project,
    selector: &str,
    location: Value,
    expected: Option<&str>,
) -> Result<Value> {
    let location = normalize_location(project, location)?;
    Store::new(project).update("data", selector, expected, |doc| {
        doc.as_object_mut()
            .unwrap()
            .entry("locations")
            .or_insert_with(|| json!([]));
        let locations = doc["locations"]
            .as_array_mut()
            .ok_or_else(|| invalid("locations must be an array"))?;
        if let Some(old) = locations
            .iter()
            .find(|old| old.get("id") == location.get("id"))
        {
            if !same_location(old, &location) {
                return Err(conflict(
                    "location ID already describes a different location",
                ));
            }
        } else {
            locations.push(location);
        }
        Ok(())
    })
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid(
            "a configured location's relative path cannot escape its root",
        ));
    }
    Ok(root.join(relative))
}

fn operations_dir(project: &Project) -> PathBuf {
    project.state_dir.join("operations")
}
fn receipt_path(project: &Project, id: &str) -> Result<PathBuf> {
    if Uuid::parse_str(id).is_err() {
        return Err(invalid("operation ID must be a UUID"));
    }
    Ok(operations_dir(project).join(format!("data-{id}.json")))
}
fn save_receipt(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let publication = store::atomic_write(path, &bytes, path.exists())?;
    if publication.durability == "uncertain" {
        return Err(Error::new(
            "durability",
            "operation receipt is visible but directory synchronization failed",
        )
        .detail(json!({"published":true,"path":path,"durability":"uncertain"})));
    }
    Ok(())
}
fn external_error(message: impl Into<String>, path: &Path, receipt: &Value) -> Error {
    Error::new("incomplete", message).detail(json!({"operation_id":receipt["operation_id"],
        "receipt":path,"staging":receipt["staging"],"destination":receipt["destination"],
        "phase":receipt["phase"],"recovery":"accrete recover"}))
}

fn cleanup_receipt(path: &Path, outcome: &mut Value) {
    if outcome.get("durability").and_then(Value::as_str) == Some("uncertain") {
        outcome["recovery_pending"] = json!(true);
        outcome["receipt"] = json!(path);
    } else if let Err(error) = fs::remove_file(path) {
        // The descriptor has already committed. A failed housekeeping unlink
        // must never turn that completion into a failed semantic operation.
        outcome["recovery_pending"] = json!(true);
        outcome["receipt"] = json!(path);
        outcome["recovery_warning"] = json!(error.to_string());
    }
}

fn operation_lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().unwrap())?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| conflict(format!("operation is active: {}", path.display())))?;
    Ok(lock)
}

fn local_source(project: &Project, location: &Value) -> Result<PathBuf> {
    if let Some(host) = location.get("host").and_then(Value::as_str) {
        if host != project.host {
            return Err(Error::new(
                "config",
                format!(
                    "source belongs to host {host:?}, current host is {:?}",
                    project.host
                ),
            ));
        }
    }
    Ok(project.resolve(str_field(location, "path")?))
}

fn source_location(project: &Project, doc: &Value, requested: Option<&str>) -> Result<Value> {
    let locations = doc
        .get("locations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Data has no locations"))?;
    let available =
        |location: &&Value| location.get("availability").and_then(Value::as_str) != Some("evicted");
    if let Some(requested) = requested {
        let mut matches = locations
            .iter()
            .filter(|location| {
                location.get("id").and_then(Value::as_str) == Some(requested)
                    || location.get("name").and_then(Value::as_str) == Some(requested)
            })
            .filter(available);
        let chosen = matches.next().ok_or_else(|| {
            invalid(format!(
                "source location {requested:?} is absent or evicted"
            ))
        })?;
        if matches.next().is_some() {
            return Err(invalid("source name is ambiguous; select the location ID"));
        }
        return Ok(chosen.clone());
    }
    for location in locations.iter().filter(available) {
        if location.get("kind").and_then(Value::as_str) == Some("filesystem") {
            if let Ok(path) = local_source(project, location) {
                if path.exists() {
                    return Ok(location.clone());
                }
            }
        }
    }
    let candidates: Vec<_> = locations
        .iter()
        .filter(available)
        .filter(|location| location.get("kind").and_then(Value::as_str) == Some("rclone"))
        .collect();
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    Err(Error::new(
        "config",
        "no unambiguous usable source; supply --source LOCATION_ID",
    ))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_file() {
        let mut src = File::open(source)?;
        let mut dst = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        std::io::copy(&mut src, &mut dst)?;
        dst.flush()?;
        fs::set_permissions(destination, metadata.permissions())?;
        dst.sync_all()?;
    } else if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        sync_dir(destination)?;
    } else {
        return Err(invalid(format!(
            "materialization requires regular files/directories; link or special file at {} must be packaged explicitly by a domain tool",
            source.display()
        )));
    }
    Ok(())
}

fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}

fn sync_payload(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        File::open(path)?.sync_all()?;
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_payload(&entry?.path())?;
        }
        sync_dir(path)?;
    } else {
        return Err(invalid(
            "transferred payload contains a symlink or special file; package it explicitly before materializing",
        ));
    }
    Ok(())
}

fn run_rclone(project: &Project, source: &Value, staging: &Path, directory: bool) -> Result<()> {
    // Retain opaque backend tokens, but do not silently fetch the current object
    // when a caller requested an unsupported historical version.
    if source.get("version").is_some()
        || source.get("version_id").is_some()
        || source.get("s3_version_at").is_some()
    {
        return Err(invalid(
            "rclone version selectors are not supported in format 1; use a version-specific exported locator or an external transfer, then data location add",
        ));
    }
    let name = str_field(source, "name")?;
    let configured = location_config(project, name)?;
    let remote = configured
        .get("remote")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            Error::new(
                "config",
                format!("rclone location {name:?} requires remote"),
            )
        })?;
    if remote.is_empty() || remote.contains([':', '/', '\\']) {
        return Err(invalid(
            "rclone remote must be a configured remote name, without a path",
        ));
    }
    let prefix = configured
        .get("prefix")
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .trim_matches('/');
    let path = str_field(source, "path")?;
    if path.starts_with('/') || path.split('/').any(|p| p == "..") {
        return Err(invalid(
            "rclone source path must be relative to its configured prefix",
        ));
    }
    let operand = if prefix.is_empty() {
        format!("{remote}:{path}")
    } else {
        format!("{remote}:{prefix}/{path}")
    };
    let executable = project
        .config
        .get("native")
        .and_then(|v| v.get("rclone"))
        .and_then(toml::Value::as_str)
        .unwrap_or("rclone");
    // rclone diagnostics/progress go to stderr; structured stdout stays clean.
    let status = Command::new(executable)
        .arg(if directory { "copy" } else { "copyto" })
        .arg("--immutable")
        .arg("--")
        .arg(operand)
        .arg(staging)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    "dependency",
                    format!("rclone executable unavailable: {executable}"),
                )
            } else {
                error.into()
            }
        })?;
    if !status.success() {
        return Err(Error::new(
            "dependency",
            format!("rclone transfer failed with {status}"),
        ));
    }
    Ok(())
}

fn compare_content(receipt: &Value, doc: &Value) -> Result<()> {
    if receipt.get("content") != doc.get("content") {
        return Err(conflict(
            "Data content description changed during materialization; retained replica awaits explicit reconciliation",
        ));
    }
    Ok(())
}

fn finish_materialization(
    project: &Project,
    receipt_path: &Path,
    receipt: &mut Value,
) -> Result<Value> {
    let id = str_field(receipt, "data_id")?.to_string();
    let destination = PathBuf::from(str_field(receipt, "destination")?);
    let staging = PathBuf::from(str_field(receipt, "staging")?);
    let container = PathBuf::from(str_field(receipt, "container")?);
    let phase = str_field(receipt, "phase")?.to_string();
    if phase == "copied" {
        if !destination.exists() {
            if !staging.exists() {
                return Err(external_error(
                    "neither completed staging nor destination exists",
                    receipt_path,
                    receipt,
                ));
            }
            // This UUID container was exclusively reserved for this operation.
            // No other Kernel operation writes its payload path.
            fs::rename(&staging, &destination)?;
        }
        sync_dir(&container)?;
        receipt["phase"] = json!("published");
        save_receipt(receipt_path, receipt)?;
    } else if phase != "published" {
        return Err(external_error(
            "transfer did not finish; inspect the staging path and retry materialization explicitly",
            receipt_path,
            receipt,
        ));
    }
    if !destination.exists() {
        return Err(external_error(
            "published replica is missing; recovery will not repeat the transfer",
            receipt_path,
            receipt,
        ));
    }
    let mut location = receipt["location"].clone();
    location["availability"] = json!("present");
    let outcome = Store::new(project)
        .update("data", &id, None, |doc| {
            compare_content(receipt, doc)?;
            doc.as_object_mut()
                .unwrap()
                .entry("locations")
                .or_insert_with(|| json!([]));
            let locations = doc["locations"]
                .as_array_mut()
                .ok_or_else(|| invalid("locations must be an array"))?;
            if let Some(existing) = locations
                .iter()
                .find(|old| old.get("id") == location.get("id"))
            {
                if !same_location(existing, &location) {
                    return Err(conflict("managed location ID has diverged"));
                }
            } else {
                locations.push(location.clone());
            }
            Ok(())
        })
        .map_err(|error| {
            external_error(
                format!("replica exists but descriptor publication needs recovery: {error}"),
                receipt_path,
                receipt,
            )
        })?;
    let mut outcome = outcome;
    cleanup_receipt(receipt_path, &mut outcome);
    outcome["location"] = location;
    outcome["content_checks"] = receipt["content_checks"].clone();
    Ok(outcome)
}

pub fn materialize(
    project: &Project,
    selector: &str,
    to: &str,
    source: Option<&str>,
    expected: Option<&str>,
) -> Result<Value> {
    let store = Store::new(project);
    let object = store.read("data", selector)?;
    check_expected(&object.revision, expected)?;
    let id = str_field(&object.doc, "id")?.to_string();
    let configured = location_config(project, to)?;
    if configured
        .get("kind")
        .and_then(toml::Value::as_str)
        .unwrap_or("filesystem")
        != "filesystem"
    {
        return Err(invalid(
            "materialization destinations must be configured filesystem locations",
        ));
    }
    let root = project.resolve(
        configured
            .get("root")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                Error::new(
                    "config",
                    format!("destination {to:?} requires a local root"),
                )
            })?,
    );
    let root = crate::config::canonical_future(&root)?;
    if let Some(locations) = object.doc.get("locations").and_then(Value::as_array) {
        for location in locations {
            if location.get("name").and_then(Value::as_str) == Some(to)
                && location.get("host").and_then(Value::as_str) == Some(&project.host)
                && location.get("managed").and_then(Value::as_bool) == Some(true)
                && location.get("availability").and_then(Value::as_str) == Some("present")
            {
                if let Ok(path) = local_source(project, location) {
                    if path.exists() && path.starts_with(&root) {
                        return Ok(
                            json!({"object_id":id,"path":object.path,"revision":object.revision,
                            "committed":true,"unchanged":true,"index_pending":false,"location":location}),
                        );
                    }
                }
            }
        }
    }
    let source = source_location(project, &object.doc, source)?;
    if source.get("kind").and_then(Value::as_str) == Some("filesystem") {
        let source_path = local_source(project, &source)?;
        if source_path.is_dir() && root.starts_with(fs::canonicalize(&source_path)?) {
            return Err(invalid(
                "materialization destination lies inside the source directory; configure a separate location root",
            ));
        }
    }
    store::create_dirs(&root)?;
    let content = object
        .doc
        .get("content")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let content_kind = content
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("external");
    if source.get("kind").and_then(Value::as_str) == Some("rclone")
        && ["version", "version_id", "s3_version_at"]
            .iter()
            .any(|key| content.get(key).is_some())
    {
        return Err(invalid(
            "rclone content version selectors are unsupported; materialize the selected version with its native tool and add its local location",
        ));
    }
    if let Some(digest) = content.get("sha256").and_then(Value::as_str) {
        check_digest(digest)?;
    }
    let directory = matches!(content_kind, "directory" | "collection");
    if !directory && !matches!(content_kind, "file" | "external") {
        return Err(invalid("unsupported content kind for materialization"));
    }
    let operation = Uuid::new_v4().to_string();
    let receipt_file = receipt_path(project, &operation)?;
    let _operation_lock = operation_lock(&receipt_file)?;
    let container_parent = safe_join(&root, &id)?;
    store::create_dirs(&container_parent)?;
    let container = container_parent.join(&operation);
    fs::create_dir(&container)?;
    sync_dir(&container_parent)?;
    let staging = container.join(".staging");
    let destination = container.join("payload");
    let ownership = json!({"format":1,"project_id":project.project_id,"data_id":id,"location_id":operation,
        "host":project.host,"path":destination,"root":root});
    let ownership_path = container.join("ownership.json");
    let ownership_bytes = serde_json::to_vec_pretty(&ownership)?;
    let publication = store::atomic_write(&ownership_path, &ownership_bytes, false)?;
    if publication.durability == "uncertain" {
        return Err(Error::new(
            "durability",
            "replica ownership marker has uncertain durability",
        )
        .detail(json!({"published":true,"path":ownership_path})));
    }
    let location = json!({"id":operation,"name":to,"kind":"filesystem","path":destination,
        "host":project.host,"managed":true,"availability":"present","observed_at":store::now(),
        "ownership":ownership_path,"ownership_revision":store::revision(&ownership_bytes)});
    let mut receipt = json!({"format":1,"command":"data.materialize","operation_id":operation,
        "project_id":project.project_id,"view_key":project.view_key,"data_id":id,"old_revision":object.revision,
        "content":content,"source":source,"location":location,"container":container,"staging":staging,
        "destination":destination,"phase":"prepared","content_checks":[]});
    save_receipt(&receipt_file, &receipt)?;
    let transferred = (|| -> Result<()> {
        match source.get("kind").and_then(Value::as_str) {
            Some("filesystem") => {
                let source_path = local_source(project, &source)?;
                let metadata = fs::symlink_metadata(&source_path)?;
                if metadata.is_dir() != directory && content_kind != "external" {
                    return Err(external_error(
                        "source filesystem type differs from content description",
                        &receipt_file,
                        &receipt,
                    ));
                }
                if destination.starts_with(fs::canonicalize(&source_path)?) {
                    return Err(external_error(
                        "destination lies inside the source directory",
                        &receipt_file,
                        &receipt,
                    ));
                }
                copy_tree(&source_path, &staging)
            }
            Some("rclone") => run_rclone(project, &source, &staging, directory),
            _ => Err(invalid(
                "source is an external reference; transfer it with its native tool and add the local location",
            )),
        }
    })();
    if let Err(error) = transferred {
        return Err(external_error(
            format!("transfer incomplete: {error}"),
            &receipt_file,
            &receipt,
        ));
    }
    sync_payload(&staging)
        .map_err(|error| external_error(error.to_string(), &receipt_file, &receipt))?;
    if let Some(digest) = content.get("sha256").and_then(Value::as_str) {
        let actual = sha256_file(&staging)
            .map_err(|error| external_error(error.to_string(), &receipt_file, &receipt))?;
        if !actual.eq_ignore_ascii_case(digest) {
            return Err(external_error(
                format!("SHA-256 mismatch: expected {digest}, received {actual}"),
                &receipt_file,
                &receipt,
            ));
        }
        receipt["content_checks"] =
            json!([{"algorithm":"sha256","digest":actual,"scope":"file bytes"}]);
    }
    if let Some(size) = content.get("size_bytes").and_then(Value::as_u64) {
        if staging.is_file()
            && fs::metadata(&staging)
                .map_err(|error| external_error(error.to_string(), &receipt_file, &receipt))?
                .len()
                != size
        {
            return Err(external_error(
                "file size differs from content.size_bytes",
                &receipt_file,
                &receipt,
            ));
        }
    }
    receipt["phase"] = json!("copied");
    save_receipt(&receipt_file, &receipt)?;
    finish_materialization(project, &receipt_file, &mut receipt)
        .map_err(|error| external_error(error.to_string(), &receipt_file, &receipt))
}

fn owned_location(project: &Project, doc: &Value, selector: &str) -> Result<Value> {
    let locations = doc
        .get("locations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Data has no locations"))?;
    let current = |location: &&Value| {
        location.get("host").and_then(Value::as_str) == Some(&project.host)
            && location.get("availability").and_then(Value::as_str) != Some("evicted")
    };
    let by_id = locations
        .iter()
        .filter(current)
        .find(|location| location.get("id").and_then(Value::as_str) == Some(selector));
    let named: Vec<_> = locations
        .iter()
        .filter(current)
        .filter(|location| location.get("name").and_then(Value::as_str) == Some(selector))
        .collect();
    let location = match (by_id, named.as_slice()) {
        (Some(location), _) => location,
        (None, [location]) => *location,
        (None, []) => {
            return Err(invalid(format!(
                "no current local location with ID or name {selector:?}"
            )));
        }
        (None, _) => {
            return Err(conflict(format!(
                "location name {selector:?} is ambiguous; select its ID"
            )));
        }
    };
    if location.get("managed").and_then(Value::as_bool) != Some(true) {
        return Err(invalid(
            "borrowed locations cannot be evicted; registration does not transfer deletion ownership",
        ));
    }
    let path = local_source(project, location)?;
    let ownership_path = PathBuf::from(str_field(location, "ownership")?);
    if ownership_path.file_name().and_then(|s| s.to_str()) != Some("ownership.json")
        || ownership_path.parent() != path.parent()
        || path.file_name().and_then(|s| s.to_str()) != Some("payload")
    {
        return Err(conflict("managed replica ownership paths disagree"));
    }
    let bytes = fs::read(&ownership_path)?;
    if store::revision(&bytes) != str_field(location, "ownership_revision")? {
        return Err(conflict("managed replica ownership marker changed"));
    }
    let ownership: Value = serde_json::from_slice(&bytes)?;
    if ownership.get("project_id").and_then(Value::as_str) != Some(&project.project_id)
        || ownership.get("data_id") != doc.get("id")
        || ownership.get("location_id") != location.get("id")
        || ownership.get("host").and_then(Value::as_str) != Some(&project.host)
        || ownership.get("path").and_then(Value::as_str) != path.to_str()
    {
        return Err(conflict(
            "managed replica ownership marker does not match this Data and host",
        ));
    }
    let root = PathBuf::from(str_field(&ownership, "root")?);
    let parent = path.parent().unwrap();
    if fs::canonicalize(parent)? != parent || !parent.starts_with(fs::canonicalize(root)?) {
        return Err(conflict("managed replica path has been redirected"));
    }
    Ok(location.clone())
}

fn finish_eviction(project: &Project, receipt_path: &Path, receipt: &mut Value) -> Result<Value> {
    let id = str_field(receipt, "data_id")?.to_string();
    let location_id = str_field(&receipt["location"], "id")?.to_string();
    let path = PathBuf::from(str_field(receipt, "destination")?);
    let current = Store::new(project).read("data", &id)?;
    let location = owned_location(project, &current.doc, &location_id);
    if let Ok(location) = location {
        if location != receipt["location"] {
            return Err(conflict("location changed after eviction was requested"));
        }
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(conflict("managed payload was replaced by a symlink"));
            }
            if metadata.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
            sync_dir(path.parent().unwrap())?;
        }
    } else {
        let already = current
            .doc
            .get("locations")
            .and_then(Value::as_array)
            .map(|locations| {
                locations.iter().any(|location| {
                    location.get("id").and_then(Value::as_str) == Some(&location_id)
                        && location.get("availability").and_then(Value::as_str) == Some("evicted")
                })
            })
            .unwrap_or(false);
        if !already {
            return Err(location.unwrap_err());
        }
    }
    receipt["phase"] = json!("removed");
    save_receipt(receipt_path, receipt)?;
    let when = receipt
        .get("requested_at")
        .cloned()
        .unwrap_or_else(|| json!(store::now()));
    let mut result = Store::new(project)
        .update("data", &id, None, |doc| {
            let locations = doc
                .get_mut("locations")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| invalid("locations must be an array"))?;
            let location = locations
                .iter_mut()
                .find(|location| location.get("id").and_then(Value::as_str) == Some(&location_id))
                .ok_or_else(|| conflict("evicted location disappeared from descriptor"))?;
            location["availability"] = json!("evicted");
            location["observed_at"] = when;
            Ok(())
        })
        .map_err(|error| {
            external_error(
                format!("replica was removed; descriptor needs recovery: {error}"),
                receipt_path,
                receipt,
            )
        })?;
    cleanup_receipt(receipt_path, &mut result);
    result["evicted_location"] = json!(location_id);
    result["removed_path"] = json!(path);
    Ok(result)
}

pub fn evict(
    project: &Project,
    selector: &str,
    location: &str,
    expected: Option<&str>,
) -> Result<Value> {
    let object = Store::new(project).read("data", selector)?;
    check_expected(&object.revision, expected)?;
    let location = owned_location(project, &object.doc, location)?;
    let operation = Uuid::new_v4().to_string();
    let path = receipt_path(project, &operation)?;
    let _lock = operation_lock(&path)?;
    let mut receipt = json!({"format":1,"command":"data.evict","operation_id":operation,"project_id":project.project_id,
        "view_key":project.view_key,"data_id":object.doc["id"],"old_revision":object.revision,"location":location,
        "destination":location["path"],"phase":"prepared","requested_at":store::now()});
    save_receipt(&path, &receipt)?;
    finish_eviction(project, &path, &mut receipt)
        .map_err(|error| external_error(error.to_string(), &path, &receipt))
}

/// Finish only effects whose receipt proves transfer completion. Recovery never
/// launches scientific work and never starts a new copy of a partial transfer.
pub fn recover(project: &Project, operation: Option<&str>) -> Result<Value> {
    let paths = if let Some(operation) = operation {
        vec![receipt_path(project, operation)?]
    } else if operations_dir(project).exists() {
        fs::read_dir(operations_dir(project))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("data-") && name.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        vec![]
    };
    let mut outcomes = Vec::new();
    for path in paths {
        let outcome = (|| -> Result<Value> {
            let _lock = operation_lock(&path)?;
            let mut receipt: Value = serde_json::from_slice(&fs::read(&path)?)?;
            if receipt.get("project_id").and_then(Value::as_str) != Some(&project.project_id)
                || receipt.get("view_key").and_then(Value::as_str) != Some(&project.view_key)
            {
                return Err(conflict("receipt belongs to a different metadata view"));
            }
            match receipt.get("command").and_then(Value::as_str) {
                Some("data.materialize") => finish_materialization(project, &path, &mut receipt),
                Some("data.evict") => finish_eviction(project, &path, &mut receipt),
                _ => Err(invalid("unsupported data operation receipt")),
            }
        })();
        match outcome {
            Ok(value) => outcomes.push(json!({"receipt":path,"recovered":true,"outcome":value})),
            Err(error) => {
                outcomes.push(json!({"receipt":path,"recovered":false,"error":error.to_string()}))
            }
        }
    }
    Ok(
        json!({"operations":outcomes,"pending":outcomes.iter().filter(|item| item["recovered"] == false).count()}),
    )
}
