# Contributing

Small, working improvements with tests are welcome. Open an issue describing the
folder workflow before adding a watcher, content extraction, or an external service.

## Local checks

Use Rust 1.89 or newer, then run `cargo test --locked` and `cargo build --release --locked`.
Tests create private temporary directories and do not use your Downloads folder.
Keep `Cargo.lock` in changes that update dependencies.

## File operation contract

Keep plan generation separate from execution. Preserve no-overwrite behavior,
source hash/identity checks, symlink rejection, journal-before-effects ordering,
and recovery after interruption. Add a test when changing any of those paths.
Do not weaken a test to make a new path pass.

The model's score remains uncalibrated until independent evaluation establishes
otherwise. A new classifier should report its training inputs and failure cases.
Useful automation should be measured on the actual workflow it supports.

Never commit `.teachsort/`, real user filenames, private documents, API keys,
tokens, or development checkout paths. Use synthetic CSV/Markdown files in examples.
This repository is the standalone product source; private research repositories
and their histories are not required to build it.
