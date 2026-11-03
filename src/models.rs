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

pub fn register(project: &Project, mut payload: Value, checkout: Option<&Path>) -> Result<Value> {
    let obj = payload
        .as_object_mut()
        .ok_or_else(|| Error::new("invalid", "Model input must be a JSON object"))?;
    obj.entry("status").or_insert(json!("active"));
    let mut binding = None;
    if let Some(path) = checkout {
        let checkout = canonical_future(&project.resolve(path))?;
        let scopes = obj
            .get("source_scope")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let observed = git::observe(&checkout, &scopes)?;
        // The portable descriptor contains no machine path. Current checkout observations
        // are admission context, never automatically an execution identity.
        let mut portable = observed;
        portable.as_object_mut().unwrap().remove("repository_root");
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            if let Ok(existing) = Store::new(project).read("model", id) {
                if let Some(previous) = existing.doc.get("registration_observation") {
                    portable = previous.clone();
                }
            }
        }
        obj.entry("registration_observation").or_insert(portable);
        binding = Some(checkout);
    } else if !obj.contains_key("repository") {
        return Err(Error::new(
            "invalid",
            "supply --checkout or a repository locator in the Model document",
        ));
    }
    let mut response = Store::new(project).create("model", payload)?;
    if let Some(path) = binding {
        let id = response["object_id"].as_str().unwrap_or("");
        match project.bind("models", id, "checkout", &path.to_string_lossy()) {
            Ok(value) => {
                response["local_binding"] = value;
            }
            Err(e) => {
                response["binding_pending"] = json!(true);
                response["warning"] = json!(e.to_string());
            }
        }
    }
    Ok(response)
}
pub fn checkout(project: &Project, id_or_alias: &str) -> Result<PathBuf> {
    let model = Store::new(project).read("model", id_or_alias)?;
    let id = model.doc["id"].as_str().unwrap();
    let name = model.doc["name"].as_str().unwrap_or(id);
    let path = table_str(&project.config, &["models", id, "checkout"])
        .or_else(|| table_str(&project.config, &["models", name, "checkout"]))
        .or_else(|| table_str(&project.config, &["models", id_or_alias, "checkout"]))
        .or_else(|| {
            model
                .doc
                .get("aliases")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter().filter_map(Value::as_str).find_map(|alias| {
                        table_str(&project.config, &["models", alias, "checkout"])
                    })
                })
        });
    path.map(|p| project.resolve(p)).ok_or_else(|| {
        Error::new(
            "config",
            format!("Model {id} has no checkout on this host; use model bind"),
        )
    })
}
pub fn bind(project: &Project, selector: &str, path: &Path) -> Result<Value> {
    let m = Store::new(project).read("model", selector)?;
    let path = canonical_future(&project.resolve(path))?;
    git::checked(&path, ["rev-parse", "--git-dir"])?;
    project.bind(
        "models",
        m.doc["id"].as_str().unwrap(),
        "checkout",
        &path.to_string_lossy(),
    )
}

#[derive(Default)]
pub struct OpenOptions {
    pub id: Option<String>,
    pub name: Option<String>,
    pub purpose: Option<String>,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub start: Option<String>,
    pub existing: bool,
}
pub fn iteration_open(project: &Project, model: &str, options: OpenOptions) -> Result<Value> {
    let store = Store::new(project);
    let model = store.read("model", model)?;
    let model_id = model.doc["id"].as_str().unwrap();
    let checkout = checkout(project, model_id)?;
    let start = git::resolve_commit(&checkout, options.start.as_deref().unwrap_or("HEAD"))?;
    let id = options
        .id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    store::valid_id(&id)?;
    let mut doc = json!({"id":id,"model_id":model_id,"name":options.name,"purpose":options.purpose,"starting_revision":start,"status":"open"});
    if let Some(branch) = &options.branch {
        git::checked(&checkout, ["check-ref-format", "--branch", branch])?;
        doc["branch"] = json!(branch);
    }
    if let Some(worktree) = options.worktree {
        let worktree = canonical_future(&project.resolve(worktree))?;
        if options.existing {
            let base = git::checked(
                &checkout,
                ["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?;
            let other = git::checked(
                &worktree,
                ["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )?;
            if canonical_future(Path::new(&base))? != canonical_future(Path::new(&other))? {
                return Err(Error::new(
                    "conflict",
                    "the existing worktree belongs to another Git repository",
                ));
            }
            doc["starting_revision"] = json!(git::head(&worktree)?);
            let observed = git::observe(&worktree, &[])?;
            doc["branch"] = observed["branch"].clone();
            let mut response = store.create("iteration", doc)?;
            response["local_binding"] =
                project.bind("iterations", &id, "worktree", &worktree.to_string_lossy())?;
            return Ok(response);
        }
        if worktree.exists() {
            return Err(Error::new(
                "conflict",
                format!(
                    "worktree destination already exists: {}",
                    worktree.display()
                ),
            ));
        }
        let receipt = project
            .state_dir
            .join("operations")
            .join(format!("worktree-{id}.json"));
        let mut state = json!({"operation":"worktree","id":id,"model_id":model_id,"repository":checkout,"path":worktree,"document":doc,"step":"prepared"});
        store::atomic_write(&receipt, &serde_json::to_vec_pretty(&state)?, false)?;
        let mut args = vec!["worktree".to_owned(), "add".into()];
        if let Some(branch) = options.branch {
            args.extend(["-b".into(), branch]);
        } else {
            args.push("--detach".into());
        }
        args.extend(["--".into(), worktree.to_string_lossy().into_owned(), start]);
        if let Err(e) = git::checked(&checkout, &args) {
            return Err(Error::new("incomplete", e.to_string())
                .detail(json!({"receipt":receipt,"path":worktree})));
        }
        state["step"] = json!("worktree_created");
        store::atomic_write(&receipt, &serde_json::to_vec_pretty(&state)?, true)?;
        let mut response = store
            .create("iteration", state["document"].clone())
            .map_err(|e| {
                Error::new("incomplete", e.to_string())
                    .detail(json!({"receipt":receipt,"worktree":worktree}))
            })?;
        response["local_binding"] = project
            .bind("iterations", &id, "worktree", &worktree.to_string_lossy())
            .map_err(|e| {
                Error::new("incomplete", e.to_string())
                    .detail(json!({"receipt":receipt,"committed":true}))
            })?;
        if let Err(e) = fs::remove_file(&receipt) {
            response["recovery_pending"] = json!(true);
            response["warning"] = json!(format!(
                "iteration and binding committed; receipt cleanup pending: {e}"
            ));
            response["receipt"] = json!(receipt);
        }
        response["worktree"] = json!(worktree);
        Ok(response)
    } else {
        store.create("iteration", doc)
    }
}
pub fn iteration_close(
    project: &Project,
    selector: &str,
    outcome: &str,
    commit: Option<&str>,
    expect: &str,
) -> Result<Value> {
    let mut patch = json!({"status":"closed","outcome":outcome,"closed_at":store::now()});
    if let Some(commit) = commit {
        patch["retained_revision"] = json!(commit);
    }
    let mut response = Store::new(project).amend("iteration", selector, patch, expect)?;
    if let Some(commit) = commit {
        let d = Store::new(project).read("iteration", selector)?;
        let retention = checkout(project, d.doc["model_id"].as_str().unwrap_or(""))
            .and_then(|path| git::retain(&path, d.doc["id"].as_str().unwrap(), commit));
        match retention {
            Ok(reference) => response["retention_ref"] = json!(reference),
            Err(e) => response["retention_warning"] = json!(e.to_string()),
        }
    }
    Ok(response)
}
pub fn worktree_remove(project: &Project, selector: &str, force: bool) -> Result<Value> {
    let iteration = Store::new(project).read("iteration", selector)?;
    let id = iteration.doc["id"].as_str().unwrap();
    let path = table_str(&project.config, &["iterations", id, "worktree"])
        .ok_or_else(|| Error::new("config", "no local worktree binding for this iteration"))?;
    let repository = checkout(project, iteration.doc["model_id"].as_str().unwrap_or(""))?;
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    git::checked(&repository, args)?;
    Ok(json!({"committed":true,"object_id":id,"removed_worktree":path}))
}
pub fn recover(project: &Project) -> Result<Value> {
    let root = project.state_dir.join("operations");
    let mut outcomes = Vec::new();
    if !root.exists() {
        return Ok(json!({"operations":outcomes}));
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .starts_with("worktree-")
        {
            continue;
        }
        let state: Value = serde_json::from_slice(&fs::read(&path)?)?;
        let worktree = Path::new(
            state["path"]
                .as_str()
                .ok_or_else(|| Error::new("invalid", "invalid worktree receipt"))?,
        );
        if !worktree.is_dir() {
            outcomes.push(json!({"receipt":path,"pending":true,"reason":"worktree absent; no Git operation retried"}));
            continue;
        }
        let repository = Path::new(
            state["repository"]
                .as_str()
                .ok_or_else(|| Error::new("invalid", "receipt missing repository"))?,
        );
        let expected = git::checked(
            repository,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let actual = git::checked(
            worktree,
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        if canonical_future(Path::new(&expected))? != canonical_future(Path::new(&actual))? {
            return Err(Error::new(
                "conflict",
                "recovery path is no longer the requested repository",
            ));
        }
        let mut result = Store::new(project).create("iteration", state["document"].clone())?;
        result["local_binding"] = project.bind(
            "iterations",
            state["id"].as_str().unwrap(),
            "worktree",
            &worktree.to_string_lossy(),
        )?;
        if let Err(e) = fs::remove_file(&path) {
            result["recovery_pending"] = json!(true);
            result["warning"] = json!(format!("iteration committed; receipt cleanup pending: {e}"));
        }
        outcomes.push(result);
    }
    Ok(json!({"operations":outcomes}))
}
