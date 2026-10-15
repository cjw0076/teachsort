//! Local filename learning and recoverable file moves for a trusted user folder.
//! Filesystem checks do not form a sandbox against hostile concurrent writers.
#![forbid(unsafe_code)]
mod model;
pub use model::{Labels, Prediction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::{
    ffi::OsStrExt,
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const STATE: &str = ".teachsort";
fn fail<T>(message: &str) -> Result<T> {
    Err(message.to_string().into())
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn stamp() -> Result<String> {
    Ok(format!(
        "{:x}-{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        std::process::id()
    ))
}
pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with([' ', '.'])
    {
        return fail("expected a non-hidden plain filename");
    }
    Ok(())
}
pub(crate) fn validate_destination(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.contains(['\\', ':'])
        || value.chars().any(char::is_control)
    {
        return fail("invalid destination");
    }
    let mut count = 0;
    for part in Path::new(value).components() {
        match part {
            Component::Normal(p) => {
                let p = p.to_str().ok_or("UTF-8 destination required")?;
                if p.starts_with('.') || p.ends_with([' ', '.']) {
                    return fail("hidden or special destination denied");
                }
            }
            _ => return fail("destination must stay inside the selected root"),
        }
        count += 1;
    }
    if count == 0 || count > 8 {
        return fail("destination needs 1 to 8 normal path components");
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 80 || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return fail("invalid plan/run ID");
    }
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(()),
        Ok(_) => fail("directory is a symlink or another file type"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(path)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return fail("regular non-symlink file required");
    }
    Ok(m)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    if regular(path)?.len() > 4 * 1024 * 1024 {
        return fail("state file is too large");
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write_json(path: &Path, value: &impl Serialize, replace: bool) -> Result<()> {
    if replace {
        regular(path)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return fail("state exceeds the 4 MiB limit");
    }
    let tmp = path
        .parent()
        .ok_or("parent")?
        .join(format!(".{}.tmp", stamp()?));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = (|| -> Result<()> {
        let mut file = options.open(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if replace {
            fs::rename(&tmp, path)?;
        } else {
            fs::hard_link(&tmp, path)?;
            fs::remove_file(&tmp)?;
        }
        sync_directory(path.parent().ok_or("parent")?)
    })();
    if tmp.exists() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
}
fn identity(metadata: &fs::Metadata) -> Result<Identity> {
    #[cfg(unix)]
    {
        Ok(Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        fail("v0.1 file moves require Unix file identity")
    }
}
fn fingerprint(path: &Path) -> Result<(String, Identity)> {
    let before = regular(path)?;
    if before.len() > MAX_FILE_BYTES {
        return fail("file exceeds the 64 MiB v0.1 limit");
    }
    let id = identity(&before)?;
    let mut file = File::open(path)?;
    if identity(&file.metadata()?)? != id {
        return fail("file identity changed");
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16384];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_FILE_BYTES {
            return fail("file grew beyond size limit");
        }
        hasher.update(&buffer[..n]);
    }
    if total != before.len() || identity(&regular(path)?)? != id {
        return fail("file changed while reading");
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        id,
    ))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub source: String,
    pub destination: String,
    pub sha256: String,
    pub identity: Identity,
    pub score: f64,
    pub evidence: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub id: String,
    pub root_hash: String,
    pub labels_hash: String,
    pub min_score: f64,
    pub moves: Vec<Move>,
    pub skipped: Vec<Skipped>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub schema: u32,
    pub plan: Plan,
    pub complete: bool,
    pub undone: bool,
}
pub struct Organizer {
    root: PathBuf,
    state: PathBuf,
    root_hash: String,
}
impl Organizer {
    fn base(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return fail("root must be a directory");
        }
        #[cfg(unix)]
        let root_hash = digest(root.as_os_str().as_bytes());
        #[cfg(not(unix))]
        let root_hash = digest(root.to_str().ok_or("UTF-8 root required")?.as_bytes());
        Ok(Self {
            state: root.join(STATE),
            root,
            root_hash,
        })
    }
    pub fn init(root: &Path) -> Result<Self> {
        let app = Self::base(root)?;
        directory(&app.state)?;
        directory(&app.state.join("plans"))?;
        directory(&app.state.join("runs"))?;
        let _lock = app.lock()?;
        if !exists(&app.state.join("labels.json"))? {
            write_json(&app.state.join("labels.json"), &Labels::default(), false)?;
        }
        app.labels()?;
        Ok(app)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let app = Self::base(root)?;
        for dir in [
            &app.state,
            &app.state.join("plans"),
            &app.state.join("runs"),
        ] {
            let m = fs::symlink_metadata(dir).map_err(|_| "run teachsort init first")?;
            if !m.is_dir() || m.file_type().is_symlink() {
                return fail("invalid TeachSort state directory");
            }
        }
        app.labels()?;
        Ok(app)
    }
    fn lock(&self) -> Result<File> {
        let path = self.state.join("lock");
        if exists(&path)? {
            regular(&path)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600);
        let f = options.open(path)?;
        f.try_lock()
            .map_err(|_| "another TeachSort command is active")?;
        Ok(f)
    }
    pub fn labels(&self) -> Result<Labels> {
        let labels: Labels = read_json(&self.state.join("labels.json"))?;
        labels.validate()?;
        Ok(labels)
    }
    pub fn teach(&self, name: &str, destination: &str) -> Result<Labels> {
        validate_name(name)?;
        validate_destination(destination)?;
        let _lock = self.lock()?;
        let mut labels = self.labels()?;
        labels.examples.insert(name.into(), destination.into());
        labels.validate()?;
        write_json(&self.state.join("labels.json"), &labels, true)?;
        Ok(labels)
    }
    fn destination(&self, category: &str, name: &str, create: bool) -> Result<PathBuf> {
        validate_destination(category)?;
        validate_name(name)?;
        let mut parent = self.root.clone();
        for part in Path::new(category).components() {
            parent.push(part);
            if exists(&parent)? {
                let m = fs::symlink_metadata(&parent)?;
                if !m.is_dir() || m.file_type().is_symlink() {
                    return fail("destination contains a symlink or non-directory");
                }
            } else if create {
                directory(&parent)?;
            }
        }
        Ok(parent.join(name))
    }
    pub fn plan(&self, min_score: f64) -> Result<Plan> {
        if !min_score.is_finite() || !(0.5..=0.999).contains(&min_score) {
            return fail("min-score must be between 0.5 and 0.999");
        }
        let _lock = self.lock()?;
        let labels = self.labels()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            names.push(entry?);
            if names.len() > 10000 {
                return fail("v0.1 supports up to 10,000 root entries");
            }
        }
        names.sort_by_key(|e| e.file_name());
        let mut moves = Vec::new();
        let mut skipped = Vec::new();
        for entry in names {
            let name = match entry.file_name().into_string() {
                Ok(s) => s,
                Err(_) => {
                    skipped.push(Skipped {
                        name: "<non-UTF8 filename>".into(),
                        reason: "unsupported filename encoding".into(),
                    });
                    continue;
                }
            };
            if name.starts_with('.') || entry.file_type()?.is_dir() {
                continue;
            }
            let candidate = (|| -> Result<Move> {
                validate_name(&name)?;
                let p = labels
                    .predict(&name)?
                    .ok_or("needs examples or matching evidence")?;
                if p.score < min_score {
                    return fail("model score below threshold; teach another example");
                }
                let destination = self.destination(&p.destination, &name, false)?;
                if exists(&destination)? {
                    return fail("destination already exists");
                }
                let (sha256, identity) = fingerprint(&entry.path())?;
                Ok(Move {
                    source: name.clone(),
                    destination: p.destination,
                    sha256,
                    identity,
                    score: p.score,
                    evidence: p.evidence,
                })
            })();
            match candidate {
                Ok(m) => moves.push(m),
                Err(e) => skipped.push(Skipped {
                    name,
                    reason: e.to_string(),
                }),
            }
        }
        let plan = Plan {
            schema: 1,
            id: stamp()?,
            root_hash: self.root_hash.clone(),
            labels_hash: digest(&serde_json::to_vec(&labels)?),
            min_score,
            moves,
            skipped,
        };
        write_json(
            &self.state.join("plans").join(format!("{}.json", plan.id)),
            &plan,
            false,
        )?;
        Ok(plan)
    }
    fn validate_plan(&self, p: &Plan, id: &str) -> Result<()> {
        validate_id(id)?;
        if p.schema != 1
            || p.id != id
            || p.root_hash != self.root_hash
            || p.moves.len() > 10000
            || p.skipped.len() > 10000
            || !p.min_score.is_finite()
            || !(0.5..=0.999).contains(&p.min_score)
        {
            return fail("invalid plan or root mismatch");
        }
        let mut sources = BTreeSet::new();
        for m in &p.moves {
            validate_name(&m.source)?;
            validate_destination(&m.destination)?;
            if !sources.insert(&m.source)
                || m.sha256.len() != 64
                || !m.sha256.chars().all(|c| c.is_ascii_hexdigit())
                || !m.score.is_finite()
            {
                return fail("invalid or duplicate plan move");
            }
        }
        Ok(())
    }
    fn matches(&self, path: &Path, m: &Move) -> Result<()> {
        let (hash, id) = fingerprint(path)?;
        if hash != m.sha256 || id != m.identity {
            return fail("file changed since the plan; regenerate the plan or inspect the run");
        }
        Ok(())
    }
    pub fn apply(&self, id: &str, confirmed: bool) -> Result<Run> {
        if !confirmed {
            return fail("review the plan, then pass --yes to apply");
        }
        validate_id(id)?;
        let _lock = self.lock()?;
        let plan: Plan = read_json(&self.state.join("plans").join(format!("{id}.json")))?;
        self.validate_plan(&plan, id)?;
        let labels = self.labels()?;
        if digest(&serde_json::to_vec(&labels)?) != plan.labels_hash {
            return fail("examples changed; generate a fresh plan");
        }
        for m in &plan.moves {
            let predicted = labels
                .predict(&m.source)?
                .ok_or("prediction no longer available")?;
            if predicted.destination != m.destination || predicted.score < plan.min_score {
                return fail("plan prediction mismatch");
            }
            self.matches(&self.root.join(&m.source), m)?;
            if exists(&self.destination(&m.destination, &m.source, false)?)? {
                return fail("destination collision; no file moves started");
            }
        }
        let path = self.state.join("runs").join(format!("{id}.json"));
        let mut run = Run {
            schema: 1,
            plan,
            complete: false,
            undone: false,
        };
        // Full recovery intent is durable before the first file mutation.
        write_json(&path, &run, false)?;
        for m in &run.plan.moves {
            let source = self.root.join(&m.source);
            let target = self.destination(&m.destination, &m.source, true)?;
            self.matches(&source, m)?;
            fs::hard_link(&source, &target)?;
            self.matches(&target, m)?;
            self.matches(&source, m)?;
            sync_directory(target.parent().ok_or("parent")?)?;
            fs::remove_file(source)?;
            sync_directory(&self.root)?;
        }
        run.complete = true;
        write_json(&path, &run, true)?;
        Ok(run)
    }
    pub fn undo(&self, id: &str, confirmed: bool) -> Result<Run> {
        if !confirmed {
            return fail("pass --yes to restore this run");
        }
        validate_id(id)?;
        let _lock = self.lock()?;
        let path = self.state.join("runs").join(format!("{id}.json"));
        let mut run: Run = read_json(&path)?;
        if run.schema != 1 {
            return fail("unsupported run schema");
        }
        self.validate_plan(&run.plan, id)?;
        if run.undone {
            return Ok(run);
        }
        // Inspect every move before restoring anything. The journal also supports
        // interruption after hard_link and before removal of the original name.
        for m in &run.plan.moves {
            self.undo_state(m)?;
        }
        for m in run.plan.moves.iter().rev() {
            let source = self.root.join(&m.source);
            let target = self.destination(&m.destination, &m.source, false)?;
            let (has_source, has_target) = self.undo_state(m)?;
            if has_target {
                if !has_source {
                    fs::hard_link(&target, &source)?;
                    self.matches(&source, m)?;
                    sync_directory(&self.root)?;
                }
                fs::remove_file(&target)?;
                sync_directory(target.parent().ok_or("parent")?)?;
            }
        }
        run.undone = true;
        write_json(&path, &run, true)?;
        Ok(run)
    }
    fn undo_state(&self, m: &Move) -> Result<(bool, bool)> {
        let source = self.root.join(&m.source);
        let target = self.destination(&m.destination, &m.source, false)?;
        let a = exists(&source)?;
        let b = exists(&target)?;
        if !a && !b {
            return fail("both source and destination are missing; manual recovery required");
        }
        if a {
            self.matches(&source, m)?;
        }
        if b {
            self.matches(&target, m)?;
        }
        Ok((a, b))
    }
}

pub fn demo(root: &Path) -> Result<Plan> {
    fs::create_dir(root)?;
    for (name, content) in [
        ("invoice_january.csv", "date,total\n2026-01-01,10\n"),
        ("invoice_february.csv", "date,total\n2026-02-01,20\n"),
        ("invoice_march.csv", "date,total\n2026-03-01,30\n"),
        ("notes_alpha.md", "# Alpha\nMeeting notes.\n"),
        ("notes_beta.md", "# Beta\nMeeting notes.\n"),
        ("notes_gamma.md", "# Gamma\nMeeting notes.\n"),
        ("unfamiliar.bin", "Unlabelled demonstration data.\n"),
    ] {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))?;
        f.write_all(content.as_bytes())?;
    }
    let app = Organizer::init(root)?;
    for name in ["invoice_january.csv", "invoice_february.csv"] {
        app.teach(name, "Invoices")?;
    }
    for name in ["notes_alpha.md", "notes_beta.md"] {
        app.teach(name, "Notes")?;
    }
    app.plan(0.85)
}
