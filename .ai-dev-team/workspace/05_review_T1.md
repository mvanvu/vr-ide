# Review T1 — APPROVE T1

Findings (no blocker/major):
[minor] crates/vr-ide/src/main.rs:39-41 — raw mode left on if EnterAlternateScreen fails
Scenario: enable_raw_mode() ok, execute!(EnterAlternateScreen) errs -> `?` returns without restore_terminal().
Fix: call restore_terminal() on that error path (or use a guard struct).

[minor] scripts/bench-startup.sh:21 — empty rss_kb aborts with an arithmetic error and no message
Scenario: /usr/bin/time output missing/unparsable -> `$(( rss_kb / 1024 ))` fails cryptically.
Fix: `[ -n "$rss_kb" ] || { echo "no RSS in time log" >&2; exit 1; }`. Also the single-quoted `'$bin'` breaks if the repo path holds a quote.

[minor] crates/vr-ide/src/cli.rs:37 — `exists()` maps permission errors to NotFound
Scenario: unreadable parent -> "path does not exist". Fix: use fs::metadata and report CliError::Io.

Edge cases probed by reading: relative paths (joined to cwd), symlinked dirs (canonicalized), dir inside git (dir is root, per spec), spaces (OsString, no splitting), nonexistent (NotFound naming path), git worktree `.git` file (exists() true). OK.

## Files read
Cargo.toml read; Cargo.lock skipped — generated (ropey 1.6.1 checked); rust-toolchain.toml read; .gitignore read; rustfmt.toml read; crates/*/Cargo.toml read (5); crates/{vr-core,vr-config,vr-fs,vr-search}/src/lib.rs listed (stubs); crates/vr-ide/src/main.rs read; crates/vr-ide/src/cli.rs read; scripts/bench-startup.sh read; README.md read.

## Rule check T1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only task files in tree |
| BASE-2 | pass | resolve/Launch/CliError match plan; extra variants listed in notes |
| BASE-3/4 | pass | red/green and guard output in notes; I reran cli test (ok) and guard (exit 0) |
| BASE-6 | pass | English why-comments |
| BASE-8 | pass | manifest deps = plan stack versions |
| BASE-25 | pass | fmt + clippy -D warnings exit 0 (unused-dependency manifest warnings only) |
| BASE-32 | pass | D4/D5/C3/C6 honoured, no clippy.toml (D14) |
| TOOL-1 | pass | rustc 1.100.0; versions equal plan |
| TOOL-2 | pass | ropey =1.6.1, lock 1.6.1 |
| TOOL-3 | pass | edition 2024, resolver 3, Cargo.lock |
| TOOL-4 | pass | none held back |
| AC-1 | pass | main.rs draws "vr-ide — <root>" |
| AC-2 | pass | cli_resolves_root covers git/non-git |
| Panic hook / EXIT_AFTER_FIRST_FRAME | pass | main.rs hook; bench sets var, run() honours it |
