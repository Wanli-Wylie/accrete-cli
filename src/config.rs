use crate::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

const NO_WORKSPACE_HINT: &str = "create a Workspace with `npx accrete-kit@latest create`, or select one with --project or ACCRETE_PROJECT";

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub metadata: PathBuf,
    pub results: PathBuf,
    pub memory: PathBuf,
    pub tool_sources: PathBuf,
    pub scratch: PathBuf,
    pub state_dir: PathBuf,
    pub index_path: PathBuf,
    pub project_id: String,
    pub timezone: String,
    pub host: String,
    pub view_key: String,
    pub config: toml::Value,
}

pub fn merge(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(a), toml::Value::Table(b)) => {
            for (key, value) in b {
                if let Some(old) = a.get_mut(&key) {
                    merge(old, value);
                } else {
                    a.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

/// Resolve existing ancestors through symlinks, even before a store exists.
pub fn canonical_future(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    // Existing paths must use the filesystem's own symlink/parent semantics.
    if let Ok(canonical) = fs::canonicalize(&absolute) {
        return Ok(canonical);
    }
    let mut normalized = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            x => normalized.push(x),
        }
    }
    let mut ancestor = normalized.clone();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| Error::new("config", "cannot resolve path"))?
            .to_os_string();
        tail.push(name);
        if !ancestor.pop() {
            return Err(Error::new("config", "no existing path ancestor"));
        }
    }
    let mut canonical = fs::canonicalize(ancestor)?;
    for part in tail.into_iter().rev() {
        canonical.push(part);
    }
    Ok(canonical)
}

fn defaults() -> toml::Value {
    toml::from_str(
        r#"
format = 1
timezone = "UTC"
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
    )
    .expect("built-in configuration")
}

pub fn table_str<'a>(config: &'a toml::Value, keys: &[&str]) -> Option<&'a str> {
    let mut v = config;
    for key in keys {
        v = v.get(*key)?;
    }
    v.as_str()
}

impl Project {
    pub fn discover(
        explicit: Option<&Path>,
        state_override: Option<&Path>,
        host_override: Option<&str>,
    ) -> Result<Self> {
        let selected = explicit
            .map(PathBuf::from)
            .or_else(|| env::var_os("ACCRETE_PROJECT").map(PathBuf::from));
        let root = if let Some(path) = selected {
            let p = canonical_future(&path)?;
            if !p.join(".accrete/project.toml").is_file() {
                return Err(Error::new(
                    "project",
                    format!(
                        "no .accrete/project.toml in {}; {NO_WORKSPACE_HINT}",
                        p.display()
                    ),
                ));
            }
            p
        } else {
            let cwd = fs::canonicalize(env::current_dir()?)?;
            cwd.ancestors()
                .find(|p| p.join(".accrete/project.toml").is_file())
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    Error::new(
                        "project",
                        format!(
                            "no ACCRETE Workspace at or above {}; {NO_WORKSPACE_HINT}",
                            cwd.display()
                        ),
                    )
                })?
        };
        let mut config = defaults();
        let shared: toml::Value =
            toml::from_str(&fs::read_to_string(root.join(".accrete/project.toml"))?)?;
        if shared.get("format").and_then(toml::Value::as_integer) != Some(1) {
            return Err(Error::new(
                "unsupported_format",
                "this binary supports project format 1 only",
            ));
        }
        let project_id = shared
            .get("project_id")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| Error::new("config", "project_id missing"))?
            .to_string();
        // The marker is written outside the Kernel, so its identity is checked here.
        crate::store::valid_id(&project_id)
            .map_err(|e| Error::new("config", format!("project_id: {}", e.message)))?;
        merge(&mut config, shared);
        let local_path = root.join(".accrete/local.toml");
        if local_path.exists() {
            let local: toml::Value = toml::from_str(&fs::read_to_string(local_path)?)?;
            if local.get("project_id").is_some() || local.get("format").is_some() {
                return Err(Error::new(
                    "config",
                    "local.toml cannot change project_id or format",
                ));
            }
            merge(&mut config, local);
        }
        let timezone = table_str(&config, &["timezone"])
            .unwrap_or("UTC")
            .to_owned();
        timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| Error::new("config", format!("unknown IANA timezone {timezone}")))?;
        let host = host_override
            .map(str::to_owned)
            .or_else(|| env::var("ACCRETE_HOST").ok())
            .or_else(|| table_str(&config, &["host"]).map(str::to_owned))
            .unwrap_or_else(|| {
                env::var("HOSTNAME")
                    .or_else(|_| fs::read_to_string("/etc/hostname").map(|s| s.trim().to_owned()))
                    .unwrap_or_else(|_| "local".into())
            });
        let path = |key: &str| -> Result<PathBuf> {
            let s = table_str(&config, &["paths", key])
                .ok_or_else(|| Error::new("config", format!("missing paths.{key}")))?;
            canonical_future(&root.join(s))
        };
        let metadata = path("metadata")?;
        let results = path("results")?;
        let memory = path("memory")?;
        let tool_sources = path("tool_sources")?;
        let scratch = path("scratch")?;
        let roots = [&metadata, &results, &memory];
        for (i, a) in roots.iter().enumerate() {
            for b in roots.iter().skip(i + 1) {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(Error::new(
                        "config",
                        "managed document roots must not overlap",
                    ));
                }
            }
        }
        for p in roots {
            if p == &root
                || root.starts_with(p)
                || scratch.starts_with(p)
                || p.starts_with(&scratch)
            {
                return Err(Error::new(
                    "config",
                    "managed roots must be separate from project and scratch roots",
                ));
            }
        }
        let state_base = if let Some(p) = state_override {
            root.join(p)
        } else if let Some(p) = env::var_os("ACCRETE_STATE_DIR") {
            root.join(p)
        } else if let Some(p) = table_str(&config, &["local_state", "base"]) {
            root.join(p)
        } else if let Some(p) = env::var_os("XDG_STATE_HOME") {
            PathBuf::from(p).join("accrete")
        } else if let Some(p) = env::var_os("HOME") {
            PathBuf::from(p).join(".local/state/accrete")
        } else {
            return Err(Error::new(
                "config",
                "set local_state.base or ACCRETE_STATE_DIR",
            ));
        };
        let state_base = canonical_future(&state_base)?;
        for p in [&metadata, &results, &memory] {
            if state_base.starts_with(p) || p.starts_with(&state_base) {
                return Err(Error::new(
                    "config",
                    "local state and managed roots must not overlap",
                ));
            }
        }
        let binding = json!({"project_id":project_id,"roots":[metadata,results,memory]});
        let view_key = format!("{:x}", Sha256::digest(serde_json::to_vec(&binding)?));
        let state_dir = state_base.join(&view_key);
        let index_path = state_dir.join("index.sqlite");
        Ok(Self {
            root,
            metadata,
            results,
            memory,
            tool_sources,
            scratch,
            state_dir,
            index_path,
            project_id,
            timezone,
            host,
            view_key,
            config,
        })
    }

    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        self.root.join(path)
    }
    pub fn resolved(&self) -> Value {
        json!({"project_root":self.root,"project_id":self.project_id,"format":1,"timezone":self.timezone,"host":self.host,
            "paths":{"metadata":self.metadata,"results":self.results,"memory":self.memory,"tool_sources":self.tool_sources,"scratch":self.scratch},
            "local_state":{"directory":self.state_dir,"index":self.index_path,"view_key":self.view_key},"configuration":self.config})
    }

    pub fn bind(&self, section: &str, id: &str, key: &str, value: &str) -> Result<Value> {
        let store = crate::store::Store::new(self);
        let _lock = store.lock()?;
        let p = self.root.join(".accrete/local.toml");
        let mut v: toml::Value = if p.exists() {
            toml::from_str(&fs::read_to_string(&p)?)?
        } else {
            toml::Value::Table(Default::default())
        };
        let table = v
            .as_table_mut()
            .ok_or_else(|| Error::new("config", "local.toml must be a table"))?;
        let section_table = table
            .entry(section)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| Error::new("config", "binding section must be a table"))?;
        let object = section_table
            .entry(id)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| Error::new("config", "binding must be a table"))?;
        object.insert(key.into(), toml::Value::String(value.into()));
        let publication =
            crate::store::atomic_write(&p, toml::to_string_pretty(&v)?.as_bytes(), true)?;
        Ok(
            json!({"committed":true,"path":p,"binding":{ "section":section,"id":id,"key":key,"value":value},"durability":publication.durability}),
        )
    }
}
