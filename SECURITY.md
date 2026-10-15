# Security and data handling

TeachSort v0.1 runs with your user permissions inside an explicitly selected folder.
It has no runtime networking. Classification uses filenames; hashing reads matching
file bytes locally. `.teachsort/` contains private filename examples and journals.

Supported execution environment: a trusted local Unix directory with hard-link
support. Hostile concurrent directory mutation, shared writable state, remote
filesystems with different locking/durability semantics, and Windows file execution
are outside the current supported scope.

Preserve source/hash checks, no-overwrite operations and durable recovery records
when reporting or fixing filesystem issues. Include a minimal reproduction using
synthetic files. Avoid posting real filenames or document contents in public issues.

For sensitive vulnerability reports, use GitHub's private vulnerability reporting
when available. Do not include credentials in any report.
