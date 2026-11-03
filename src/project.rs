//! Native project initialization and explicit, ownership-aware template refresh.
use crate::config::{Project, canonical_future};
use crate::error::{Error, Result};
use crate::store::{Store, atomic_write, create_dirs, json_bytes, now, revision, sync_directory};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub fn init(
    path: &Path,
    name: Option<&str>,
    timezone: &str,
    id: Option<&str>,
    git_init: bool,
    template: Option<&Path>,
) -> Result<Value> {
    init_with_context(path, name, timezone, id, git_init, template, None, None)
}

#[allow(clippy::too_many_arguments)]
pub fn init_with_context(
    path: &Path,
    name: Option<&str>,
    timezone: &str,
    id: Option<&str>,
    git_init: bool,
    template: Option<&Path>,
    state_override: Option<&Path>,
    host_override: Option<&str>,
) -> Result<Value> {
    timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| Error::new("config", format!("Unknown IANA timezone {timezone}")))?;
    if let Some(id) = id {
        uuid::Uuid::parse_str(id)
            .map_err(|_| Error::new("invalid", "Project --id must be a UUID"))?;
    }
    let root = canonical_future(path)?;
    let marker = root.join(".accrete/project.toml");
    let created = !marker.exists();
    if git_init {
        let status = Command::new("git").arg("--version").output().map_err(|e| {
            Error::new(
                "dependency",
                format!("Git is needed for initialization; use --no-git to omit it: {e}"),
            )
        })?;
        if !status.status.success() {
            return Err(Error::new("dependency", "Git could not run"));
        }
    }
    create_dirs(&root)?;
    no_symlinks(&root, Path::new(".accrete/project.toml"))?;
    if created {
        let project_id = id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let project_name = name.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let mut config: toml::Value = toml::from_str(
            r#"
format = 1
[paths]
metadata = ".accrete/objects"
results = "results"
memory = "memory"
tool_sources = "tools"
scratch = "scratch"
[locations.working]
kind = "filesystem"
root = ".accrete/data"
"#,
        )?;
        config
            .as_table_mut()
            .unwrap()
            .insert("project_id".into(), toml::Value::String(project_id));
        config
            .as_table_mut()
            .unwrap()
            .insert("name".into(), toml::Value::String(project_name));
        config
            .as_table_mut()
            .unwrap()
            .insert("timezone".into(), toml::Value::String(timezone.into()));
        let text = toml::to_string_pretty(&config)?;
        let publication = atomic_write(&marker, text.as_bytes(), false)?;
        if publication.durability != "durable" {
            return Err(Error::new(
                "durability",
                "Project marker was published, but its durability could not be confirmed",
            )
            .detail(json!({"published":true,"path":marker,"warning":publication.warning})));
        }
    }
    let project = Project::discover(Some(&root), state_override, host_override)?;
    if id.is_some_and(|id| id != project.project_id) {
        return Err(Error::new(
            "conflict",
            "Existing project identity differs from the requested ID",
        )
        .detail(json!({"path":marker,"project_id":project.project_id})));
    }
    if let Some(name) = name {
        if project.config.get("name").and_then(toml::Value::as_str) != Some(name) {
            return Err(Error::new(
                "conflict",
                "Existing project name differs; initialization does not rewrite shared configuration",
            ));
        }
    }
    let mut paths = Vec::new();
    {
        let store = Store::new(&project);
        let _lock = store.lock()?;
        for directory in [
            &project.metadata,
            &project.results,
            &project.memory,
            &project.tool_sources,
            &project.scratch,
        ] {
            create_dirs(directory)?;
        }
        for kind in ["models", "iterations", "tools", "data"] {
            create_dirs(&project.metadata.join(kind))?;
        }
        let ignore_path = root.join(".gitignore");
        no_symlinks(&root, Path::new(".gitignore"))?;
        let original = if ignore_path.exists() {
            fs::read_to_string(&ignore_path)?
        } else {
            String::new()
        };
        let entries = ["/.accrete/local.toml", "/.accrete/data/", "/scratch/"];
        let missing: Vec<_> = entries
            .iter()
            .filter(|entry| !original.lines().any(|line| line.trim() == **entry))
            .collect();
        if !missing.is_empty() {
            let mut updated = original.clone();
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str("\n# ACCRETE local configuration and working storage\n");
            for entry in missing {
                updated.push_str(entry);
                updated.push('\n');
            }
            let publication = atomic_write(&ignore_path, updated.as_bytes(), ignore_path.exists())?;
            if publication.durability != "durable" {
                return Err(Error::new(
                    "durability",
                    "Ignore rules were published with uncertain directory durability",
                )
                .detail(json!({"published":true,"path":ignore_path})));
            }
            paths.push(ignore_path);
        }
    }
    let mut git_created = false;
    if git_init {
        let inside = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--git-dir"])
            .output()?;
        if !inside.status.success() {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .arg("init")
                .output()?;
            if !output.status.success() {
                return Err(Error::new("external", "Project files are initialized, but git init failed").detail(json!({"project_initialized":true,"project_root":root,"stderr":String::from_utf8_lossy(&output.stderr)})));
            }
            git_created = true;
        }
    }
    let mut response = json!({"committed":true,"project_id":project.project_id,"object_id":project.project_id,
        "path":marker,"project_root":root,"format":1,"created":created,"git_initialized":git_created,"paths_written":paths,"timezone":project.timezone});
    if let Some(source) = template {
        response["template"] = upgrade(&project, source, false)?;
    }
    Ok(response)
}

#[derive(Deserialize)]
struct Manifest {
    format: u32,
    asset_version: String,
    requires: Requirements,
    files: Vec<Asset>,
}
#[derive(Deserialize)]
struct Requirements {
    cli_min: String,
    cli_max_exclusive: String,
    project_format: u32,
}
#[derive(Deserialize)]
struct Asset {
    source: String,
    destination: String,
    #[serde(default)]
    executable: bool,
}

fn semver(text: &str) -> Result<(u64, u64, u64)> {
    let parts: Vec<_> = text.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(Error::new(
            "invalid",
            "Template CLI versions must be numeric major.minor.patch",
        ));
    }
    Ok((
        parts[0]
            .parse()
            .map_err(|_| Error::new("invalid", "Version component overflows"))?,
        parts[1]
            .parse()
            .map_err(|_| Error::new("invalid", "Version component overflows"))?,
        parts[2]
            .parse()
            .map_err(|_| Error::new("invalid", "Version component overflows"))?,
    ))
}

fn relative(text: &str) -> Result<PathBuf> {
    if text.is_empty()
        || text.contains('\\')
        || text.contains(':')
        || text.bytes().any(|c| c < 32)
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::new(
            "invalid",
            format!("Template paths must be portable relative paths without traversal: {text}"),
        ));
    }
    let path = PathBuf::from(text);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::new(
            "invalid",
            format!("Invalid relative template path: {text}"),
        ));
    }
    Ok(path)
}

fn no_symlinks(root: &Path, relative: &Path) -> Result<()> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "Template/init path traverses a symbolic link: {}",
                        path.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn file_state(path: &Path) -> Result<Value> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            Ok(json!({"other":true}))
        }
        Ok(_) => Ok(json!({"sha256":revision(&fs::read(path)?),"executable":executable(path)?})),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(error) => Err(error.into()),
    }
}
#[cfg(unix)]
fn executable(path: &Path) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn executable(_path: &Path) -> Result<bool> {
    Ok(false)
}
fn set_mode(path: &Path, enabled: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if enabled { 0o755 } else { 0o644 }),
        )?;
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, enabled);
    }
    Ok(())
}

pub fn upgrade(project: &Project, source: &Path, dry_run: bool) -> Result<Value> {
    let source = fs::canonicalize(source)?;
    let manifest_path = source.join("accrete-template.json");
    no_symlinks(&source, Path::new("accrete-template.json"))?;
    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest_digest = revision(&manifest_bytes);
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.format != 1 || manifest.requires.project_format != 1 {
        return Err(Error::new(
            "unsupported_format",
            "Template manifest/project format is unsupported; both must be 1",
        ));
    }
    if manifest.asset_version.trim().is_empty() {
        return Err(Error::new(
            "invalid",
            "Template asset_version must be nonempty",
        ));
    }
    let current = semver(env!("CARGO_PKG_VERSION"))?;
    let minimum = semver(&manifest.requires.cli_min)?;
    let maximum = semver(&manifest.requires.cli_max_exclusive)?;
    if minimum >= maximum || current < minimum || current >= maximum {
        return Err(Error::new("unsupported_format", "This template is incompatible with the installed CLI").detail(json!({"cli":env!("CARGO_PKG_VERSION"),"requires":{"cli_min":manifest.requires.cli_min,"cli_max_exclusive":manifest.requires.cli_max_exclusive}})));
    }
    let store = Store::new(project);
    let _lock = store.lock()?;
    let inventory_path = project.root.join(".accrete/template-install.json");
    no_symlinks(&project.root, Path::new(".accrete/template-install.json"))?;
    let inventory_bytes = if inventory_path.exists() {
        Some(fs::read(&inventory_path)?)
    } else {
        None
    };
    let mut inventory: Value = match &inventory_bytes {
        Some(bytes) => serde_json::from_slice(bytes)?,
        None => json!({"format":1,"files":{}}),
    };
    if inventory["format"] != 1 || !inventory["files"].is_object() {
        return Err(Error::new(
            "unsupported_format",
            "Invalid or unsupported template installation inventory",
        ));
    }
    let receipt_path = project.state_dir.join("receipts/template-upgrade.json");
    let pending: Option<Value> = if receipt_path.exists() {
        Some(serde_json::from_slice(&fs::read(&receipt_path)?)?)
    } else {
        None
    };
    if let Some(pending) = &pending {
        if pending["manifest_sha256"] != manifest_digest {
            return Err(Error::new("incomplete", "A template refresh is incomplete; retry its original template before applying another").detail(json!({"receipt":receipt_path,"manifest_sha256":pending["manifest_sha256"]})));
        }
        if pending["inventory_sha256"] != json!(inventory_bytes.as_ref().map(|b| revision(b)))
            && inventory["manifest_sha256"] != manifest_digest
        {
            return Err(Error::new(
                "conflict",
                "Template inventory changed after the interrupted refresh",
            )
            .detail(json!({"receipt":receipt_path,"inventory":inventory_path})));
        }
    }
    let mut assets = BTreeMap::new();
    let mut destinations = BTreeSet::new();
    let mut plans = Vec::new();
    for asset in &manifest.files {
        let source_relative = relative(&asset.source)?;
        let destination_relative = relative(&asset.destination)?;
        if !destinations.insert(asset.destination.clone()) {
            return Err(Error::new(
                "invalid",
                format!("Duplicate template destination {}", asset.destination),
            ));
        }
        no_symlinks(&source, &source_relative)?;
        no_symlinks(&project.root, &destination_relative)?;
        let destination = project.root.join(&destination_relative);
        let source_path = source.join(&source_relative);
        if !source_path.is_file() {
            return Err(Error::new(
                "invalid",
                format!(
                    "Template source must be a regular file: {}",
                    source_path.display()
                ),
            ));
        }
        let protected = destination_relative.starts_with(".git")
            || [
                ".accrete/project.toml",
                ".accrete/local.toml",
                ".accrete/template-install.json",
            ]
            .iter()
            .any(|p| destination_relative == Path::new(p))
            || [
                &project.metadata,
                &project.results,
                &project.memory,
                &project.tool_sources,
                &project.scratch,
                &project.state_dir,
            ]
            .iter()
            .any(|p| destination.starts_with(p));
        if protected {
            return Err(Error::new(
                "invalid",
                format!(
                    "Template destination overlaps project state or managed content: {}",
                    destination.display()
                ),
            ));
        }
        let bytes = fs::read(&source_path)?;
        let desired = json!({"sha256":revision(&bytes),"executable":asset.executable});
        let current = file_state(&destination)?;
        let prior = &inventory["files"][&asset.destination];
        let resumed = pending
            .as_ref()
            .and_then(|p| p["files"].as_array())
            .and_then(|files| files.iter().find(|f| f["destination"] == asset.destination));
        if resumed.is_some_and(|p| p["desired"] != desired) {
            return Err(Error::new("conflict", "Template source bytes changed during an interrupted refresh; restore the original snapshot before retrying").detail(json!({"source":source_path,"receipt":receipt_path})));
        }
        let receipt_matches = resumed.is_some_and(|p| p["desired"] == desired);
        let owned = prior.get("sha256").is_some()
            && current["sha256"] == prior["sha256"]
            && current["executable"] == prior["executable"];
        let status = if current.is_null() {
            "create"
        } else if current == desired && (owned || receipt_matches) {
            "unchanged"
        } else if owned {
            "replace"
        } else if receipt_matches && current["sha256"] == desired["sha256"] {
            "finish_permissions"
        } else {
            "conflict"
        };
        plans.push(json!({"destination":asset.destination,"path":destination,"source":asset.source,"status":status,"before":current,"desired":desired}));
        assets.insert(asset.destination.clone(), bytes);
    }
    // A destination cannot be both a file and another destination's parent.
    for a in &destinations {
        for b in &destinations {
            if a != b && Path::new(b).starts_with(Path::new(a)) {
                return Err(Error::new(
                    "invalid",
                    format!("Template file destinations overlap: {a} and {b}"),
                ));
            }
        }
    }
    let conflicts: Vec<_> = plans
        .iter()
        .filter(|p| p["status"] == "conflict")
        .cloned()
        .collect();
    if dry_run {
        return Ok(
            json!({"committed":false,"dry_run":true,"asset_version":manifest.asset_version,"compatible":true,"files":plans,"conflicts":conflicts}),
        );
    }
    if !conflicts.is_empty() {
        return Err(Error::new(
            "conflict",
            "Template files contain owner edits or existing unowned content; no asset was changed",
        )
        .detail(json!({"conflicts":conflicts,"source":source})));
    }
    let receipt = json!({"format":1,"operation":"template_upgrade","source":source,"manifest_sha256":manifest_digest,
        "inventory_sha256":inventory_bytes.as_ref().map(|b|revision(b)),"asset_version":manifest.asset_version,"files":plans});
    if pending.is_none() {
        let written = atomic_write(&receipt_path, &json_bytes(&receipt)?, false)?;
        if written.durability != "durable" {
            return Err(Error::new(
                "durability",
                "Template recovery receipt has uncertain durability",
            )
            .detail(json!({"receipt":receipt_path})));
        }
    }
    let mut uncertain = false;
    let mut effects = Vec::new();
    for plan in &plans {
        let destination = plan["destination"].as_str().unwrap();
        let path = project.root.join(destination);
        no_symlinks(&project.root, Path::new(destination))?;
        if file_state(&path)? != plan["before"] {
            return Err(Error::new("conflict", "Template destination changed after planning; completed files remain, retry with this template").detail(json!({"path":path,"receipt":receipt_path})));
        }
        if plan["status"] != "unchanged" {
            let written = atomic_write(&path, &assets[destination], !plan["before"].is_null())
                .map_err(|e| {
                    Error::new("incomplete", e.to_string())
                        .detail(json!({"receipt":receipt_path,"path":path}))
                })?;
            uncertain |= written.durability != "durable";
            set_mode(
                &path,
                plan["desired"]["executable"].as_bool().unwrap_or(false),
            )
            .map_err(|e| {
                Error::new("incomplete", e.to_string())
                    .detail(json!({"receipt":receipt_path,"path":path}))
            })?;
        }
        if pending.is_some() {
            fs::File::open(&path)?.sync_all()?;
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
        }
        inventory["files"][destination] = plan["desired"].clone();
        effects
            .push(json!({"path":path,"status":plan["status"],"sha256":plan["desired"]["sha256"]}));
    }
    inventory["asset_version"] = json!(manifest.asset_version);
    inventory["manifest_sha256"] = json!(manifest_digest);
    inventory["installed_at"] = json!(now());
    let written = atomic_write(
        &inventory_path,
        &json_bytes(&inventory)?,
        inventory_path.exists(),
    )
    .map_err(|e| {
        Error::new("incomplete", e.to_string())
            .detail(json!({"receipt":receipt_path,"inventory":inventory_path}))
    })?;
    uncertain |= written.durability != "durable";
    let mut cleanup_warning = None;
    if !uncertain {
        match fs::remove_file(&receipt_path) {
            Ok(()) => {
                if let Some(parent) = receipt_path.parent() {
                    if let Err(error) = sync_directory(parent) {
                        cleanup_warning = Some(error.to_string());
                    }
                }
            }
            Err(error) => cleanup_warning = Some(error.to_string()),
        }
    }
    Ok(
        json!({"committed":!uncertain,"published":true,"durability":if uncertain{"uncertain"}else{"durable"},"asset_version":manifest.asset_version,"inventory":inventory_path,"files":effects,
        "receipt":if receipt_path.exists(){json!(receipt_path)}else{Value::Null},"cleanup_warning":cleanup_warning}),
    )
}
