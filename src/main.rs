use std::{collections::BTreeMap, path::Path};
use teachsort::{Organizer, Result};
const HELP:&str = "TeachSort 0.1.0 - learn folder choices from filename examples\n\nUSAGE\n  teachsort init --root DIR\n  teachsort teach --root DIR --name FILENAME --to FOLDER\n  teachsort plan --root DIR [--min-score 0.85]\n  teachsort apply --root DIR --plan ID --yes\n  teachsort undo --root DIR --run ID --yes\n  teachsort status --root DIR\n  teachsort demo --root NEW_DIRECTORY\n\nClassification reads filenames. Apply hashes matching files and moves them locally.\nReview the JSON plan before applying. v0.1 supports Unix filesystems with hard links.\n";
fn run() -> Result<()> {
    let mut args = std::env::args_os()
        .skip(1)
        .map(|s| s.into_string().map_err(|_| "UTF-8 arguments required"));
    let command = match args.next().transpose()? {
        Some(c) => c,
        None => {
            print!("{HELP}");
            return Ok(());
        }
    };
    if ["--help", "-h", "help"].contains(&command.as_str()) {
        print!("{HELP}");
        return Ok(());
    }
    if command == "--version" {
        println!("teachsort {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut values = BTreeMap::new();
    let mut confirmed = false;
    while let Some(key) = args.next().transpose()? {
        if key == "--yes" {
            if confirmed {
                return Err("duplicate --yes".into());
            }
            confirmed = true;
            continue;
        }
        if !["--root", "--name", "--to", "--min-score", "--plan", "--run"].contains(&key.as_str()) {
            return Err(format!("unknown option: {key}").into());
        }
        let value = args.next().transpose()?.ok_or("option needs a value")?;
        if values.insert(key, value).is_some() {
            return Err("duplicate option".into());
        }
    }
    let root = values.remove("--root").ok_or("--root DIR is required")?;
    let allowed: &[&str] = match command.as_str() {
        "init" | "status" | "demo" => &[],
        "teach" => &["--name", "--to"],
        "plan" => &["--min-score"],
        "apply" => &["--plan"],
        "undo" => &["--run"],
        _ => return Err(format!("unknown command: {command}").into()),
    };
    if values.keys().any(|key| !allowed.contains(&key.as_str()))
        || (confirmed && !["apply", "undo"].contains(&command.as_str()))
    {
        return Err("option does not apply to this command".into());
    }
    let value = match command.as_str() {
        "init" => {
            let app = Organizer::init(Path::new(&root))?;
            serde_json::to_value(app.labels()?)?
        }
        "teach" => {
            let name = values.remove("--name").ok_or("--name required")?;
            let to = values.remove("--to").ok_or("--to required")?;
            serde_json::to_value(Organizer::open(Path::new(&root))?.teach(&name, &to)?)?
        }
        "plan" => {
            let score: f64 = values
                .remove("--min-score")
                .unwrap_or_else(|| "0.85".into())
                .parse()?;
            serde_json::to_value(Organizer::open(Path::new(&root))?.plan(score)?)?
        }
        "apply" => {
            let id = values.remove("--plan").ok_or("--plan required")?;
            serde_json::to_value(Organizer::open(Path::new(&root))?.apply(&id, confirmed)?)?
        }
        "undo" => {
            let id = values.remove("--run").ok_or("--run required")?;
            serde_json::to_value(Organizer::open(Path::new(&root))?.undo(&id, confirmed)?)?
        }
        "status" => serde_json::to_value(Organizer::open(Path::new(&root))?.labels()?)?,
        "demo" => serde_json::to_value(teachsort::demo(Path::new(&root))?)?,
        _ => return Err(format!("unknown command: {command}").into()),
    };
    if !values.is_empty() {
        return Err("option does not apply to this command".into());
    }
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("teachsort: {error}");
        std::process::exit(1);
    }
}
