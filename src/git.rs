use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output},
};

pub fn run<I, S>(cwd: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    "dependency",
                    "Git is required; install git and put it on PATH",
                )
            } else {
                e.into()
            }
        })
}
pub fn checked<I, S>(cwd: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = run(cwd, args)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::new(
            "external",
            format!(
                "git failed in {}: {}",
                cwd.display(),
                stderr.chars().take(8192).collect::<String>().trim()
            ),
        ));
    }
    String::from_utf8(out.stdout)
        .map(|s| s.trim_end().to_owned())
        .map_err(|_| Error::new("external", "Git returned non-UTF-8 output"))
}
pub fn head(cwd: &Path) -> Result<String> {
    checked(cwd, ["rev-parse", "--verify", "HEAD^{commit}"])
}
pub fn resolve_commit(cwd: &Path, revision: &str) -> Result<String> {
    if revision.is_empty() || revision.starts_with('-') {
        return Err(Error::new("invalid", "invalid Git revision"));
    }
    checked(
        cwd,
        [
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ],
    )
}
pub fn observe(cwd: &Path, scopes: &[String]) -> Result<Value> {
    let repository = checked(cwd, ["rev-parse", "--show-toplevel"])?;
    let commit = head(cwd).ok();
    let status = checked(
        cwd,
        ["status", "--porcelain=v1", "--untracked-files=normal"],
    )?;
    let branch = checked(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    let mut trees = serde_json::Map::new();
    for scope in scopes {
        if let Some(c) = &commit {
            if let Ok(tree) = checked(
                cwd,
                [
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{c}:{scope}"),
                ],
            ) {
                trees.insert(scope.clone(), json!(tree));
            }
        }
    }
    Ok(
        json!({"repository_root":repository,"commit":commit,"branch":branch,"dirty":!status.is_empty(),"observed_at":crate::store::now(),"source_trees":trees}),
    )
}
pub fn retain(cwd: &Path, id: &str, commit: &str) -> Result<String> {
    let resolved = resolve_commit(cwd, commit)?;
    let reference = format!("refs/accrete/retained/{id}/{resolved}");
    checked(cwd, ["update-ref", &reference, &resolved])?;
    Ok(reference)
}
