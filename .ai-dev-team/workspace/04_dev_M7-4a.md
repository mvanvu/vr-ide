# Dev notes — M7-4a editor git display (gutter, status bar, inline blame)

Main tree /home/rainy/DEV/vr-ide (master 7a4a27c). Owner fe. Scope: D297 "In the editor" + "Multi-root", AC-16, 16a, 16b, 16d, 16e, AC-1, SEC-15, SEC-17.
Tools available: Bash, Read, Write, Edit, Skill, Agent, Artifact, ToolSearch, SubagentHandback (+ deferred: Monitor, TaskStop, WebFetch, WebSearch, ...).

## Progress
- read proposal, spec M7, threat model SEC-15/17, D297-D301, 04_dev_M7-1, 05_review_M7-1, capabilities, vr-git API (repo/worker/diff/blame), vr-ide app/event loop/editor_view/status_bar/slash/theme/settings
- 2026-11-25T20:20:29+07:00 vr-config setting git_inline_blame (+ test, green); vr-git dep added to vr-ide
- 2026-11-25T20:25:13+07:00 tests written first: src/git_tests.rs (19 tests), ui/safe_text.rs unit tests; red = compile errors (git::status_label, GitState, MARK_*, theme.git_* missing)
- 2026-11-25T20:29:20+07:00 git.rs, theme fields, editor_view marks + inline blame, status bar tiers, /git blame line, hooks (tick, fs event, roots_changed, exit, main) written; git_tests 18/19 green, then the unsaved-edit test fixed (deletion next to an edit merges into one modified hunk, as git does); 3 new snapshots reviewed
- 2026-11-25T20:31:12+07:00 docs/commands.md (table + paragraph), README settings line, capabilities.md; pty walk script qa-artifacts/M7-4a/dev/m74a.py; workspace clippy + full suite + release build started via run-heavy.sh (background)
- full suite / workspace clippy / release build (run-heavy.sh) all exit 0; pty walk 12/12; nothing left running

## Files
- new: crates/vr-ide/src/git.rs, crates/vr-ide/src/git_tests.rs, crates/vr-ide/src/ui/safe_text.rs, snapshots crates/vr-ide/src/snapshots/vr_ide__git_tests__render_git_gutter_status_and_blame_{120x40,80x24,40x12}.snap
- changed: Cargo.toml (workspace dep vr-git), Cargo.lock (vr-ide -> vr-git), crates/vr-ide/Cargo.toml, crates/vr-ide/src/{app.rs (App.git, start_git, tick + fs-event hooks), app_workspace.rs (roots_changed hook), event_loop.rs (exit shutdown), main.rs (start_git after the first frame), lib.rs, commands/slash.rs (/git blame line), slash_tests.rs (registry list), theme.rs + theme/synthwave.rs (git_added/modified/deleted/blame + contrast pairs), ui/editor_view.rs (marks + inline blame), ui/status_bar.rs (git tiers), ui/mod.rs}; crates/vr-config/{defaults/settings.toml, src/settings.rs, tests/config.rs} (git_inline_blame); docs/commands.md; README.md
- crates/vr-git untouched (read only); no read-API change needed. Merge note: M7-2 may also add `vr-git = { path = ... }` to the root Cargo.toml -> trivial conflict.

## Design calls
- D? (M7-4a) per root, not per file: a file's repo is the one discovered for its workspace root (spec "per root"); a nested repo / submodule inside a root shows no marks for its files. Two roots inside one repo share one GitWorker (by workdir); a worker is shut down when no root uses it.
- D? (M7-4a) `.git` is watched by a second `vr_core::Watcher` owned by GitState (git dir + common dir flat, `refs/` recursive -> `invalidate()` + Head/AheadBehind again); the app's existing watcher is wired to `invalidate_path` (open files, shown folders). A separate instance keeps the explorer's watch_dir bookkeeping untouched (a user expanding `.git` in the explorer would otherwise share and end the same watch entry).
- D? (M7-4a) `/git blame line` toggles for the session (like /theme, /hidden); `git_inline_blame` in settings.toml (default false) keeps it; /set and the live reload handle it. The toggle does not write settings.toml (no slash command writes it).
- D? (M7-4a) gutter marks use the gutter's existing trailing space column (`▎` added/modified, `▁` deletion below the line, `▔` deletion at the top): no layout change for files without git; a change mark wins over a deletion mark on one line.
- D? (M7-4a) timing: edits debounced 250 ms (a newly shown file is asked at once); the blame of the whole buffer is asked with the marks after the same pause (the worker cache makes cursor moves free) and shown once the cursor rests 300 ms on a line; hidden while the version or line changes. Stale marks stay drawn until the new answer (no flicker) but are not "fresh" after any file / .git change.
- D? (M7-4a) status bar: per position tier first `· <branch ↑a ↓b [· parent repo: ~/x]>`, then the short form (branch cut to 16, `· parent repo` without path), plain tiers last; the parent label never drops while the branch shows. Detached: `detached <short>` / `<short>`. Without a file: `<workspace> · <branch>` of the active root.
- D? (M7-4a) no marks/blame for read-only (non-UTF-8) docs or buffers over Options.max_file (8 MiB): no text copy is made; the BOM is prepended for UTF-8-BOM files so marks equal git's.
- Automatic read errors (timeout, too large) show nothing (tracing::debug); a `.git` watch failure is one warning notice.

## Tests (cargo test -p vr-ide: 569 passed, 0 failed, 7 ignored; vr-config 23 incl. the new setting test)
git_tests.rs (19): marks == `git diff -U0` for a saved change and for unsaved edits (diffed after saving the same text); CRLF with text=auto (no marks, then one Modified == git diff); the three marks drawn in the theme colours (+ distinct in all 4 themes); `git add` in the terminal clears the marks via the .git watcher; debounce (no request while typing, one after 250 ms); branch + ahead/behind == `git status -sb` (↑2 ↓1), then a terminal commit (↑3) and a terminal branch switch via the watcher; detached short SHA == `git rev-parse --short`; parent repo label (root = subdir) vs own repo; no-file status bar; no git: nothing shown, no warnings, no worker (with blame toggled on); untracked file + unborn branch; multi-root (main / dev / no git, marks per root, 2 workers); root removal shuts its worker (Repo clone -> ShutDown) and exit shuts the rest; AC-1 with a 2 s-sleeping git shim (start + 20 ticks/frames < 1.5 s, nothing before start_git); inline blame: hidden before the 300 ms rest, "Ann Author, 3 days ago • Add alpha", other line "Bob Builder, ...", hidden while typing, "You, uncommitted", toggle off; registry/docs/`/set`; relative ages; SEC-15 hostile author/subject/branch (ESC, OSC 52, C1 CSI, CR, U+202E) rendered at 120x40/80x24/40x12 with no unsafe char in any cell and placeholders visible; snapshots 120x40/80x24/40x12 (reviewed: marks on lines 2/3/6, blame after line 1, `main ↑1` in the status bar; at 40x12 the blame is cut at the edge).
safe_text.rs unit (4): placeholders, unicode unchanged, tab expansion, sweep U+0000..U+3000 and U+FE00..U+FFFF.
Red evidence: git_tests / safe_text tests written before git.rs (compile red, 61 errors: status_label, inline_text, GitState, MARK_*, theme.git_* missing); first run 23/25: the unsaved-edit test was wrong (a deletion next to an edit is one modified hunk in git too) -> test fixed; snapshot new. The vr-config setting test was written with the field (not seen red).

## Commands run
- `cargo test -p vr-ide` -> 569 passed, 0 failed, 7 ignored
- `cargo test -p vr-config` -> 17 + 23 + 8 passed
- `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> exit 0
- `cargo fmt --all --check` -> clean (rustfmt on own files only)
- `run-heavy.sh cargo test --workspace -j 4` -> exit 0, 32 test binaries, 1032 passed, 0 failed
- `run-heavy.sh cargo build --release -p vr-ide` -> exit 0
- pty walk `qa-artifacts/M7-4a/dev/m74a.py target/release/vr-ide` -> 12/12 (walk.txt): `main ↑1`, gutter ▎ on modified/added lines, unsaved edit mark, /git blame line shows "one  Ann Author, ...", terminal branch switch shows up, no notice, quit leaves no process with cwd in the repo, no-git folder plain.
- Nothing left running (background run finished; pty sessions exited; temp repos are tempdirs).

## SEC compliance
| ID | Evidence |
|---|---|
| SEC-15 | `ui/safe_text.rs::sanitize` is the only path for git text drawn: branch / short SHA and parent path (`git::status_label`, `home_relative`), author + subject (`git::annotation`), watch-error text. Gutter marks are fixed glyphs. Tests: safe_text unit sweep; `hostile_git_strings_are_drawn_without_control_or_bidi_chars` (TestBackend, 3 sizes, every cell) |
| SEC-17 | Each repo has a serial; every request records (serial, path, buffer version) and every answer is matched in its own worker's pending map; marks / blame / status are drawn only when the serial is the active file's root's repo now (`serial_for`) and the version is current. The status bar names the repo of the active file's root; a repo above the root says `parent repo: <~path>` (D298). Tests: `each_root_shows_its_own_repo`, `parent_repo_is_labelled`. `/git <root-name>` and popup binding are M7-3 |
| SEC-1 (app side) | `git::shutdown` on exit (event_loop::exit); workers dropped when a root goes or the workspace is replaced; test `removing_a_root_and_exit_shut_the_git_workers_down`; pty: no process left |
| SEC-7 / AC-16e | inline blame via `Request::InlineBlame` (10 s automatic budget); nothing asked for buffers > max_file |
| SEC-8 | the .git watch cannot loop: vr-git reads write nothing under .git (M7-1) |

## Reuse
| Need | Found | Decision |
|---|---|---|
| git reads, worker, cache | vr-git `Repo`, `GitWorker` (M7-1) | reused as is |
| file watching | `vr_core::Watcher` | second instance for .git |
| slash registry, docs table | `commands::slash::SLASH_COMMANDS` | one entry |
| hermetic git fixture | vr-git tests/common (not importable) | small copy in git_tests.rs |
| pty harness | qa-artifacts/FR-10/h.py | reused |
Added to capabilities.md: safe_text, git.rs, mark_rows / theme fields.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | M7-4a scope only (vr-ide UI, one vr-config setting, docs); vr-git untouched |
| BASE-2 | pass | M7-1 contracts used as written |
| BASE-3 | pass (partial) | tests before code (compile red); setting test not seen red |
| BASE-4 | pass | outputs above |
| BASE-5 / COST | pass | heavy runs via run-heavy.sh once each |
| BASE-6 | pass | English, comments say why |
| BASE-8 | pass | no new external crate |
| BASE-10 | pass | cargo test -p vr-ide / vr-config; full suite once |
| BASE-11 / BASE-24 | pass | nothing committed; no history-changing git command on the shared tree |
| BASE-14 | pass | long runs in the background |
| BASE-15 | pass | tests stop only their own processes |
| BASE-16 | pass | Reuse table, capabilities.md |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines |
| BASE-25 / BASE-38 | pass | clippy / fmt clean; rustfmt on own files only |
| BASE-32 | pass | D297 / D298; no new key binding (commands only) |
| TOOL | n-a | existing toolchain |
| web UI packs (SWEEP, COMPACT, REAL, EASE) | n-a | terminal UI; ratatui snapshots 120x40 / 80x24 / 40x12 |

## Round 1

### Progress
- 2026-11-25T20:45:09+07:00 read 05_review_M7-4a, D304; 4 tests written first in crates/vr-ide/src/git_tests.rs (reopen marks, reopen blame, root becomes/stops being a repo, hung discovery killed at exit) + D304 note on the shutdown test
- 2026-11-25T20:46:50+07:00 fixes in git.rs (forget_doc, root .git rediscovery, discovery kill), app.rs hooks, vr-ide rustix dep; git_tests 23/23 green
- 2026-11-25T20:47:25+07:00 fmt fixed (import order); vr-ide tests 572/1 fail: dialog_tests::replace_never_loses_a_concurrent_write (timing race test, unrelated; passes 2/2 alone); clippy + full suite + release build started via run-heavy.sh
- 2026-11-25T20:50:49+07:00 clippy exit 0; full suite: other crates ok, vr-ide lib 572/1 (workspace_ui_tests::add_folder_checks_run_off_the_ui_thread, load flake, passes alone 2/2 + module 26/26 + vr-ide 573/0 rerun); --exclude vr-ide rerun exit 0; release build exit 0; starting review walk
- 2026-11-25T20:54:01+07:00 live check of git init in a plain root FAILED (discovery ran when .git was half made); added the unborn .git watch (watch_unborn/look_again) + test a_git_folder_seen_before_git_init_finished_is_found_when_done (red with the watch disabled, green with it)
- 2026-11-25T20:58:00+07:00 release rebuilt; review walk 17/17 and rediscover walk 4/4 on it; vr-ide 574/0; workspace clippy and fmt clean; nothing left running

### Findings fixed
| Review finding | Fix | Test (red first) |
|---|---|---|
| major: stale marks/blame after reopening a file changed while closed (git.rs asked_marks/marks/asked_blame/blame keyed by path + version only) | `GitState::forget_doc(path)` drops asked + answered marks, blame and the shown inline for the path; called by `App::open_path` when it makes a new document and by `App::close_doc` when no other tab is on the path | `reopened_file_changed_on_disk_while_closed_gets_fresh_marks` (== `git diff -U0`), `reopened_file_changed_on_disk_while_closed_gets_a_fresh_blame` ("You, uncommitted", not the old author) |
| minor: a root becomes a repo only at discovery | `root_git_changed`: Created/Removed/Renamed of `<root>/.git` from the app watcher (explorer's flat root watch) re-runs discovery (new generation) and drops unused workers. Live walk showed the root watch fires while `git init` is still filling `.git`, so a `.git` folder git does not take yet is watched flat (`unborn_git`), and a change there looks again (one discovery per root at a time, `look_again` when one is running) | `a_root_that_becomes_a_repo_later_is_found_and_lost_again` (also: `.gitignore` created does not rediscover; `.git` removed -> no repo, worker ended), `a_git_folder_seen_before_git_init_finished_is_found_when_done` |
| minor: discovery threads not covered by shutdown | each discovery thread records its kernel tid (`/proc/thread-self`); `GitState::shutdown` sets a closed flag (threads that have not run git skip it) and kills the process group of every child listed in `/proc/self/task/<tid>/children` of running discovery threads, until they end or 1 s passes. Only the threads' own children are hit (not terminals). Linux-only (the supported platform); elsewhere a no-op. vr-ide gets `rustix` (workspace dep, `process` feature) | `a_hung_discovery_git_is_killed_at_exit` (git shim that hangs with a child `sleep 60`; exit < 2 s; the child is gone) |
| minor (D304) | `repo_of_root` documented read-only; test at `removing_a_root_and_exit_shut_the_git_workers_down` carries the note that writes use `vr_git::Writer::open` (M7-3) | n/a (doc) |

### Files changed (round 1)
- crates/vr-ide/src/git.rs, crates/vr-ide/src/git_tests.rs, crates/vr-ide/src/app.rs, crates/vr-ide/Cargo.toml, Cargo.lock (rustix line for vr-ide)
- walk: .ai-dev-team/workspace/qa-artifacts/M7-4a/dev-r1/ (walk.txt from review/rev.py, rediscover.py + rediscover.txt, screens)

### Commands (trimmed real output)
- `cargo test -p vr-ide --lib -- git_tests` before the fix: 4 FAILED (reopen marks/blame stale, branch timeout, hung git survived)
- after: `test result: ok. 24 passed; 0 failed` (git_tests); `cargo test -p vr-ide`: `574 passed; 0 failed; 7 ignored`
- `cargo fmt --all -- --check`: clean; `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`: exit 0
- `run-heavy.sh cargo test --workspace` (once): every crate ok except vr-ide lib `572 passed; 1 failed` = `workspace_ui_tests::add_folder_checks_run_off_the_ui_thread` (timing under load; passes alone 2/2, module 26/26, vr-ide rerun 0 failed; git not started in that test). An earlier light vr-ide run had `dialog_tests::replace_never_loses_a_concurrent_write` fail once the same way (passes alone 2/2). `--exclude vr-ide` rerun to cover the crates after it: exit 0
- `run-heavy.sh cargo build --release`: Finished, exit 0 (rebuilt after the unborn-.git change)
- `review/rev.py target/release/vr-ide dev-r1`: 17/17 (was 16/17: "reopened file shows the line added while it was closed" now PASS)
- `dev-r1/rediscover.py`: 4/4 (plain root, `git init` + commit from outside -> `· main` in the status bar; `rm -rf .git` -> gone; quit)

### Rule compliance (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only the four review findings; vr-git and .worktrees untouched |
| BASE-3 | pass | 5 tests red before the fix (unborn test red with the watch disabled) |
| BASE-4 | pass | outputs above |
| BASE-10 | pass | related tests + mapped guards; full suite once via run-heavy |
| BASE-16 | pass | reused the explorer's root watch, GitState's .git watcher and the FR-10 pty harness |
| SEC-1 (app side) | pass | discovery gits killed with their group at exit (test) |
| SEC-17 | pass | reopened document never shows answers of an earlier document |
| AC-16a | pass | reopen walk + test |
| COST-9 | pass | no vr-ide / walk process left (ps check) |
