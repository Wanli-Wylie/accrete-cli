//! Complete-content, dated Markdown publication; filenames are not identities.
use crate::config::Project;
use crate::error::{Error, Result};
use crate::store::{self, Store};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Default, Debug, Clone)]
pub struct AppendOptions {
    pub id: Option<String>,
    pub authored_at: Option<String>,
    pub links: Vec<Value>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryDocument {
    pub metadata: Value,
    pub body: String,
    pub path: PathBuf,
    pub revision: String,
}

pub fn encode(metadata: &Value, body: &str) -> Result<Vec<u8>> {
    // serde_json's arbitrary_precision Numbers use a private serialization
    // token. Passing Value directly to a YAML serializer would write that token
    // as a visible map. Convert ordinary scalars explicitly; use JSON's valid
    // YAML flow syntax when a number cannot round-trip through YAML's Number.
    let yaml = match yaml_value(metadata) {
        Some(value) => serde_yaml::to_string(&value)?,
        None => format!("{}\n", serde_json::to_string_pretty(metadata)?),
    };
    Ok(format!("---\n{yaml}---\n{body}").into_bytes())
}

fn yaml_value(value: &Value) -> Option<serde_yaml::Value> {
    use serde_yaml::Value as Yaml;
    Some(match value {
        Value::Null => Yaml::Null,
        Value::Bool(value) => Yaml::Bool(*value),
        Value::String(value) => Yaml::String(value.clone()),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Yaml::Number(integer.into())
            } else if let Some(integer) = value.as_u64() {
                Yaml::Number(integer.into())
            } else {
                let float = value.as_f64()?;
                if serde_json::Number::from_f64(float).as_ref() != Some(value) {
                    return None;
                }
                Yaml::Number(float.into())
            }
        }
        Value::Array(values) => {
            Yaml::Sequence(values.iter().map(yaml_value).collect::<Option<Vec<_>>>()?)
        }
        Value::Object(values) => {
            let mut mapping = serde_yaml::Mapping::new();
            for (key, value) in values {
                mapping.insert(Yaml::String(key.clone()), yaml_value(value)?);
            }
            Yaml::Mapping(mapping)
        }
    })
}

pub fn parse(path: &Path) -> Result<MemoryDocument> {
    parse_optional(path)?.ok_or_else(|| {
        Error::new(
            "invalid",
            format!("Not an ACCRETE Memory document: {}", path.display()),
        )
    })
}

pub fn parse_optional(path: &Path) -> Result<Option<MemoryDocument>> {
    let bytes = fs::read(path)?;
    decode(&bytes, path)
}

pub fn decode(bytes: &[u8], path: &Path) -> Result<Option<MemoryDocument>> {
    let text = std::str::from_utf8(&bytes).map_err(|e| {
        Error::new(
            "invalid",
            format!("Memory must be UTF-8 at {}: {e}", path.display()),
        )
    })?;
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok(None);
    };
    let Some(end) = rest.find("\n---\n") else {
        if rest.contains("kind: memory") {
            return Err(Error::new(
                "invalid",
                format!("Unclosed Memory frontmatter at {}", path.display()),
            ));
        }
        return Ok(None);
    };
    let frontmatter = &rest[..end];
    let metadata: Value = if frontmatter.trim_start().starts_with('{') {
        serde_json::from_str(frontmatter).map_err(|e| {
            Error::new(
                "invalid",
                format!(
                    "Invalid JSON-form Memory frontmatter at {}: {e}",
                    path.display()
                ),
            )
        })?
    } else {
        serde_yaml::from_str(frontmatter).map_err(|e| {
            Error::new(
                "invalid",
                format!("Invalid Memory frontmatter at {}: {e}", path.display()),
            )
        })?
    };
    if metadata.get("kind").and_then(Value::as_str) != Some("memory") {
        return Ok(None);
    }
    store::validate_document(&metadata, Some("memory"))?;
    Ok(Some(MemoryDocument {
        metadata,
        body: rest[end + 5..].to_string(),
        path: path.to_path_buf(),
        revision: store::revision(bytes),
    }))
}

pub fn find(project: &Project, id: &str) -> Result<Option<MemoryDocument>> {
    let mut found = None;
    for path in store::document_paths(&project.memory, "md")? {
        if let Some(entry) = parse_optional(&path)? {
            if entry.metadata["id"].as_str() == Some(id) {
                if found.is_some() {
                    return Err(Error::new(
                        "conflict",
                        format!("Several Memory documents claim ID {id}"),
                    ));
                }
                found = Some(entry);
            }
        }
    }
    Ok(found)
}

pub fn append(project: &Project, body: &str, options: AppendOptions) -> Result<Value> {
    if body.trim().is_empty() {
        return Err(Error::new(
            "invalid",
            "Memory append requires complete, nonempty authored content",
        ));
    }
    let store = Store::new(project);
    let _lock = store.lock()?;
    let id = options
        .id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    store::valid_id(&id)?;
    let existing = find(project, &id)?;
    let authored_at = options
        .authored_at
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|e| e.metadata["authored_at"].as_str().map(str::to_owned))
        })
        .unwrap_or_else(store::now);
    DateTime::parse_from_rfc3339(&authored_at)
        .map_err(|e| Error::new("invalid", format!("Invalid authored_at: {e}")))?;
    let mut metadata = json!({"format":1,"kind":"memory","id":id,"project_id":project.project_id,
        "authored_at":authored_at,"links":options.links});
    if let Some(title) = options.title {
        metadata["title"] = json!(title);
    }
    if let Some(existing) = existing {
        if existing.metadata == metadata && existing.body == body {
            return store.existing(&metadata, &existing.path, &existing.revision);
        }
        return Err(Error::new(
            "conflict",
            format!("Memory ID {id} already has different content"),
        )
        .detail(json!({"object_id":id,"path":existing.path,"revision":existing.revision})));
    }
    if store::has_id(project, &id)? {
        return Err(Error::new(
            "conflict",
            "Memory ID already belongs to another object",
        ));
    }
    let timezone: chrono_tz::Tz = project
        .timezone
        .parse()
        .map_err(|_| Error::new("config", "Unknown project timezone"))?;
    let day = Utc::now()
        .with_timezone(&timezone)
        .format("%Y/%m/%d")
        .to_string();
    let path = next_path(&project.memory.join(day))?;
    let bytes = encode(&metadata, body)?;
    let publication = store::atomic_write(&path, &bytes, false)?;
    store.finish(&metadata, &path, &bytes, publication)
}

pub fn next_path(directory: &Path) -> Result<PathBuf> {
    store::create_dirs(directory)?;
    let mut largest: u64 = 0;
    for item in fs::read_dir(directory)? {
        let item = item?;
        let path = item.path();
        if path.extension().and_then(|x| x.to_str()) == Some("md") {
            if let Some(n) = path
                .file_stem()
                .and_then(|x| x.to_str())
                .and_then(|x| x.parse::<u64>().ok())
            {
                largest = largest.max(n);
            }
        }
    }
    let number = largest
        .checked_add(1)
        .ok_or_else(|| Error::new("storage", "Memory sequence overflow"))?;
    Ok(directory.join(format!("{number:03}.md")))
}

fn contained_path(project: &Project, preferred: &Path) -> Result<PathBuf> {
    let relative = if preferred.is_absolute() {
        preferred.strip_prefix(&project.memory).map_err(|_| {
            Error::new(
                "invalid",
                "Memory path must be inside configured memory root",
            )
        })?
    } else {
        preferred
    };
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || relative.extension().and_then(|v| v.to_str()) != Some("md")
    {
        return Err(Error::new(
            "invalid",
            "Memory path must be a normal relative Markdown path",
        ));
    }
    let mut path = project.memory.clone();
    for component in relative.components() {
        path.push(component.as_os_str());
        if path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(Error::new(
                "conflict",
                "Memory import path crosses a symbolic link",
            ));
        }
    }
    Ok(path)
}

pub fn import_entry(
    project: &Project,
    mut metadata: Value,
    body: &str,
    preferred_path: Option<&Path>,
    expect: Option<&str>,
) -> Result<Value> {
    store::validate_document(&metadata, Some("memory"))?;
    let id = metadata["id"].as_str().unwrap().to_owned();
    let store = Store::new(project);
    let _lock = store.lock()?;
    if let Some(existing) = find(project, &id)? {
        // origin_path records an import collision, not a content change.
        let mut existing_semantic = existing.metadata.clone();
        if metadata.get("origin_path").is_none() {
            existing_semantic
                .as_object_mut()
                .unwrap()
                .remove("origin_path");
        }
        if existing_semantic == metadata && existing.body == body {
            return store.existing(&existing.metadata, &existing.path, &existing.revision);
        }
        if expect != Some(existing.revision.as_str()) {
            return Err(
                Error::new("conflict", "Incoming Memory diverges from an existing ID").detail(
                    json!({"object_id":id,"path":existing.path,"revision":existing.revision}),
                ),
            );
        }
        let bytes = encode(&metadata, body)?;
        let publication = store::atomic_write(&existing.path, &bytes, true)?;
        return store.finish(&metadata, &existing.path, &bytes, publication);
    }
    if expect.is_some() {
        return Err(Error::new(
            "conflict",
            "Expected Memory predecessor is absent",
        ));
    }
    if store::has_id(project, &id)? {
        return Err(Error::new(
            "conflict",
            "Memory ID belongs to another object family",
        ));
    }
    let requested = if let Some(path) = preferred_path {
        contained_path(project, path)?
    } else {
        let timezone: chrono_tz::Tz = project
            .timezone
            .parse()
            .map_err(|_| Error::new("config", "Unknown timezone"))?;
        let date = metadata["authored_at"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&timezone).format("%Y/%m/%d").to_string())
            .unwrap_or_else(|| "undated".to_owned());
        next_path(&project.memory.join(date))?
    };
    let path = if requested.exists() {
        if metadata.get("origin_path").is_none() {
            metadata["origin_path"] = json!(
                requested
                    .strip_prefix(&project.memory)
                    .unwrap_or(&requested)
            );
        }
        next_path(requested.parent().unwrap())?
    } else {
        requested
    };
    let bytes = encode(&metadata, body)?;
    let publication = store::atomic_write(&path, &bytes, false)?;
    store.finish(&metadata, &path, &bytes, publication)
}

/// Conversion-only replacement of an exact legacy source file.
pub fn publish_migration(
    project: &Project,
    metadata: Value,
    body: &str,
    path: &Path,
    expected_source_revision: &str,
) -> Result<Value> {
    store::validate_document(&metadata, Some("memory"))?;
    let store = Store::new(project);
    let _lock = store.lock()?;
    let path = contained_path(project, path)?;
    let bytes = encode(&metadata, body)?;
    let old = fs::read(&path)?;
    if old == bytes {
        return store.existing(&metadata, &path, &store::revision(&old));
    }
    if store::revision(&old) != expected_source_revision {
        return Err(
            Error::new("conflict", "Legacy Memory changed after migration planning")
                .detail(json!({"path":path})),
        );
    }
    if let Some(existing) = find(project, metadata["id"].as_str().unwrap())? {
        if existing.path != path {
            return Err(Error::new(
                "conflict",
                "Migration ID already exists at another Memory path",
            ));
        }
    } else if store::has_id(project, metadata["id"].as_str().unwrap())? {
        return Err(Error::new(
            "conflict",
            "Migration ID belongs to another object family",
        ));
    }
    let publication = store::atomic_write(&path, &bytes, true)?;
    store.finish(&metadata, &path, &bytes, publication)
}
