//! Explicit, restartable conversion of the supplied ACCRETE Python scaffold.
//! Only small metadata files are read. Payloads, model histories and tools are
//! retained at their existing paths, and exact original metadata is backed up.
use crate::config::Project;
use crate::error::{Error, Result};
use crate::{
    index, memory,
    store::{self, Store},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalid", message)
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("migration plan missing {key}")))
}
fn write_json(path: &Path, value: &Value, replace: bool) -> Result<()> {
    let publication = store::atomic_write(path, &store::json_bytes(value)?, replace)?;
    if publication.durability == "uncertain" {
        return Err(Error::new(
            "durability",
            "migration metadata is visible but directory synchronization failed",
        )
        .detail(json!({"published":true,"path":path,"durability":"uncertain"})));
    }
    Ok(())
}
fn safe_relative(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid(format!(
            "legacy path is not a normal relative path: {}",
            path.display()
        )));
    }
    Ok(())
}

fn envelop(project: &Project, mut value: Value, kind: &str, id: &str, when: &str) -> Result<Value> {
    let map = value
        .as_object_mut()
        .ok_or_else(|| invalid("legacy JSON must be an object"))?;
    for (key, expected) in [
        ("format", json!(1)),
        ("kind", json!(kind)),
        ("project_id", json!(project.project_id)),
        ("id", json!(id)),
    ] {
        if let Some(existing) = map.get(key) {
            if existing != &expected {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "legacy field {key:?} conflicts with the format-1 envelope; provide an explicit source edit/mapping"
                    ),
                ));
            }
        }
        map.insert(key.into(), expected);
    }
    if kind != "memory" {
        map.entry("created_at").or_insert_with(|| json!(when));
    }
    store::validate_document(&value, Some(kind))?;
    Ok(value)
}

fn source_entry(path: &Path, backup_root: &Path) -> Result<Value> {
    let bytes = fs::read(path)?;
    Ok(json!({"path":path,"revision":store::revision(&bytes),
        "backup":backup_root.join(format!("{}.original",store::revision(path.to_string_lossy().as_bytes())))}))
}

fn push_action(
    plan: &mut Value,
    source: &Path,
    destination: &Path,
    kind: &str,
    id: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let text = String::from_utf8(bytes.clone()).map_err(|e| invalid(e.to_string()))?;
    plan["actions"].as_array_mut().unwrap().push(
        json!({"source":source,"destination":destination,"kind":kind,"id":id,
        "revision":store::revision(&bytes),"content":text,"complete":false}),
    );
    Ok(())
}

fn legacy_frontmatter(text: &str) -> Result<(Value, String)> {
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            let metadata: Value = serde_yaml::from_str(&rest[..end])?;
            if !metadata.is_object() {
                return Err(invalid("legacy Memory frontmatter must be a mapping"));
            }
            return Ok((metadata, rest[end + 5..].to_string()));
        }
    }
    Ok((json!({}), text.to_string()))
}

fn plan_migration(project: &Project, source: &Path, state: &Path) -> Result<Value> {
    let migration_id = Uuid::new_v4().to_string();
    let when = store::now();
    let backup = state.join("backup");
    let mut plan = json!({"format":1,"migration_id":migration_id,"project_id":project.project_id,
        "legacy_root":source,"planned_at":when,"status":"planned","sources":[],"actions":[],
        "legacy_reservations":[],"call_sites":[],"bindings":[],"identity_map":{}});
    let store = Store::new(project);
    let mut sources = BTreeSet::new();
    let mut data_ids = BTreeMap::new();
    let mut model_ids = BTreeMap::new();
    let mut tool_ids = BTreeMap::new();
    let index_path = source.join("data/index.tsv");
    if index_path.exists() {
        let text = fs::read_to_string(&index_path)?;
        let mut lines = text.lines();
        let header = lines.next().unwrap_or("").trim_end_matches('\r');
        if header != "key\torigin\tbytes\tsha256" {
            return Err(invalid(
                "legacy data/index.tsv must have header key<TAB>origin<TAB>bytes<TAB>sha256",
            ));
        }
        sources.insert(index_path.clone());
        let mut rows = BTreeMap::<String, Vec<String>>::new();
        for (number, line) in lines.enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let columns: Vec<_> = line
                .trim_end_matches('\r')
                .split('\t')
                .map(str::to_string)
                .collect();
            if columns.len() != 4 {
                return Err(invalid(format!(
                    "legacy data index row {} has {} columns",
                    number + 2,
                    columns.len()
                )));
            }
            let key = &columns[0];
            safe_relative(Path::new(key))?;
            if let Some(old) = rows.get(key) {
                if old != &columns {
                    return Err(Error::new(
                        "conflict",
                        format!("legacy Data key {key:?} has incompatible repeated rows"),
                    ));
                }
                continue;
            }
            rows.insert(key.clone(), columns);
        }
        for (key, columns) in rows {
            let id = Uuid::new_v4().to_string();
            data_ids.insert(key.clone(), id.clone());
            let mut content =
                json!({"kind":"file","mode":"closed","identity_scope":"legacy payload bytes"});
            if !columns[2].is_empty() {
                let size: Value = serde_json::from_str(&columns[2])
                    .map_err(|_| invalid(format!("invalid legacy size for {key}")))?;
                if !size.is_number()
                    || columns[2].starts_with('-')
                    || columns[2].contains(['.', 'e', 'E'])
                {
                    return Err(invalid(format!(
                        "legacy size must be a nonnegative integer for {key}"
                    )));
                }
                content["size_bytes"] = size;
            }
            if !columns[3].is_empty() {
                content["sha256"] = json!(columns[3]);
            }
            if key.ends_with(".tar.gz") || key.ends_with(".tgz") {
                content["packaging"] = json!({"format":"tar.gz","links":"legacy archive may contain links to external material; no extraction or inspection performed"});
            }
            let data_path = source.join("data").join(&key);
            let value = json!({"name":key,"aliases":[key],"content":content,"origin":{"legacy_text":columns[1]},
                "locations":[{"id":format!("legacy-{}",id),"name":"legacy","kind":"filesystem","path":data_path,
                    "host":project.host,"managed":false,"availability":"unknown","description":"Legacy path retained without payload access"}],
                "legacy":{"key":key,"row":columns,"source_index":index_path}});
            let doc = envelop(project, value, "data", &id, &when)?;
            let destination = store.path("data", &id)?;
            push_action(
                &mut plan,
                &index_path,
                &destination,
                "data",
                &id,
                store::json_bytes(&doc)?,
            )?;
            plan["identity_map"][format!("data:{key}")] = json!(id);
        }
    }

    // The recognized scaffold uses ordinary Git submodules. Read .gitmodules
    // with Git's parser, without initializing, fetching or changing a checkout.
    let gitmodules = source.join(".gitmodules");
    if gitmodules.exists() {
        sources.insert(gitmodules.clone());
        let output = Command::new("git")
            .args(["config", "--file"])
            .arg(&gitmodules)
            .args(["--get-regexp", r"^submodule\..*\.(path|url)$"])
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::new("dependency", "Git is required to parse .gitmodules")
                } else {
                    e.into()
                }
            })?;
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(invalid("Git could not parse legacy .gitmodules"));
        }
        let text = String::from_utf8(output.stdout).map_err(|e| invalid(e.to_string()))?;
        let mut modules = BTreeMap::<String, BTreeMap<String, String>>::new();
        for line in text.lines() {
            let (key, value) = line
                .split_once(' ')
                .ok_or_else(|| invalid("unexpected Git config output"))?;
            let key = key.strip_prefix("submodule.").unwrap();
            let (name, field) = key
                .rsplit_once('.')
                .ok_or_else(|| invalid("invalid submodule key"))?;
            modules
                .entry(name.to_string())
                .or_default()
                .insert(field.to_string(), value.to_string());
        }
        for (name, module) in modules {
            let Some(path) = module.get("path") else {
                continue;
            };
            safe_relative(Path::new(path))?;
            let id = Uuid::new_v4().to_string();
            model_ids.insert(name.clone(), id.clone());
            let mut repository = json!({"topology":"submodule","legacy_path":path});
            if let Some(url) = module.get("url") {
                repository["url"] = json!(url);
            }
            let value = envelop(
                project,
                json!({"name":name,"repository":repository,"status":"active","legacy":{"source":gitmodules}}),
                "model",
                &id,
                &when,
            )?;
            push_action(
                &mut plan,
                &gitmodules,
                &store.path("model", &id)?,
                "model",
                &id,
                store::json_bytes(&value)?,
            )?;
            plan["identity_map"][format!("model:{name}")] = json!(id);
            plan["bindings"]
                .as_array_mut()
                .unwrap()
                .push(json!({"family":"models","id":id,"key":"checkout","path":source.join(path)}));
        }
    }

    let tools_root = source.join("tools");
    if tools_root.is_dir() {
        for item in walkdir::WalkDir::new(&tools_root).follow_links(false) {
            let item = item?;
            if !item.file_type().is_file() || item.file_name() != "SKILL.md" {
                continue;
            }
            let skill = item.path().to_path_buf();
            sources.insert(skill.clone());
            let tool_root = skill.parent().unwrap();
            let relative = tool_root
                .strip_prefix(source)
                .map_err(|e| invalid(e.to_string()))?;
            let name = tool_root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&skill)?;
            let (front, _) = legacy_frontmatter(&text)?;
            let id = Uuid::new_v4().to_string();
            tool_ids.insert(relative.to_string_lossy().into_owned(), id.clone());
            let source_descriptor = if let Ok(relative) = tool_root.strip_prefix(&project.root) {
                json!({"mode":"live","path":relative})
            } else {
                json!({"mode":"live","external":true})
            };
            let value = envelop(
                project,
                json!({"name":name,"description":front.get("description").cloned().unwrap_or(json!("Imported project capability")),
                "source":source_descriptor,"status":"active","legacy":{"source":skill}}),
                "tool",
                &id,
                &when,
            )?;
            push_action(
                &mut plan,
                &skill,
                &store.path("tool", &id)?,
                "tool",
                &id,
                store::json_bytes(&value)?,
            )?;
            plan["identity_map"][format!("tool:{}", relative.display())] = json!(id);
            if !tool_root.starts_with(&project.root) {
                plan["bindings"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"family":"tools","id":id,"key":"source","path":tool_root}));
            }
        }
    }

    let legacy_results = source.join("results");
    for path in store::document_paths(&legacy_results, "json")? {
        let bytes = fs::read(&path)?;
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| invalid(format!("malformed legacy Result {}: {e}", path.display())))?;
        if value.get("format") == Some(&json!(1)) && value.get("kind") == Some(&json!("result")) {
            // Already converted records are covered by the saved migration map,
            // or may have been intentionally created before this import.
            store::validate_document(&value, Some("result"))?;
            if source == project.root {
                continue;
            }
        }
        let filename_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("Result filename must be UTF-8"))?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(filename_id)
            .to_string();
        store::valid_id(&id)?;
        if id != filename_id {
            return Err(Error::new(
                "conflict",
                format!(
                    "Result ID {id:?} differs from filename {}; resolve that mapping before migration",
                    path.display()
                ),
            ));
        }
        let relative = path.strip_prefix(&legacy_results).unwrap();
        let destination = project.results.join(relative);
        sources.insert(path.clone());
        // Retain every historical operand and add resolved IDs only when its
        // key has an unambiguous mapping in this same migration.
        if value.get("data").is_none() {
            let mut links = Vec::new();
            for (key, role) in [("inputs", "input"), ("outputs", "output")] {
                if let Some(operands) = value.get(key).and_then(Value::as_array) {
                    for operand in operands {
                        if let Some(key) = operand.as_str() {
                            if let Some(id) = data_ids.get(key) {
                                links.push(json!({"id":id,"role":role,"legacy_key":key}));
                            }
                        }
                    }
                }
            }
            if !links.is_empty() {
                value["data"] = json!(links);
            }
        }
        if let Some(models) = value.get_mut("models").and_then(Value::as_object_mut) {
            for model in models.values_mut() {
                let id = model
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(|name| model_ids.get(name))
                    .cloned();
                if model.get("id").is_none() {
                    if let Some(id) = id {
                        model["id"] = json!(id);
                    }
                }
            }
        }
        if let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools {
                let id = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(|name| tool_ids.get(name))
                    .cloned();
                if tool.get("id").is_none() {
                    if let Some(id) = id {
                        tool["id"] = json!(id);
                    }
                }
            }
        }
        let doc = envelop(project, value, "result", &id, &when)?;
        push_action(
            &mut plan,
            &path,
            &destination,
            "result",
            &id,
            store::json_bytes(&doc)?,
        )?;
        plan["identity_map"][format!("result:{}", relative.display())] = json!(id);
    }

    let legacy_memory = source.join("memory");
    for path in store::document_paths(&legacy_memory, "md")? {
        let text = fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            plan["legacy_reservations"]
                .as_array_mut()
                .unwrap()
                .push(json!(path));
            continue;
        }
        if memory::parse_optional(&path)?.is_some() && source == project.root {
            continue;
        }
        let relative = path.strip_prefix(&legacy_memory).unwrap();
        safe_relative(relative)?;
        let (metadata, body) = legacy_frontmatter(&text)?;
        let id = metadata
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut metadata = envelop(project, metadata, "memory", &id, &when)?;
        // The historical day is kept as a path. No UTC authored instant is
        // invented from a filename or the migration clock.
        metadata
            .as_object_mut()
            .unwrap()
            .entry("legacy_origin_path")
            .or_insert_with(|| json!(relative));
        sources.insert(path.clone());
        let destination = project.memory.join(relative);
        push_action(
            &mut plan,
            &path,
            &destination,
            "memory",
            &id,
            memory::encode(&metadata, &body)?,
        )?;
        plan["identity_map"][format!("memory:{}", relative.display())] = json!(id);
    }

    for path in sources {
        plan["sources"]
            .as_array_mut()
            .unwrap()
            .push(source_entry(&path, &backup)?);
    }
    // Find legacy callers in known guidance/Tool source files, not model source
    // or payload trees. Report locations; never rewrite the callers.
    let needles = [
        "meta/results/scripts/record.py",
        "meta/results/scripts/memory.py",
        "meta/results/scripts/index.py",
        "meta/data-store/scripts/data.py",
    ];
    let mut caller_files = Vec::new();
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let path = source.join(name);
        if path.is_file() {
            caller_files.push(path);
        }
    }
    for root in [source.join("tools"), source.join("meta")] {
        if root.is_dir() {
            for entry in walkdir::WalkDir::new(root).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    let extension = entry
                        .path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if ["md", "py", "sh", "toml", "json", "js", "mjs"].contains(&extension)
                        && entry
                            .metadata()
                            .map(|m| m.len() < 2 * 1024 * 1024)
                            .unwrap_or(false)
                    {
                        caller_files.push(entry.into_path());
                    }
                }
            }
        }
    }
    for path in caller_files {
        if let Ok(text) = fs::read_to_string(&path) {
            for (number, line) in text.lines().enumerate() {
                let references: Vec<_> = needles
                    .iter()
                    .filter(|needle| line.contains(**needle))
                    .copied()
                    .collect();
                if !references.is_empty() {
                    plan["call_sites"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"path":path,"line":number+1,"references":references}));
                }
            }
        }
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for action in plan["actions"].as_array().unwrap() {
        if !ids.insert(field(action, "id")?.to_string()) {
            return Err(Error::new(
                "conflict",
                "two legacy objects claim the same ID",
            ));
        }
        if !paths.insert(field(action, "destination")?.to_string()) {
            return Err(Error::new(
                "conflict",
                "two legacy objects resolve to the same destination",
            ));
        }
    }
    Ok(plan)
}

fn summary(plan: &Value, state: &Path, dry_run: bool) -> Value {
    let actions = plan["actions"].as_array().unwrap();
    json!({"dry_run":dry_run,"migration_id":plan["migration_id"],"status":plan["status"],
        "plan":state.join("plan.json"),"backup":state.join("backup"),
        "objects":actions.iter().map(|action|json!({"kind":action["kind"],"id":action["id"],"source":action["source"],
            "destination":action["destination"],"complete":action["complete"]})).collect::<Vec<_>>(),
        "legacy_reservations":plan["legacy_reservations"],"legacy_call_sites":plan["call_sites"],
        "identity_map":plan["identity_map"],"committed":!dry_run && plan["status"]=="complete"})
}

/// `migrate` without a legacy source. Format 1 is the only persisted format, so a
/// readable project is already current.
pub fn current(project: &Project, dry_run: bool) -> Value {
    json!({"committed":false,"dry_run":dry_run,"current":true,"changed":false,"format":crate::FORMAT,
        "project_id":project.project_id,"message":"Project is at format 1; nothing to migrate"})
}

pub fn migrate(project: &Project, legacy_root: &Path, dry_run: bool) -> Result<Value> {
    let source = fs::canonicalize(project.resolve(legacy_root))?;
    if !source.is_dir() {
        return Err(invalid("legacy source must be a directory"));
    }
    let state = project.state_dir.join("migrations").join(format!(
        "legacy-{}",
        &store::revision(source.to_string_lossy().as_bytes())[..24]
    ));
    let plan_path = state.join("plan.json");
    let store = Store::new(project);
    let lock = store.lock()?;
    let mut plan = if plan_path.exists() {
        serde_json::from_slice(&fs::read(&plan_path)?)?
    } else {
        plan_migration(project, &source, &state)?
    };
    if plan.get("project_id").and_then(Value::as_str) != Some(&project.project_id)
        || plan.get("legacy_root").and_then(Value::as_str) != source.to_str()
    {
        return Err(Error::new(
            "conflict",
            "migration plan is bound to another project/source",
        ));
    }
    if dry_run {
        return Ok(summary(&plan, &state, true));
    }
    if !plan_path.exists() {
        // Original bytes and assigned IDs are retained before the first change.
        for entry in plan["sources"].as_array().unwrap() {
            let path = PathBuf::from(field(entry, "path")?);
            let bytes = fs::read(&path)?;
            if store::revision(&bytes) != field(entry, "revision")? {
                return Err(Error::new(
                    "conflict",
                    format!("legacy input changed while planning: {}", path.display()),
                ));
            }
            let backup = PathBuf::from(field(entry, "backup")?);
            if backup.exists() {
                if fs::read(&backup)? != bytes {
                    return Err(Error::new(
                        "conflict",
                        "a retained migration backup differs from its source; preserve both before retrying",
                    ));
                }
                continue;
            }
            let publication = store::atomic_write(&backup, &bytes, false)?;
            if publication.durability == "uncertain" {
                return Err(Error::new(
                    "durability",
                    "migration backup publication durability is uncertain",
                ));
            }
        }
        write_json(&plan_path, &plan, false)?;
    }
    // Re-check every original metadata input before applying remaining actions.
    // In-place destinations already carrying the planned bytes are admissible.
    for entry in plan["sources"].as_array().unwrap() {
        let path = PathBuf::from(field(entry, "path")?);
        let bytes = fs::read(&path)?;
        let revision = store::revision(&bytes);
        let converted =
            plan["actions"].as_array().unwrap().iter().any(|action| {
                action["destination"] == entry["path"] && action["revision"] == revision
            });
        if revision != field(entry, "revision")? && !converted {
            return Err(Error::new(
                "conflict",
                format!(
                    "legacy source changed since the saved migration plan: {}",
                    path.display()
                ),
            )
            .detail(json!({"plan":plan_path,"source":path,"backup":entry["backup"]})));
        }
    }
    for position in 0..plan["actions"].as_array().unwrap().len() {
        let action = plan["actions"][position].clone();
        let destination = PathBuf::from(field(&action, "destination")?);
        let desired = field(&action, "content")?.as_bytes();
        let input = PathBuf::from(field(&action, "source")?);
        let replacing = if destination.exists() {
            let current = fs::read(&destination)?;
            if current == desired {
                plan["actions"][position]["complete"] = json!(true);
                continue;
            }
            let expected = plan["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["path"] == action["source"])
                .ok_or_else(|| invalid("migration source mapping is missing"))?;
            if destination != input || store::revision(&current) != field(expected, "revision")? {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "migration destination has independent content: {}",
                        destination.display()
                    ),
                )
                .detail(json!({"plan":plan_path,"destination":destination,"completed":position})));
            }
            true
        } else {
            false
        };
        // Check managed IDs across all families without treating unconverted
        // legacy JSON as invalid; its values remain untouched until its turn.
        for kind in store::KINDS {
            for path in store::document_paths(&store.directory(kind)?, "json")? {
                if path == destination {
                    continue;
                }
                let doc: Value = serde_json::from_slice(&fs::read(&path)?)?;
                if doc.get("id") == action.get("id") {
                    return Err(Error::new(
                        "conflict",
                        format!("migration ID already exists at {}", path.display()),
                    ));
                }
            }
        }
        for path in store::document_paths(&project.memory, "md")? {
            if path == destination {
                continue;
            }
            if let Some(doc) = memory::parse_optional(&path)? {
                if doc.metadata.get("id") == action.get("id") {
                    return Err(Error::new(
                        "conflict",
                        format!("migration ID already exists at {}", path.display()),
                    ));
                }
            }
        }
        let publication = store::atomic_write(&destination, desired, replacing)?;
        if publication.durability == "uncertain" {
            return Err(Error::new(
                "durability",
                "converted document is published but its durability is uncertain",
            )
            .detail(json!({"published":true,"path":destination,"plan":plan_path})));
        }
        plan["actions"][position]["complete"] = json!(true);
        write_json(&plan_path, &plan, true)?;
    }
    plan["status"] = json!("complete");
    let portable = project
        .root
        .join(".accrete/migrations")
        .join(format!("{}.json", field(&plan, "migration_id")?));
    let public_map = json!({"format":1,"migration_id":plan["migration_id"],"project_id":project.project_id,
        "legacy_root":source,"status":"complete","identity_map":plan["identity_map"],
        "paths":plan["actions"].as_array().unwrap().iter().map(|action|json!({"id":action["id"],"source":action["source"],"destination":action["destination"]})).collect::<Vec<_>>()});
    write_json(&portable, &public_map, portable.exists())?;
    write_json(&plan_path, &plan, true)?;
    let mut result = summary(&plan, &state, false);
    result["portable_map"] = json!(portable);
    match index::refresh_locked(project) {
        Ok(index) => result["index"] = index,
        Err(error) => {
            result["index_pending"] = json!(true);
            result["index_error"] = json!(error.to_string());
        }
    }
    drop(lock);
    let mut bindings = Vec::new();
    for binding in plan["bindings"].as_array().unwrap() {
        let response = project.bind(
            field(binding, "family")?,
            field(binding, "id")?,
            field(binding, "key")?,
            field(binding, "path")?,
        );
        match response {
            Ok(value) => bindings.push(json!({"binding":binding,"committed":true,"outcome":value})),
            Err(error) => bindings
                .push(json!({"binding":binding,"committed":false,"error":error.to_string()})),
        }
    }
    result["bindings"] = json!(bindings);
    Ok(result)
}
