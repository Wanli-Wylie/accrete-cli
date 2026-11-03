use accrete::{
    config::Project,
    data,
    error::{Error, Result},
    exchange, index, memory, models, project, results,
    store::{self, Store},
    tools,
};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "accrete",
    version,
    about = "Native object kernel for sustained scientific and software work",
    long_about = "ACCRETE persists Models, Tools, Data, Results and Memory. Read with Git, file tools and SQLite; use this CLI for organizational writes."
)]
struct Cli {
    /// Select project root (otherwise ACCRETE_PROJECT, then nearest ancestor marker).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// Host-local state base; each metadata view receives its own subdirectory.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Explicit host label for location observations.
    #[arg(long, global = true)]
    host: Option<String>,
    /// Emit one JSON response on stdout; diagnostics/errors go to stderr.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize native stores/configuration; optionally install a template manifest.
    Init(Init),
    /// Resolve the Kernel-owned configuration and physical paths.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Register Models, bind local checkouts and manage Git-backed iterations.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Admit, update, expose and retire capabilities without editing their source.
    Tool {
        #[command(subcommand)]
        command: ToolCommand,
    },
    /// Register scientific material, manage locations and materialize explicit replicas.
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },
    /// Persist activity descriptions and observations.
    Result {
        #[command(subcommand)]
        command: ResultCommand,
    },
    /// Publish complete authored Markdown entries.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Maintain the disposable host-local SQLite projection.
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },
    /// Export selected small documents, without scientific payloads or local bindings.
    Export {
        #[arg(long)]
        to: PathBuf,
        #[arg(long)]
        id: Vec<String>,
    },
    /// Import a metadata bundle with per-object conflict handling.
    Import {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        allow_foreign: bool,
    },
    /// Reconcile bounded receipts; never relaunches scientific computation.
    Recover {
        #[arg(long)]
        operation: Option<String>,
    },
    /// Convert the supplied legacy scaffold's metadata, retaining originals and identity maps.
    Migrate {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Explicit project guidance updates; separate from binary and data-format updates.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Print stable versions and integration conventions for template/tool authors.
    Interface,
}
#[derive(Args)]
struct Init {
    #[arg(default_value = ".")]
    directory: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value = "UTC")]
    timezone: String,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    no_git: bool,
    #[arg(long)]
    template: Option<PathBuf>,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Resolve,
}
#[derive(Subcommand)]
enum IndexCommand {
    Refresh,
}
#[derive(Subcommand)]
enum ProjectCommand {
    Upgrade {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Args)]
struct Input {
    /// JSON object file, or '-' for stdin. Unknown extension fields are preserved.
    #[arg(long)]
    from: Option<PathBuf>,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    description: Option<String>,
}
impl Input {
    fn payload(&self) -> Result<Value> {
        let mut v = match &self.from {
            Some(p) => read_json(p)?,
            None => json!({}),
        };
        let map = v
            .as_object_mut()
            .ok_or_else(|| Error::new("invalid", "input must be a JSON object"))?;
        if let Some(id) = &self.id {
            map.insert("id".into(), json!(id));
        }
        if let Some(name) = &self.name {
            map.insert("name".into(), json!(name));
        }
        if let Some(description) = &self.description {
            map.insert("description".into(), json!(description));
        }
        Ok(v)
    }
}
#[derive(Args)]
struct Amend {
    selector: String,
    #[arg(long)]
    from: PathBuf,
    #[arg(long)]
    expect: String,
}
#[derive(Args)]
struct Retire {
    selector: String,
    #[arg(long)]
    expect: String,
}
#[derive(Subcommand)]
enum ModelCommand {
    Register {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        checkout: Option<PathBuf>,
        #[arg(long)]
        source_scope: Vec<String>,
    },
    Bind {
        selector: String,
        #[arg(long)]
        checkout: PathBuf,
    },
    Update(Amend),
    Retire(Retire),
    Iteration {
        #[command(subcommand)]
        command: IterationCommand,
    },
}
#[derive(Subcommand)]
enum IterationCommand {
    Open {
        model: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        worktree: Option<PathBuf>,
        #[arg(long)]
        start: Option<String>,
        /// Attach an existing worktree instead of creating one.
        #[arg(long, requires = "worktree")]
        existing: bool,
    },
    Close {
        selector: String,
        #[arg(long)]
        outcome: String,
        #[arg(long)]
        commit: Option<String>,
        #[arg(long)]
        expect: String,
    },
    /// Explicit native Git worktree removal, separate from iteration closure.
    RemoveWorktree {
        selector: String,
        #[arg(long)]
        force: bool,
    },
}
#[derive(Subcommand)]
enum ToolCommand {
    Admit {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        source: Option<PathBuf>,
        #[arg(long, default_value = "live")]
        source_mode: String,
        #[arg(long)]
        revision: Option<String>,
    },
    Bind {
        selector: String,
        #[arg(long)]
        source: PathBuf,
    },
    Update(Amend),
    Expose {
        selector: String,
        #[arg(long,value_parser=["codex","claude"])]
        agent: String,
        #[arg(long,default_value="wrapper",value_parser=["wrapper","symlink"])]
        mode: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        destination: Option<PathBuf>,
    },
    Refresh {
        selector: String,
    },
    Retire(Retire),
    Export {
        selector: String,
        #[arg(long)]
        to: PathBuf,
    },
}
#[derive(Subcommand)]
enum DataCommand {
    Register {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        hash: bool,
        #[arg(long)]
        result: Option<String>,
        #[arg(long)]
        live: bool,
    },
    Amend(Amend),
    Location {
        #[command(subcommand)]
        command: LocationCommand,
    },
    Materialize {
        selector: String,
        #[arg(long, default_value = "working")]
        to: String,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        expect: Option<String>,
    },
    Evict {
        selector: String,
        #[arg(long)]
        location: String,
        #[arg(long)]
        expect: Option<String>,
    },
    Retire(Retire),
}
#[derive(Subcommand)]
enum LocationCommand {
    Add {
        selector: String,
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(long)]
        location: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        expect: Option<String>,
    },
}
#[derive(Subcommand)]
enum ResultCommand {
    Record {
        #[command(flatten)]
        input: Input,
        /// Label current checkout information as recording-time observation only.
        #[arg(long)]
        observe_checkout: Option<PathBuf>,
    },
    Amend(Amend),
    Observe {
        selector: String,
        #[arg(long)]
        text_file: PathBuf,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Explicit bounded source capture; does not promise a complete runtime snapshot.
    Capture {
        #[arg(long)]
        checkout: PathBuf,
        #[arg(long, required = true)]
        path: Vec<String>,
        #[arg(long)]
        to: PathBuf,
    },
}
#[derive(Subcommand)]
enum MemoryCommand {
    Append {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        authored_at: Option<String>,
        #[arg(long)]
        result: Vec<String>,
        #[arg(long)]
        model: Vec<String>,
        #[arg(long)]
        tool: Vec<String>,
        #[arg(long)]
        data: Vec<String>,
        #[arg(long)]
        supersedes: Vec<String>,
        /// JSON array of additional links, including optional revisions and roles.
        #[arg(long)]
        links_from: Option<PathBuf>,
    },
}

fn read_text(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(fs::read_to_string(path)?)
    }
}
fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&read_text(path)?)?)
}
fn amend(project: &Project, kind: &str, args: &Amend) -> Result<Value> {
    Store::new(project).amend(kind, &args.selector, read_json(&args.from)?, &args.expect)
}
fn retire(project: &Project, kind: &str, args: &Retire) -> Result<Value> {
    Store::new(project).amend(
        kind,
        &args.selector,
        json!({"status":"retired","retired_at":store::now()}),
        &args.expect,
    )
}

fn execute(cli: &Cli) -> Result<Value> {
    if let Command::Init(args) = &cli.command {
        return project::init_with_context(
            &args.directory,
            args.name.as_deref(),
            &args.timezone,
            args.id.as_deref(),
            !args.no_git,
            args.template.as_deref(),
            cli.state_dir.as_deref(),
            cli.host.as_deref(),
        );
    }
    if let Command::Interface = &cli.command {
        return Ok(
            json!({"cli_version":accrete::VERSION,"interface_version":1,"project_formats":[1],"document_formats":[1],"index_format":1,
            "binary":"accrete","project_marker":".accrete/project.toml","local_config":".accrete/local.toml","project_environment":"ACCRETE_PROJECT",
            "template_manifest":"accrete-template.json","template_inventory":".accrete/template-install.json",
            "document_envelope":["format","kind","id","project_id"],"revision":"sha256 of exact published bytes",
            "amendments":"JSON Merge Patch with required --expect; unknown fields retained","stdin":"--from -",
            "output":"--json: one JSON success on stdout; errors on stderr; diagnostics on stderr",
            "exit_codes":{"0":"success, including committed with index_pending","2":"invalid invocation/document","3":"configuration or absent object","4":"revision or destination conflict","5":"missing native dependency","6":"storage failure","7":"incomplete external operation/import","8":"published, durability uncertain"},
            "host_exposures":{"codex":".agents/skills","claude":".claude/skills"},
            "native_reads":["Git","JSON/Markdown files","SQLite"],"manual":"docs/cli-reference.md"}),
        );
    }
    let p = Project::discover(
        cli.project.as_deref(),
        cli.state_dir.as_deref(),
        cli.host.as_deref(),
    )?;
    match &cli.command {
        Command::Config {
            command: ConfigCommand::Resolve,
        } => Ok(p.resolved()),
        Command::Model { command } => match command {
            ModelCommand::Register {
                input,
                checkout,
                source_scope,
            } => {
                let mut v = input.payload()?;
                if !source_scope.is_empty() {
                    v["source_scope"] = json!(source_scope);
                }
                models::register(&p, v, checkout.as_deref())
            }
            ModelCommand::Bind { selector, checkout } => models::bind(&p, selector, checkout),
            ModelCommand::Update(args) => amend(&p, "model", args),
            ModelCommand::Retire(args) => retire(&p, "model", args),
            ModelCommand::Iteration { command } => match command {
                IterationCommand::Open {
                    model,
                    id,
                    name,
                    purpose,
                    branch,
                    worktree,
                    start,
                    existing,
                } => models::iteration_open(
                    &p,
                    model,
                    models::OpenOptions {
                        id: id.clone(),
                        name: name.clone(),
                        purpose: purpose.clone(),
                        branch: branch.clone(),
                        worktree: worktree.clone(),
                        start: start.clone(),
                        existing: *existing,
                    },
                ),
                IterationCommand::Close {
                    selector,
                    outcome,
                    commit,
                    expect,
                } => models::iteration_close(&p, selector, outcome, commit.as_deref(), expect),
                IterationCommand::RemoveWorktree { selector, force } => {
                    models::worktree_remove(&p, selector, *force)
                }
            },
        },
        Command::Tool { command } => match command {
            ToolCommand::Admit {
                input,
                source,
                source_mode,
                revision,
            } => tools::admit(
                &p,
                input.payload()?,
                source.as_deref(),
                source_mode,
                revision.as_deref(),
            ),
            ToolCommand::Bind { selector, source } => tools::bind(&p, selector, source),
            ToolCommand::Update(args) => amend(&p, "tool", args),
            ToolCommand::Expose {
                selector,
                agent,
                mode,
                name,
                destination,
            } => tools::expose(
                &p,
                selector,
                agent,
                mode,
                name.as_deref(),
                destination.as_deref(),
            ),
            ToolCommand::Refresh { selector } => tools::refresh(&p, selector),
            ToolCommand::Retire(args) => tools::retire(&p, &args.selector, &args.expect),
            ToolCommand::Export { selector, to } => tools::export(&p, selector, to),
        },
        Command::Data { command } => match command {
            DataCommand::Register {
                input,
                path,
                kind,
                hash,
                result,
                live,
            } => {
                let mut v = input.payload()?;
                if let Some(id) = result {
                    object_field(&mut v, "origin")?.insert("result_id".into(), json!(id));
                }
                if *live {
                    object_field(&mut v, "content")?.insert("mode".into(), json!("live"));
                }
                data::register(&p, v, path.as_deref(), kind.as_deref(), *hash)
            }
            DataCommand::Amend(args) => {
                let patch = read_json(&args.from)?;
                Store::new(&p).update("data",&args.selector,Some(&args.expect),|doc|{
                    if !patch.is_object(){return Err(Error::new("invalid","amendment must be a JSON object"));}
                    let before=doc["content"].clone();let locations=doc.get("locations").cloned();store::merge_patch(doc,&patch);
                    if before["mode"]!="live"&&doc["content"]!=before{return Err(Error::new("invalid","closed Data content needs a new Data ID; metadata/location changes retain the original content"));}
                    if doc.get("locations")!=locations.as_ref(){return Err(Error::new("invalid","use data location add/materialize/evict to change locations"));}Ok(())
                })
            }
            DataCommand::Location {
                command:
                    LocationCommand::Add {
                        selector,
                        from,
                        location,
                        path,
                        kind,
                        expect,
                    },
            } => {
                let mut v = match from {
                    Some(f) => read_json(f)?,
                    None => json!({}),
                };
                if !v.is_object() {
                    return Err(Error::new("invalid", "location input must be an object"));
                }
                if let Some(n) = location {
                    v["name"] = json!(n);
                }
                if let Some(s) = path {
                    v["path"] = json!(s);
                }
                if let Some(k) = kind {
                    v["kind"] = json!(k);
                }
                data::location_add(&p, selector, v, expect.as_deref())
            }
            DataCommand::Materialize {
                selector,
                to,
                source,
                expect,
            } => data::materialize(&p, selector, to, source.as_deref(), expect.as_deref()),
            DataCommand::Evict {
                selector,
                location,
                expect,
            } => data::evict(&p, selector, location, expect.as_deref()),
            DataCommand::Retire(args) => retire(&p, "data", args),
        },
        Command::Result { command } => match command {
            ResultCommand::Record {
                input,
                observe_checkout,
            } => results::record(&p, input.payload()?, observe_checkout.as_deref()),
            ResultCommand::Amend(args) => amend(&p, "result", args),
            ResultCommand::Observe {
                selector,
                text_file,
                operation_id,
            } => results::observe(
                &p,
                selector,
                &read_text(text_file)?,
                operation_id.as_deref(),
            ),
            ResultCommand::Capture { checkout, path, to } => {
                results::capture(&p, checkout, path, to)
            }
        },
        Command::Memory {
            command:
                MemoryCommand::Append {
                    from,
                    id,
                    title,
                    authored_at,
                    result,
                    model,
                    tool,
                    data,
                    supersedes,
                    links_from,
                },
        } => {
            let mut links = if let Some(file) = links_from {
                read_json(file)?
                    .as_array()
                    .ok_or_else(|| Error::new("invalid", "links input must be a JSON array"))?
                    .clone()
            } else {
                Vec::new()
            };
            for (kind, ids) in [
                ("result", result),
                ("model", model),
                ("tool", tool),
                ("data", data),
                ("memory", supersedes),
            ] {
                for id in ids {
                    let mut link = json!({"kind":kind,"id":id});
                    if kind == "memory" {
                        link["relation"] = json!("supersedes");
                    }
                    links.push(link);
                }
            }
            memory::append(
                &p,
                &read_text(from)?,
                memory::AppendOptions {
                    id: id.clone(),
                    authored_at: authored_at.clone(),
                    links,
                    title: title.clone(),
                },
            )
        }
        Command::Index {
            command: IndexCommand::Refresh,
        } => index::refresh(&p),
        Command::Export { to, id } => exchange::export(&p, to, id),
        Command::Import {
            from,
            allow_foreign,
        } => exchange::import(&p, from, *allow_foreign),
        Command::Recover { operation } => {
            let data = data::recover(&p, operation.as_deref())?;
            let models = if operation.is_none() {
                models::recover(&p)?
            } else {
                json!({"skipped":true})
            };
            let publication = store::recover_publications(&p)?;
            Ok(json!({"data":data,"worktrees":models,"publications":publication}))
        }
        Command::Migrate { from, dry_run } => accrete::migration::migrate(&p, from, *dry_run),
        Command::Project {
            command: ProjectCommand::Upgrade { from, dry_run },
        } => project::upgrade(&p, from, *dry_run),
        Command::Init(_) | Command::Interface => unreachable!(),
    }
}

fn object_field<'a>(
    value: &'a mut Value,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    value
        .as_object_mut()
        .unwrap()
        .entry(key)
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::new("invalid", format!("{key} must be an object")))
}
fn contains_uncertain(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            m.get("durability").and_then(Value::as_str) == Some("uncertain")
                || m.values().any(contains_uncertain)
        }
        Value::Array(a) => a.iter().any(contains_uncertain),
        _ => false,
    }
}
fn contains_failed(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            m.get("failed").and_then(Value::as_u64).unwrap_or(0) > 0
                || m.values().any(contains_failed)
        }
        Value::Array(a) => a.iter().any(contains_failed),
        _ => false,
    }
}
fn warnings(v: &Value) {
    if v.get("index_pending") == Some(&Value::Bool(true)) {
        eprintln!(
            "Document {} is committed; index refresh is pending. Run accrete index refresh. {}",
            v["object_id"],
            v.get("index_error").and_then(Value::as_str).unwrap_or("")
        );
    }
    if let Some(message) = v.get("warning").and_then(Value::as_str) {
        eprintln!("warning: {message}");
    }
    match v {
        Value::Object(m) => {
            for v in m.values() {
                warnings(v)
            }
        }
        Value::Array(a) => {
            for v in a {
                warnings(v)
            }
        }
        _ => {}
    }
}
fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let json = std::env::args().any(|a| a == "--json");
            if e.use_stderr() && json {
                eprintln!("{}", Error::new("invalid", e.to_string()).json());
            } else {
                let _ = e.print();
            }
            std::process::exit(e.exit_code());
        }
    };
    match execute(&cli) {
        Ok(mut value) => {
            let exit = if contains_uncertain(&value) {
                8
            } else if contains_failed(&value) {
                7
            } else {
                0
            };
            warnings(&value);
            if value.is_object() {
                value["ok"] = json!(exit == 0);
            }
            if cli.json {
                println!("{}", serde_json::to_string(&value).unwrap());
            } else if let Some(id) = value.get("object_id").and_then(Value::as_str) {
                println!("Object: {id}");
                if let Some(path) = value.get("path").and_then(Value::as_str) {
                    println!("Path: {path}");
                }
                if let Some(revision) = value.get("revision").and_then(Value::as_str) {
                    println!("Revision: {revision}");
                }
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            std::process::exit(exit);
        }
        Err(error) => {
            if cli.json {
                eprintln!("{}", error.json());
            } else {
                eprintln!("{error}");
                if !error.details.is_null() {
                    eprintln!("{}", serde_json::to_string_pretty(&error.details).unwrap());
                }
            }
            std::process::exit(error.exit_code);
        }
    }
}
