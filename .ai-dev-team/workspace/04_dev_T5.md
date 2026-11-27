# T5 Desktop entry new form + migration
Task: `| T5 | Desktop entry new form + migration | fe | T1 | crates/vr-ide/src/desktop.rs, desktop_tests.rs | security | BASE, COST, AC-W7 | cargo test -p vr-ide desktop |`

## Progress
- read desktop.rs, desktop_tests.rs, spec "Launch", D3
- red: new tests fail to compile (EntryForm, migrate_on_window_start missing)
- green: cargo test -p vr-ide desktop 21 passed
- code: EntryForm, launches_in_window, migrate_on_window_start; dropped db refresh in migration (wrote mimeinfo.cache into the test dir)
- related tests, clippy, fmt, guard: green (output below)

- Files changed: `crates/vr-ide/src/desktop.rs`, `crates/vr-ide/src/desktop_tests.rs`
- Tests added (desktop_tests.rs): `install_window_form_writes_a_direct_launch_entry`, `migration_rewrites_an_old_entry_of_ours_once`, `migration_leaves_new_absent_and_terminal_form_entries_alone`, `migration_never_touches_a_foreign_entry`, `migration_never_follows_or_replaces_a_symlinked_entry`, `migration_skips_an_entry_that_starts_another_binary`, `migration_write_failure_returns_a_warning_and_keeps_the_entry`, `uninstall_removes_both_forms`; `exec_round_trips_hostile_path` extended to all three forms. All use a tempdir as data home; none touches `~/.local/share`. `desktop-file-validate` (/usr/bin) validates the window-form and migrated entries.

## API for T4
- `pub enum EntryForm { Window, GnomeTerminal, Terminal }` (desktop.rs:62)
- `pub fn launches_in_window() -> bool` (desktop.rs:75) — returns `true` today. **T4 gates it** on the `window` feature (and the display check if wanted). `run_install` picks `Window` when it is true, else `GnomeTerminal` when `gnome-terminal` is on PATH, else `Terminal`.
- `pub fn build_entry(exe: &str, form: EntryForm) -> String`, `pub fn install(data_home, exe, form: EntryForm)` — signature changed from `bool` to `EntryForm`; only callers are `run_install` and the tests.
- `pub fn migrate_on_window_start(data_home: &Path, exe: &Path) -> Option<String>` (desktop.rs:217). T4 calls it once at window start with `desktop::data_home()?` and `std::env::current_exe()?` (skip silently if either errors) and shows the returned text as a notice:
  - `Some("Updated the vr-ide menu entry to open its own window.")` — rewritten.
  - `Some("warning: could not update the vr-ide menu entry: …")` — write failed; entry unchanged.
  - `None` — absent, not a regular file (symlink), no exact marker line, not the GNOME Terminal form (already new, or the `Terminal=true` form), or `TryExec` names a different binary than `exe`.
  - Never panics; idempotent (second call `None`, file not rewritten).

## Deviations / decisions (for review)
- **Different binary → no rewrite.** The migration only rewrites when the entry's `TryExec` is the same file as the running exe (canonicalized). Reason: a dev build (`target/debug/vr-ide`) opening a window must not repoint the user's menu entry at itself, and an entry naming another (possibly TUI-only) binary must not be switched to `Terminal=false`. That binary updates its own entry on its own first window start, or the user runs `--install-desktop`.
- **`Terminal=true` form (non-GNOME desktops) is not migrated** — AC-W7 names only the gnome-terminal form. With the window binary it still works (a terminal opens beside the window). Leader/user may want it migrated too; it is a one-line change (`exec.starts_with(GNOME_EXEC_PREFIX)` → also `Terminal=true`).
- **No `update-desktop-database` after migration**: the entry has no `MimeType`, so the MIME cache does not change; it also keeps the function free of child processes on window start.
- Safe write: re-runs `check_target` (marker, no symlink) right before `write_atomic` (O_NOFOLLOW temp file + rename, which replaces a symlink rather than following it); `validate_exe` runs on the exe before the Exec line is built.
- Brief said BASE-1..51; rule-packs.md §3 in plugin 1.0.45 lists BASE-1..49, so the table covers 1..49.

## Commands run
```
$ cargo test -p vr-ide desktop     # red first
error[E0432]: unresolved imports `crate::desktop::EntryForm`, `crate::desktop::migrate_on_window_start`
$ cargo test -p vr-ide desktop     # after code (1st run)
test desktop_tests::migration_rewrites_an_old_entry_of_ours_once ... FAILED   (left: 2 right: 1 — mimeinfo.cache from the db refresh; refresh removed)
$ cargo test -p vr-ide desktop
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 1123 filtered out; finished in 0.10s
test result: ok. 2 passed; 0 failed; ... (other targets: 0 run)
$ cargo clippy -p vr-ide --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 31.31s
$ rustfmt --edition 2024 crates/vr-ide/src/desktop.rs crates/vr-ide/src/desktop_tests.rs   (ok)
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings      # guard, exact
warning: unused workspace dependency `vr-window`   (Cargo.toml, T2's in-progress change, not mine)
    Checking vr-ide v0.1.0
    Checking vr-window v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 30.48s   (exit 0)
```

## Out-of-scope noticed
- Workspace manifest warning `unused workspace dependency vr-window` (T2 / T4 territory).
- AC-W7 text lives in 02_tasks.md; spec 01_spec.md "Launch" has the same behaviour without the `AC-W7` label.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| safe write / ours-check / exe validation | desktop.rs | reused `write_atomic`, `check_target`, `read_regular_nofollow`, `validate_exe` |
| Exec parsing in tests | desktop_tests.rs | reused `parse_exec`, `exec_of`, `fake_exe`, `validate` |
| capability index | `.ai-dev-team/capabilities.md` (no desktop entry) | nothing new to add beyond desktop.rs API above |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-W7 | pass | `install_window_form_writes_a_direct_launch_entry`, `migration_rewrites_an_old_entry_of_ours_once`, `migration_never_touches_a_foreign_entry`, `migration_never_follows_or_replaces_a_symlinked_entry`; run_install picks `EntryForm::Window` via `launches_in_window()` |
| BASE-1 | pass | `git diff --stat` touches only crates/vr-ide/src/desktop.rs, desktop_tests.rs |
| BASE-2 | pass | `migrate_on_window_start(&Path,&Path)->Option<String>` as briefed; deviations (same-binary rule) listed above |
| BASE-3 | pass | red run E0432 before code; failing migration test before refresh removal |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | related run is one test module (light, direct); guard run unwrapped exactly as the brief wrote it (BASE-25), 30 s; nothing left running |
| BASE-6 | pass | comments English, "why" only (desktop.rs migrate doc, refresh comment) |
| BASE-9 | pass | exe validated (`validate_exe`), entry read with O_NOFOLLOW, marker + regular-file check re-run before write; symlink/foreign tests |
| BASE-10 | pass | only `cargo test -p vr-ide desktop` + mapped guard |
| BASE-11 | pass | no stash/checkout/restore run |
| BASE-13 | pass | migration rewrites only marker-carrying regular files vr-ide wrote; foreign/symlink tests byte-for-byte |
| BASE-14 | pass | all commands finished in foreground < 1 min |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | Progress section from start |
| BASE-22 | pass | new names follow module style (`EntryForm`, `migrate_on_window_start`, test names `migration_*`) |
| BASE-23 | pass | both entry paths tested: `--install-desktop` (install tests) and window start (migration tests) |
| BASE-24 | pass | no git index/history command |
| BASE-25 | pass | guard exact, exit 0 |
| BASE-32 | pass | D3 kept: no window code in vr-ide; only a switch fn for T4 |
| BASE-38 | pass | rustfmt only on the two changed files |
| BASE-45 | pass | AC-W7 behaviour matches spec "Launch" text (label only in 02_tasks.md, noted) |
| BASE-48 | pass | no claim about real machine state; tests use tempdirs |
| COST-3 | pass | no sleep/pgrep |
| COST-8 | pass | edits batched per file; tests after the group |
| COST-9 | pass | nothing left running |
| COST-10 | pass | `grep -rl desktop crates/vr-ide/src --include=*tests*.rs` → desktop_tests.rs (run) and app_tests.rs:624 (a comment about desktop key grabs, not this module) |
| COST-13 | pass | no scripts written; nothing in $TEAM_TOOLS |
| COST-16 | pass | outputs filtered to summary lines |

n-a: BASE-7 (no commit in this task) · BASE-8 (no dependency change) · BASE-12 (no other repo) · BASE-20 (nothing removed; `bool` param replaced, all callers updated) · BASE-21 (no secrets) · BASE-26 (no rewrite in another stack) · BASE-27, BASE-30 (no shared data) · BASE-28 (own code) · BASE-29, BASE-40, BASE-41, BASE-43 (no outward systems, credentials, access rules) · BASE-31 (no containers) · BASE-33 (no external state) · BASE-34 (no blocked step) · BASE-35 (no USER rules for this area) · BASE-36 (single app) · BASE-37, BASE-47 (not a defect fix) · BASE-39 (no schema) · BASE-42 (no test settings) · BASE-44 (no fake adapter) · BASE-46 (no runtime qc evidence claimed) · BASE-49 (no program usage probed) · COST-1 (no browser/heavy suite) · COST-2 (dev tests only) · COST-4, COST-11 (no full suite / e2e) · COST-5 (nothing started) · COST-6, COST-7 (no UI) · COST-12 (leader) · COST-14 (no CI) · COST-15, COST-17, COST-19, COST-20 (no goldens / UI) · COST-18 (no full run) · COST-21 (qc)

## Fix round 1
- read findings 1-3
- red: a_build_without_the_window_picks_a_terminal_form (E0432 entry_form), migration_rewrites_our_terminal_form_entry_but_not_a_changed_exec (panicked: expect a notice). cfg!(feature="window") gave 'unexpected cfg condition value: window' -> kept false + doc naming T4
- code: `launches_in_window()` -> `false` (+ doc naming T4), new `entry_form(window, gnome_on_path)`, terminal-form migration; green; guard green

### Findings
1. [major] fixed — `launches_in_window()` (desktop.rs:76-78) now returns `false`. I tried `cfg!(feature = "window")` first; it gives `warning: unexpected cfg condition value: window` (`unexpected_cfgs`, on by default), so clippy `-D warnings` would fail. I kept `false`, and the doc comment says T4 replaces the body with `cfg!(feature = "window")` when it declares the feature. Form selection is now `pub fn entry_form(window: bool, gnome_terminal_on_path: bool) -> EntryForm` (desktop.rs:82), and `run_install` calls `entry_form(launches_in_window(), find_in_path("gnome-terminal").is_some())`. Today this build writes GnomeTerminal or Terminal. `migrate_on_window_start` is unchanged and callable regardless of the feature. Test: `a_build_without_the_window_picks_a_terminal_form`.
2. [minor] fixed — the migration also rewrites an entry of ours with `Terminal=true` and `Exec=` exactly `quote_exec_arg(<TryExec>) + " --session"`, under the same `TryExec` same-file check as the GNOME case (desktop.rs:239-250). An entry whose Exec was edited is kept. Test: `migration_rewrites_our_terminal_form_entry_but_not_a_changed_exec`. `migration_leaves_new_absent_and_terminal_form_entries_alone` was renamed to `migration_leaves_new_and_absent_entries_alone` and no longer expects the Terminal form to be left alone.
3. [low] no change, as instructed.

This supersedes two items in the sections above. **Deviations**: the `Terminal=true` form is now migrated. **API**: `launches_in_window()` returns `false` until T4.

### Commands (round 1)
```
$ cargo test -p vr-ide desktop    # red
error[E0432]: unresolved import `crate::desktop::entry_form`
$ cargo test -p vr-ide desktop    # entry_form added, migration not yet
warning: unexpected `cfg` condition value: `window`     (-> kept `false`)
test desktop_tests::migration_rewrites_our_terminal_form_entry_but_not_a_changed_exec ... FAILED  (expect "a notice")
$ cargo test -p vr-ide desktop
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 1123 filtered out
$ cargo clippy -p vr-ide --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.52s
$ rustfmt --edition 2024 crates/vr-ide/src/desktop.rs crates/vr-ide/src/desktop_tests.rs
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
guard exit=0
```
BASE-51: before the red tests I copied desktop.rs and desktop_tests.rs to `qa-artifacts/T5/`. When a `perl -0pi` edit misplaced a block (its `|` delimiter clashed with the closure bars), I fixed it with the Edit tool and checked the result with `diff` against that copy; the diff shows only the intended changes. I deleted the two copies afterwards. They were created by this task.

### Rule compliance (round 1 rows changed)
| Rule | Status | Evidence |
|---|---|---|
| AC-W7 | pass | with T4's feature: window form plus migration of both old forms (tests above); without it: terminal form (`a_build_without_the_window_picks_a_terminal_form`) |
| BASE-3 | pass | both new tests were seen red before the code (Commands round 1) |
| BASE-25 | pass | guard exit=0 |
| BASE-51 | pass | copy-and-diff check described above; copies removed |
