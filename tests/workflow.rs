#![cfg(unix)]
use std::os::unix::fs::{symlink, DirBuilderExt};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use teachsort::{demo, Organizer, Plan, Run};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "teachsort-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("inbox")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot(root: &Path, plan: &Plan) -> Vec<(String, Vec<u8>)> {
    plan.moves
        .iter()
        .map(|m| (m.source.clone(), fs::read(root.join(&m.source)).unwrap()))
        .collect()
}
#[test]
fn demo_apply_and_undo_preserve_every_byte() {
    let t = Temp::new();
    let root = t.root();
    let plan = demo(&root).unwrap();
    assert_eq!(plan.moves.len(), 6);
    assert!(plan.skipped.iter().any(|s| s.name == "unfamiliar.bin"));
    let original = snapshot(&root, &plan);
    let app = Organizer::open(&root).unwrap();
    assert!(app.apply(&plan.id, false).is_err());
    let run = app.apply(&plan.id, true).unwrap();
    assert!(run.complete);
    for m in &plan.moves {
        assert!(!root.join(&m.source).exists());
        assert!(root.join(&m.destination).join(&m.source).is_file());
    }
    assert!(app.apply(&plan.id, true).is_err());
    assert!(app.undo(&plan.id, true).unwrap().undone);
    assert!(app.undo(&plan.id, true).unwrap().undone);
    for (name, bytes) in original {
        assert_eq!(fs::read(root.join(name)).unwrap(), bytes);
    }
}
#[test]
fn changed_source_stops_the_entire_preflight() {
    let t = Temp::new();
    let root = t.root();
    let plan = demo(&root).unwrap();
    fs::write(root.join(&plan.moves[0].source), "changed").unwrap();
    assert!(Organizer::open(&root)
        .unwrap()
        .apply(&plan.id, true)
        .is_err());
    assert!(plan.moves.iter().all(|m| root.join(&m.source).exists()));
    assert!(!root.join("Invoices").exists());
}
#[test]
fn destination_collision_never_overwrites() {
    let t = Temp::new();
    let root = t.root();
    let plan = demo(&root).unwrap();
    let m = &plan.moves[0];
    fs::create_dir(root.join(&m.destination)).unwrap();
    let target = root.join(&m.destination).join(&m.source);
    fs::write(&target, "keep me").unwrap();
    assert!(Organizer::open(&root)
        .unwrap()
        .apply(&plan.id, true)
        .is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep me");
    assert!(plan.moves.iter().all(|m| root.join(&m.source).exists()));
}
#[test]
fn undo_refuses_a_new_source_file_before_restoring_other_files() {
    let t = Temp::new();
    let root = t.root();
    let plan = demo(&root).unwrap();
    let app = Organizer::open(&root).unwrap();
    app.apply(&plan.id, true).unwrap();
    fs::write(root.join(&plan.moves[0].source), "new work").unwrap();
    assert!(app.undo(&plan.id, true).is_err());
    assert_eq!(
        fs::read_to_string(root.join(&plan.moves[0].source)).unwrap(),
        "new work"
    );
    assert!(plan
        .moves
        .iter()
        .all(|m| root.join(&m.destination).join(&m.source).exists()));
}
#[test]
fn traversal_and_changed_labels_are_rejected() {
    let t = Temp::new();
    let root = t.root();
    let plan = demo(&root).unwrap();
    let app = Organizer::open(&root).unwrap();
    for destination in [
        "../outside",
        "/tmp/outside",
        "Invoices/../../outside",
        ".teachsort/plans",
    ] {
        assert!(app.teach("invoice.csv", destination).is_err());
    }
    assert!(app.teach("../secret.csv", "Invoices").is_err());
    assert!(app.apply("../../escape", true).is_err());
    app.teach("another.csv", "Invoices").unwrap();
    assert!(app.apply(&plan.id, true).is_err());
}
#[test]
fn source_and_destination_symlinks_are_excluded() {
    let t = Temp::new();
    let root = t.root();
    demo(&root).unwrap();
    let outside = t.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), "private").unwrap();
    symlink(outside.join("secret"), root.join("invoice_symlink.csv")).unwrap();
    symlink(&outside, root.join("Invoices")).unwrap();
    let app = Organizer::open(&root).unwrap();
    let plan = app.plan(0.85).unwrap();
    assert!(plan.moves.iter().all(|m| m.destination != "Invoices"));
    app.apply(&plan.id, true).unwrap();
    assert_eq!(
        fs::read_to_string(outside.join("secret")).unwrap(),
        "private"
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
}
#[test]
fn journal_recovers_both_interruption_states() {
    for remove_original in [false, true] {
        let t = Temp::new();
        let root = t.root();
        let plan = demo(&root).unwrap();
        let m = &plan.moves[0];
        let run = Run {
            schema: 1,
            plan: plan.clone(),
            complete: false,
            undone: false,
        };
        fs::write(
            root.join(".teachsort/runs")
                .join(format!("{}.json", plan.id)),
            serde_json::to_vec(&run).unwrap(),
        )
        .unwrap();
        fs::create_dir(root.join(&m.destination)).unwrap();
        let target = root.join(&m.destination).join(&m.source);
        fs::hard_link(root.join(&m.source), &target).unwrap();
        if remove_original {
            fs::remove_file(root.join(&m.source)).unwrap();
        }
        assert!(
            Organizer::open(&root)
                .unwrap()
                .undo(&plan.id, true)
                .unwrap()
                .undone
        );
        assert!(!target.exists());
        assert!(plan.moves.iter().all(|m| root.join(&m.source).is_file()));
    }
}
#[test]
fn kernel_lock_is_released_when_the_handle_closes() {
    let t = Temp::new();
    let root = t.root();
    demo(&root).unwrap();
    let lock = File::options()
        .read(true)
        .write(true)
        .open(root.join(".teachsort/lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let app = Organizer::open(&root).unwrap();
    assert!(app.teach("new.csv", "Invoices").is_err());
    drop(lock);
    assert!(app.teach("new.csv", "Invoices").is_ok());
}
#[test]
fn feedback_correction_is_idempotent_and_unicode_works() {
    let t = Temp::new();
    let root = t.root();
    fs::create_dir(&root).unwrap();
    let app = Organizer::init(&root).unwrap();
    for name in ["청구서_일월.csv", "청구서_이월.csv"] {
        app.teach(name, "청구서").unwrap();
    }
    for name in ["회의록_월요일.md", "회의록_화요일.md"] {
        app.teach(name, "회의록").unwrap();
    }
    app.teach("청구서_일월.csv", "청구서").unwrap();
    let labels = app.labels().unwrap();
    assert_eq!(labels.examples.len(), 4);
    assert_eq!(
        labels
            .predict("청구서_삼월.csv")
            .unwrap()
            .unwrap()
            .destination,
        "청구서"
    );
    app.teach("청구서_일월.csv", "회의록").unwrap();
    assert_eq!(app.labels().unwrap().examples.len(), 4);
}
#[test]
fn sparse_oversize_file_is_not_read_or_moved() {
    let t = Temp::new();
    let root = t.root();
    demo(&root).unwrap();
    let file = File::create(root.join("invoice_huge.csv")).unwrap();
    file.set_len(teachsort::MAX_FILE_BYTES + 1).unwrap();
    let plan = Organizer::open(&root).unwrap().plan(0.85).unwrap();
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.name == "invoice_huge.csv" && s.reason.contains("64 MiB")));
}
#[test]
fn too_few_labels_and_unknown_names_abstain() {
    let t = Temp::new();
    let root = t.root();
    fs::create_dir(&root).unwrap();
    let app = Organizer::init(&root).unwrap();
    app.teach("invoice_one.csv", "Invoices").unwrap();
    app.teach("invoice_two.csv", "Invoices").unwrap();
    assert!(app
        .labels()
        .unwrap()
        .predict("invoice_three.csv")
        .unwrap()
        .is_none());
    assert!(app.plan(f64::NAN).is_err());
    assert!(app.plan(0.1).is_err());
}
#[test]
fn cli_rejects_extra_options_before_any_changes() {
    let t = Temp::new();
    let root = t.root();
    fs::create_dir(&root).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_teachsort"))
        .args(["init", "--root"])
        .arg(&root)
        .args(["--to", "wrong"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.join(".teachsort").exists());
}
#[test]
fn a_new_undertrained_category_causes_abstention() {
    let t = Temp::new();
    let root = t.root();
    demo(&root).unwrap();
    let app = Organizer::open(&root).unwrap();
    app.teach("receipt_one.csv", "Receipts").unwrap();
    assert!(app
        .labels()
        .unwrap()
        .predict("receipt_two.csv")
        .unwrap()
        .is_none());
}
