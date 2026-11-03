use crate::{
    config::Project,
    error::{Error, Result},
    git, models,
    store::{self, Store},
};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn record(
    project: &Project,
    mut payload: Value,
    observe_checkout: Option<&Path>,
) -> Result<Value> {
    if !payload.is_object() {
        return Err(Error::new("invalid", "Result input must be a JSON object"));
    }
    if payload.get("observations").is_some_and(|v| !v.is_array()) {
        return Err(Error::new("invalid", "observations must be an array"));
    }
    if let Some(path) = observe_checkout {
        // Deliberately separate from execution-time model references.
        let existing = payload
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Store::new(project).read("result", id).ok());
        payload["recording_observation"] =
            match existing.and_then(|e| e.doc.get("recording_observation").cloned()) {
                Some(previous) => previous,
                None => git::observe(&project.resolve(path), &[])?,
            };
    }
    let mut response = Store::new(project).create("result", payload.clone())?;
    let mut retained = Vec::new();
    let mut warnings = Vec::new();
    if let Some(references) = payload["models"].as_array() {
        for r in references {
            if let (Some(id), Some(commit)) = (
                r.get("model_id")
                    .or_else(|| r.get("id"))
                    .and_then(Value::as_str),
                r.get("commit")
                    .or_else(|| r.get("commit_id"))
                    .and_then(Value::as_str),
            ) {
                match models::checkout(project, id)
                    .and_then(|p| git::retain(&p, response["object_id"].as_str().unwrap(), commit))
                {
                    Ok(reference) => retained.push(json!({"model_id":id,"ref":reference})),
                    Err(e) => warnings.push(json!({"model_id":id,"message":e.to_string()})),
                }
            }
        }
    }
    if !retained.is_empty() {
        response["retained_refs"] = json!(retained);
    }
    if !warnings.is_empty() {
        response["retention_warnings"] = json!(warnings);
    }
    Ok(response)
}
pub fn observe(
    project: &Project,
    selector: &str,
    text: &str,
    operation_id: Option<&str>,
) -> Result<Value> {
    if text.trim().is_empty() {
        return Err(Error::new("invalid", "observation must contain text"));
    }
    let op = operation_id
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Store::new(project).update("result", selector, None, |doc| {
        let observations = doc
            .as_object_mut()
            .unwrap()
            .entry("observations")
            .or_insert(json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                Error::new(
                    "invalid",
                    "observations must be an array to append an observation",
                )
            })?;
        if let Some(old) = observations.iter().find(|v| v["operation_id"] == op) {
            if old["text"] == text {
                return Ok(());
            }
            return Err(Error::new(
                "conflict",
                "observation operation ID already has different text",
            ));
        }
        observations.push(json!({"operation_id":op,"text":text,"observed_at":store::now()}));
        Ok(())
    })
}
pub fn capture(
    project: &Project,
    checkout: &Path,
    paths: &[String],
    destination: &Path,
) -> Result<Value> {
    if paths.is_empty() {
        return Err(Error::new(
            "invalid",
            "select at least one source path with --path",
        ));
    }
    let checkout = project.resolve(checkout);
    let destination = project.resolve(destination);
    if destination.exists() {
        return Err(Error::new("conflict", "capture destination already exists"));
    }
    fs::create_dir_all(&destination)?;
    let mut args = vec![
        "diff".to_string(),
        "--binary".into(),
        "HEAD".into(),
        "--".into(),
    ];
    args.extend_from_slice(paths);
    let diff = git::run(&checkout, &args)?;
    if !diff.status.success() {
        return Err(Error::new(
            "external",
            String::from_utf8_lossy(&diff.stderr).into_owned(),
        ));
    }
    fs::write(destination.join("tracked.patch"), diff.stdout)?;
    let mut list = vec![
        "ls-files".to_string(),
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
    ];
    list.extend_from_slice(paths);
    let files = git::run(&checkout, &list)?;
    if !files.status.success() {
        return Err(Error::new("external", "git ls-files failed"));
    }
    let mut copied = Vec::new();
    for name in files.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name = std::str::from_utf8(name)
            .map_err(|_| Error::new("invalid", "capture requires UTF-8 paths"))?;
        let source = checkout.join(name);
        if fs::symlink_metadata(&source)?.file_type().is_symlink() {
            return Err(Error::new(
                "invalid",
                "selected untracked symlinks require a domain-owned source bundle",
            ));
        }
        let dest = destination.join("untracked").join(name);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(&source, &dest)?;
        copied.push(name.to_owned());
    }
    let manifest = json!({"format":1,"recording_observation":git::observe(&checkout,&[])?,"selected_paths":paths,"untracked_files":copied,
        "scope":"selected tracked binary diff and nonignored untracked regular files; ignored files, external dependencies and runtime state are omitted"});
    fs::write(
        destination.join("capture.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(json!({"path":destination,"capture":manifest}))
}
