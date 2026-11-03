use crate::{
    config::{Project, canonical_future, table_str},
    error::{Error, Result},
    git,
    store::{self, Store},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn slug(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::new(
            "invalid",
            "exposure names use 1–64 lowercase letters, digits and single hyphens",
        ));
    }
    Ok(())
}
pub fn admit(
    project: &Project,
    mut payload: Value,
    source: Option<&Path>,
    mode: &str,
    revision: Option<&str>,
) -> Result<Value> {
    if !payload.is_object() {
        return Err(Error::new("invalid", "Tool input must be an object"));
    }
    if mode != "live" && mode != "retained" {
        return Err(Error::new(
            "invalid",
            "source mode must be live or retained",
        ));
    }
    payload
        .as_object_mut()
        .unwrap()
        .entry("status")
        .or_insert(json!("active"));
    let mut external = None;
    if let Some(source) = source {
        let source = canonical_future(&project.resolve(source))?;
        if !source.is_dir() {
            return Err(Error::new(
                "invalid",
                "Tool source must be an existing directory",
            ));
        }
        let mut desc = json!({"mode":mode});
        if let Ok(relative) = source.strip_prefix(&project.root) {
            desc["path"] = json!(relative);
        } else {
            desc["external"] = json!(true);
            external = Some(source.clone());
        }
        if let Some(revision) = revision {
            desc["revision"] = json!(revision);
        }
        if mode == "retained" && revision.is_none() {
            return Err(Error::new(
                "invalid",
                "retained source requires --revision (Git commit)",
            ));
        }
        if let Ok(observation) = git::observe(&source, &[]) {
            desc["admission_commit"] = observation["commit"].clone();
            desc["admission_dirty"] = observation["dirty"].clone();
        }
        payload["source"] = desc;
    } else if payload.get("source").is_none() {
        return Err(Error::new(
            "invalid",
            "supply --source or a source descriptor",
        ));
    }
    let mut result = Store::new(project).create("tool", payload)?;
    if let Some(source) = external {
        match project.bind(
            "tools",
            result["object_id"].as_str().unwrap(),
            "source",
            &source.to_string_lossy(),
        ) {
            Ok(binding) => result["local_binding"] = binding,
            Err(e) => {
                result["binding_pending"] = json!(true);
                result["warning"] = json!(e.to_string());
            }
        }
    }
    Ok(result)
}
pub fn source(project: &Project, tool: &Value) -> Result<PathBuf> {
    let id = tool["id"].as_str().unwrap_or("");
    let name = tool["name"].as_str().unwrap_or(id);
    let path = table_str(&project.config, &["tools", id, "source"])
        .or_else(|| table_str(&project.config, &["tools", name, "source"]))
        .or_else(|| {
            tool.get("aliases").and_then(Value::as_array).and_then(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .find_map(|alias| table_str(&project.config, &["tools", alias, "source"]))
            })
        })
        .or_else(|| tool["source"]["path"].as_str())
        .ok_or_else(|| {
            Error::new(
                "config",
                format!("Tool {id} has no source on this host; use tool bind"),
            )
        })?;
    let path = canonical_future(&project.resolve(path))?;
    if !path.is_dir() {
        return Err(Error::new(
            "config",
            format!("Tool source is unavailable: {}", path.display()),
        ));
    }
    if tool["source"]["mode"] == "retained" {
        let requested = tool["source"]["revision"]
            .as_str()
            .ok_or_else(|| Error::new("invalid", "retained Tool is missing source.revision"))?;
        let wanted = git::resolve_commit(&path, requested)?;
        let obs = git::observe(&path, &[])?;
        if obs["commit"] != wanted || obs["dirty"] != false {
            return Err(Error::new(
                "conflict",
                "retained exposure needs a clean checkout at the selected commit; bind a matching checkout",
            ));
        }
    }
    Ok(path)
}
pub fn bind(project: &Project, selector: &str, path: &Path) -> Result<Value> {
    let doc = Store::new(project).read("tool", selector)?;
    let path = canonical_future(&project.resolve(path))?;
    if !path.is_dir() {
        return Err(Error::new(
            "invalid",
            "Tool binding must be an existing directory",
        ));
    }
    project.bind(
        "tools",
        doc.doc["id"].as_str().unwrap(),
        "source",
        &path.to_string_lossy(),
    )
}

fn inventory_path(project: &Project, id: &str) -> PathBuf {
    project
        .state_dir
        .join("exposures")
        .join(format!("{id}.json"))
}
fn inventory(project: &Project, id: &str) -> Result<Value> {
    let path = inventory_path(project, id);
    if path.exists() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        Ok(json!({"tool_id":id,"exposures":[]}))
    }
}
fn save_inventory(project: &Project, id: &str, inv: &Value) -> Result<()> {
    store::atomic_write(
        &inventory_path(project, id),
        &serde_json::to_vec_pretty(inv)?,
        true,
    )?;
    Ok(())
}
fn check_owned(destination: &Path, entry: &Value) -> Result<()> {
    if let Some(previous) = entry.get("previous") {
        let mut current = entry.clone();
        current.as_object_mut().unwrap().remove("previous");
        if check_owned(destination, &current).is_ok() || check_owned(destination, previous).is_ok()
        {
            return Ok(());
        }
        return Err(Error::new(
            "conflict",
            "interrupted exposure differs from both previous and intended generated content",
        ));
    }
    let meta = match fs::symlink_metadata(destination) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if meta.file_type().is_symlink() {
        if entry["mode"] != "symlink"
            || fs::read_link(destination)? != PathBuf::from(entry["source"].as_str().unwrap_or(""))
        {
            return Err(Error::new(
                "conflict",
                "managed exposure symlink was changed; preserving it",
            ));
        }
    } else {
        if entry["mode"] != "wrapper" || !meta.is_dir() {
            return Err(Error::new("conflict", "exposure destination changed type"));
        }
        let file = destination.join("SKILL.md");
        if file.exists()
            && entry.get("digest").and_then(Value::as_str)
                != Some(store::revision(&fs::read(file)?).as_str())
        {
            return Err(Error::new(
                "conflict",
                "generated Skill was edited; preserving it",
            ));
        }
        for e in fs::read_dir(destination)? {
            if e?.file_name() != "SKILL.md" {
                return Err(Error::new(
                    "conflict",
                    "exposure contains additional files; preserving them",
                ));
            }
        }
    }
    Ok(())
}
pub fn expose(
    project: &Project,
    selector: &str,
    host: &str,
    mode: &str,
    name: Option<&str>,
    destination: Option<&Path>,
) -> Result<Value> {
    let store = Store::new(project);
    let _lock = store.lock()?;
    let tool = store.read("tool", selector)?;
    let id = tool.doc["id"].as_str().unwrap();
    if tool.doc["status"] == "retired" {
        return Err(Error::new("invalid", "retired Tool cannot be exposed"));
    }
    let source = source(project, &tool.doc)?;
    let name = name.or_else(|| tool.doc["name"].as_str()).unwrap_or(id);
    slug(name)?;
    let base = match host {
        "codex" => ".agents/skills",
        "claude" => ".claude/skills",
        _ => return Err(Error::new("invalid", "host must be codex or claude")),
    };
    if mode != "wrapper" && mode != "symlink" {
        return Err(Error::new(
            "invalid",
            "exposure mode must be wrapper or symlink",
        ));
    }
    let destination = destination
        .map(|d| project.resolve(d))
        .unwrap_or_else(|| project.root.join(base).join(name));
    let parent = canonical_future(
        destination
            .parent()
            .ok_or_else(|| Error::new("invalid", "exposure needs a parent directory"))?,
    )?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or_else(|| Error::new("invalid", "exposure destination needs a name"))?,
    );
    if destination == source || source.starts_with(&destination) {
        return Err(Error::new(
            "invalid",
            "exposure must not contain its source",
        ));
    }
    if mode == "symlink" {
        let text = fs::read_to_string(source.join("SKILL.md"))?;
        let yaml = text
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---"))
            .map(|(a, _)| a)
            .ok_or_else(|| {
                Error::new(
                    "invalid",
                    "symlink exposure needs SKILL.md with YAML frontmatter",
                )
            })?;
        let front: Value = serde_yaml::from_str(yaml)?;
        if front["name"] != name || front["description"].as_str().unwrap_or("").is_empty() {
            return Err(Error::new(
                "invalid",
                "symlink Skill name must match the exposure name and contain description",
            ));
        }
    }
    let mut inv = inventory(project, id)?;
    let entries = inv["exposures"].as_array_mut().unwrap();
    let existing = entries
        .iter()
        .position(|e| e["destination"] == destination.to_string_lossy().as_ref());
    if let Some(i) = existing {
        check_owned(&destination, &entries[i])?;
    } else if fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::new(
            "conflict",
            format!(
                "exposure destination is not owned by this Tool: {}",
                destination.display()
            ),
        ));
    }
    let description = tool.doc["description"]
        .as_str()
        .unwrap_or("Use this project capability when relevant to the assigned work.");
    let skill = if source.join("SKILL.md").is_file() {
        source.join("SKILL.md")
    } else {
        source.clone()
    };
    let header = serde_yaml::to_string(&json!({"name":name,"description":description}))?;
    let wrapper = format!(
        "---\n{header}---\n\n# {name}\n\nACCRETE managed exposure for Tool `{id}`.\n\nRead the capability instructions at `{}` before using it. The source directory is `{}`. Resolve all relative script, reference and asset paths from that source directory. Run programs from the working directory specified by those instructions. This wrapper contains no implementation.\n\nSource selection: {}. The source remains under its owner's normal editing and Git workflow.\n",
        skill.display(),
        source.display(),
        tool.doc["source"]["mode"].as_str().unwrap_or("live")
    );
    let entry = json!({"host":host,"mode":mode,"name":name,"destination":destination,"source":source,"digest":store::revision(wrapper.as_bytes())});
    if let Some(i) = existing {
        let previous = entries[i].clone();
        entries[i] = entry.clone();
        entries[i]["previous"] = previous;
    } else {
        entries.push(entry.clone());
    }
    // Persist intended ownership before touching generated output. Refresh reconciles
    // the same destination after interruption without touching source content.
    save_inventory(project, id, &inv)?;
    fs::create_dir_all(&parent)?;
    if let Ok(meta) = fs::symlink_metadata(&destination) {
        if meta.file_type().is_symlink() {
            fs::remove_file(&destination)?;
        } else if mode == "symlink" {
            fs::remove_file(destination.join("SKILL.md"))?;
            fs::remove_dir(&destination)?;
        }
    }
    if mode == "symlink" {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &destination)?;
        #[cfg(not(unix))]
        return Err(Error::new(
            "invalid",
            "symlink exposure is supported on Unix; choose wrapper",
        ));
    } else {
        fs::create_dir_all(&destination)?;
        store::atomic_write(&destination.join("SKILL.md"), wrapper.as_bytes(), true)?;
    }
    let entries = inv["exposures"].as_array_mut().unwrap();
    let i = entries
        .iter()
        .position(|e| e["destination"] == destination.to_string_lossy().as_ref())
        .unwrap();
    entries[i] = entry;
    save_inventory(project, id, &inv)?;
    // Host-specific absolute source bindings must not travel in project Git.
    // Git's repository-local exclude file avoids changing user instructions.
    if let Ok(relative) = destination.strip_prefix(&project.root) {
        if let Ok(exclude) =
            git::checked(&project.root, ["rev-parse", "--git-path", "info/exclude"])
        {
            let exclude = project.resolve(exclude);
            let pattern = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
            let mut text = fs::read_to_string(&exclude).unwrap_or_default();
            if !text.lines().any(|line| line == pattern) {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&pattern);
                text.push('\n');
                store::atomic_write(&exclude, text.as_bytes(), true)?;
            }
        }
    }
    Ok(
        json!({"object_id":id,"committed":true,"exposed":true,"host":host,"path":destination,"source":source,"mode":mode}),
    )
}
pub fn refresh(project: &Project, selector: &str) -> Result<Value> {
    let tool = Store::new(project).read("tool", selector)?;
    let id = tool.doc["id"].as_str().unwrap();
    let inv = inventory(project, id)?;
    let mut effects = Vec::new();
    if tool.doc["status"] == "retired" {
        return remove_exposures(project, id);
    }
    for e in inv["exposures"].as_array().unwrap() {
        match expose(
            project,
            id,
            e["host"].as_str().unwrap(),
            e["mode"].as_str().unwrap(),
            e["name"].as_str(),
            e["destination"].as_str().map(Path::new),
        ) {
            Ok(r) => effects.push(r),
            Err(err) => {
                effects.push(json!({"pending":true,"path":e["destination"],"error":err.json()}))
            }
        }
    }
    Ok(
        json!({"object_id":id,"exposures":effects,"exposure_pending":effects.iter().any(|e|e["pending"]==true)}),
    )
}
fn remove_exposures(project: &Project, id: &str) -> Result<Value> {
    let store = Store::new(project);
    let _lock = store.lock()?;
    let mut inv = inventory(project, id)?;
    let mut pending = Vec::new();
    let mut removed = Vec::new();
    for entry in inv["exposures"].as_array().unwrap() {
        let path = Path::new(entry["destination"].as_str().unwrap());
        if let Err(e) = check_owned(path, entry) {
            pending.push(json!({"entry":entry,"error":e.to_string()}));
            continue;
        }
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() {
                fs::remove_file(path)?;
            } else {
                if path.join("SKILL.md").exists() {
                    fs::remove_file(path.join("SKILL.md"))?;
                }
                fs::remove_dir(path)?;
            }
        }
        removed.push(json!(path));
    }
    inv["exposures"] = json!(
        pending
            .iter()
            .map(|e| e["entry"].clone())
            .collect::<Vec<_>>()
    );
    save_inventory(project, id, &inv)?;
    Ok(
        json!({"object_id":id,"removed":removed,"pending":pending,"exposure_pending":!pending.is_empty()}),
    )
}
pub fn retire(project: &Project, selector: &str, expect: &str) -> Result<Value> {
    let mut response = Store::new(project).amend(
        "tool",
        selector,
        json!({"status":"retired","retired_at":store::now()}),
        expect,
    )?;
    let id = response["object_id"].as_str().unwrap();
    match remove_exposures(project, id) {
        Ok(v) => response["exposures"] = v,
        Err(e) => {
            response["exposure_pending"] = json!(true);
            response["warning"] = json!(e.to_string());
        }
    }
    Ok(response)
}
pub fn export(project: &Project, selector: &str, destination: &Path) -> Result<Value> {
    let tool = Store::new(project).read("tool", selector)?;
    let source = source(project, &tool.doc)?;
    let destination = canonical_future(&project.resolve(destination))?;
    if destination.exists() {
        return Err(Error::new("conflict", "Tool export destination exists"));
    }
    if destination.starts_with(&source) {
        return Err(Error::new(
            "invalid",
            "Tool export cannot be inside its source",
        ));
    }
    fs::create_dir_all(&destination)?;
    let stage = tempfile::Builder::new()
        .prefix(".accrete-export-")
        .tempdir_in(destination.parent().unwrap())?;
    let root = stage.path().join("source");
    fs::create_dir(&root)?;
    for item in walkdir::WalkDir::new(&source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let item = item?;
        let rel = item.path().strip_prefix(&source).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let to = root.join(rel);
        if item.file_type().is_symlink() {
            return Err(Error::new(
                "invalid",
                "Tool export encountered a symlink; create a domain-owned export for explicit link semantics",
            ));
        }
        if item.file_type().is_dir() {
            fs::create_dir_all(to)?;
        } else {
            fs::copy(item.path(), to)?;
        }
    }
    fs::write(
        stage.path().join("tool.json"),
        serde_json::to_vec_pretty(&tool.doc)?,
    )?;
    fs::remove_dir(&destination)?;
    fs::rename(stage.keep(), &destination)?;
    Ok(
        json!({"object_id":tool.doc["id"],"path":destination,"exported":true,"source_revision":tool.revision}),
    )
}
