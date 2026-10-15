# TeachSort

**Teach your folder where files belong. Review the plan. Apply it. Undo it.**

TeachSort is a small Rust CLI that learns destination folders from filename examples.
It runs locally: classification uses names and extensions, and execution reads matching
files to verify their hashes. It has no network client, API keys, telemetry, or background service.

```text
filename examples → local classifier → reviewable plan → file moves → undo journal
```

## Try it in a new demo folder

Requires Rust 1.89+ and a Unix filesystem supporting hard links. Linux is tested locally;
CI covers Linux and macOS. Windows file execution is outside v0.1's supported scope.

```bash
git clone https://github.com/cjw0076/teachsort.git
cd teachsort
cargo build --release --locked

# Creates a NEW directory with seven small CSV/Markdown/data files.
./target/release/teachsort demo --root /tmp/teachsort-demo
```

The demo learns `Invoices` and `Notes`, plans six moves, and leaves the unfamiliar file alone.
Copy the printed `id`, review `moves`, then run:

```bash
./target/release/teachsort apply --root /tmp/teachsort-demo --plan ID_FROM_PLAN --yes
./target/release/teachsort undo  --root /tmp/teachsort-demo --run ID_FROM_PLAN --yes
```

The same ID identifies the plan and its run. `demo` refuses to replace an existing directory.

## Use your own folder

```bash
teachsort init --root "$HOME/Downloads"
teachsort teach --root "$HOME/Downloads" --name invoice_january.pdf --to Finance/Invoices
teachsort teach --root "$HOME/Downloads" --name invoice_february.pdf --to Finance/Invoices
teachsort teach --root "$HOME/Downloads" --name holiday_beach.jpg --to Photos
teachsort teach --root "$HOME/Downloads" --name holiday_mountain.jpg --to Photos

teachsort plan --root "$HOME/Downloads"
# Review the JSON output before using its ID:
teachsort apply --root "$HOME/Downloads" --plan ID_FROM_PLAN --yes
```

Use `./target/release/teachsort` in place of `teachsort` if it is not on your PATH.
Examples are filenames: the named training files do not need to exist.
Provide at least two examples in each of at least two folders. A repeated `teach`
updates an example, so correcting a destination does not duplicate training data.
`status --root DIR` prints the current examples.

Unknown words, low scores, collisions, symlinks, and oversized files are listed in `skipped`.
To label a skipped item, teach its filename and generate a fresh plan. Changing examples
invalidates existing plans. Optional `--min-score 0.90` raises the planning threshold.

## What is learned?

A native multinomial Naive Bayes classifier uses lowercase filename words and extensions.
It rebuilds its counts from the saved examples, uses Laplace smoothing, and reports the
tokens supporting a suggestion. Numeric-only parts are ignored. Korean and other Unicode
alphabetic words are supported.

The displayed `score` is **uncalibrated**. It is useful for ranking suggestions; it is not
an independently measured probability that a move is correct. Every plan still requires
an explicit `apply --yes` command. Review a plan when using a new set of examples.

## File behavior

- Only visible, regular files directly inside the selected folder are considered.
- Source contents and Unix file identities are checked against the plan before execution.
- Existing destination files are never overwritten. Moves use hard links followed by removal
  of the original name, so source and destination must share a filesystem.
- A complete recovery journal is synced before file moves begin. `undo` also handles an
  interrupted run, including the interval where both hard-linked names exist.
- Undo refuses to overwrite a newly created source file or restore a modified destination.
- Kernel file locks serialize TeachSort commands and are released when a process exits.
- Files up to 64 MiB and up to 10,000 root entries are supported in v0.1. State files are
  limited to 4 MiB. Empty destination folders may remain after undo.

Multiple file moves are recoverable, not a single atomic filesystem transaction. If an
operation fails partway through, inspect the error and use its plan ID with `undo`.
Keep `.teachsort/` until you have finished restoring a run. Treat it as private data: it
contains your filenames, example destinations, file hashes, and filesystem identities.

Use TeachSort in a folder you control. Do not edit files or run another organizer during
apply/undo. The path and hash checks do not provide a security sandbox against a hostile
process changing directory entries concurrently. This first release has no automatic watcher.

## Commands

| Command | Purpose |
|---|---|
| `init --root DIR` | Create private local state |
| `teach --root DIR --name NAME --to FOLDER` | Add or correct an example |
| `status --root DIR` | Inspect saved examples |
| `plan --root DIR [--min-score N]` | Generate a JSON plan without moving files |
| `apply --root DIR --plan ID --yes` | Execute a reviewed plan |
| `undo --root DIR --run ID --yes` | Restore a complete or interrupted run |
| `demo --root NEW_DIR` | Make a new self-contained demonstration folder |

All successful commands emit JSON. Errors go to stderr with exit code 1.

## Development

```bash
cargo test --locked
cargo build --release --locked
```

Integration tests exercise actual files: learning, apply/undo byte preservation,
interruption recovery, collisions, changed sources, symlinks, Unicode, locking,
and command argument validation. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Scope and alternatives

TeachSort is a practical starting point for experience-driven local automation.
Its first release learns filename-to-folder choices and executes reviewed file moves.
The model, planner, and filesystem execution are separate modules for extension.

For a broad rule-based file automation system, see
[organize](https://organize.readthedocs.io/en/latest/). TeachSort's interaction starts
with labeled filename examples and includes a local undo journal.
It was written independently; no code from that project is included.

License: [MIT](LICENSE).
