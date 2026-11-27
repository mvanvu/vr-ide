# FR-4 vr-core: non-recursive directory watch, set up off the UI thread
Worktree: .worktrees/FR4 (branch fr4-watch-dirs). Source: 05_review_T8.md major 1, D110.

## Progress
- Read brief-common, 05_review_T8 major 1, watch.rs, D38–D47, D110, notify-debouncer-full 0.8 cache.rs, notify 9 inotify.rs.
- Finding: on Linux `RecommendedCache` = `NoCache` (cache.rs:119), so no FileIdMap walk happens here; the walk exists on macOS/Windows only, and there `add_path(NonRecursive)` walks depth 1 (cache.rs:51-63). notify inotify non-recursive `add_watch` = one `add_single_watch`, no walk (inotify.rs:975-981).
- Plan: stub `watch_dir`/`unwatch_dir` (no-op) so each new test compiles and goes red by assertion, then implement.
- Red 1: `dir_watch_reports_entry_create_modify_rename_remove` against the no-op stub → `left: [] right: [Created("/tmp/.tmpJwgEHA/a.txt")]`.
- Code: `Scope.flat`, `flat_root`/`tree_root`, `notify_roots` adds NonRecursive per flat dir outside recursive roots, `watch_dir`/`unwatch_dir`. Test 1 green.
- Red 2–6 (written before the code they cover was enabled; run against the stub body `return Ok(())` restored temporarily, then the real body put back):
  - `dir_watch_ignores_deeper_levels_and_adds_no_watch` → `left: 0 right: 1` (watch count)
  - `unwatch_dir_stops_events_and_keeps_file_watches` → `left: [] right: [Created(".../a.txt")]`
  - `dir_watch_of_ten_thousand_subdirs_is_one_watch_and_fast` → `left: 0 right: 1`
  - `dir_watch_does_not_follow_symlinked_dirs` → `left: 0 right: 1`
  - `rescan_covers_dir_watches` → `left: [] right: [Rescan("/tmp/.tmpM9lSqI")]`; stayed red with the real `watch_dir` until `translate` chained `scope.flat` into the Rescan list.
- Green: all 6 new tests; full vr-core + vr-ide; clippy workspace; fmt check.

# FR-4 summary
- Files changed: crates/vr-core/src/watch.rs (only). Team files: capabilities.md (watch.rs line), this notes file.
- API added (additive, BASE-2):
  - `pub fn watch_dir(&mut self, dir: &Path) -> Result<(), WatchError>` — one dir, direct entries only (create/remove/rename/modify, and the dir itself removed); events use `dir` as given as prefix; not a dir → `WatchError::Path(dir, NotADirectory)`; missing → `WatchError::Path(dir, NotFound)`.
  - `pub fn unwatch_dir(&mut self, dir: &Path)` — by the spelling given to `watch_dir`; file watches and recursive watches stay.
  - Existing `watch`/`unwatch`/`own_writes`/`FsEvent`/`WatchError` unchanged.
- Design (D110 b): no command channel added. `watch_dir` is O(1): canonicalize + one request to notify's inotify event-loop thread (`inotify_add_watch` once, inotify.rs:975-981); on Linux the debouncer cache is `NoCache` (notify-debouncer-full cache.rs:119), so no walk; on macOS/Windows `FileIdMap::add_path(NonRecursive)` walks depth 1 only (cache.rs:51-63). Documented in the `watch_dir` doc comment.
- Measured (debug build, 10,000 subdirectories, 5 runs): watch_dir 0.96–1.04 ms; unwatch_dir 0.06–0.16 ms; kernel watches after watch_dir = 1 (counted from /proc/self/fdinfo inotify lines of this watcher's fd), after unwatch_dir = 0.
- Overlap rules: a flat dir inside a recursive `watch` root adds no watch (the recursive one covers it); a file watch in a flat dir shares the same NonRecursive watch; `unwatch_dir` keeps it for the file (test).
- Symlinks: symlinked entries in a watched dir are not followed (no watch on the target, no events — test). A symlink passed as `dir` itself is canonicalized once and events map back to the given spelling (same as `watch`).
- Rescan/Error (D110 d): queue overflow emits `Rescan(given)` for each `watch_dir` dir too (`rescan_covers_dir_watches`); `FsEvent::Error` is watcher-wide and already covers them (`watcher_errors_reach_the_caller`).
- Deviations: none. Note: the 5 ms bound in the 10k test leaves ~5x headroom on this machine; a heavily loaded CI box could approach it.
- Out of scope: wiring in crates/vr-ide (T8 developer after merge). vr-ide manifest warning in clippy output is pre-existing (`vr-ide (manifest) generated 1 warning`), not touched.

## Commands (trimmed real output, worktree .worktrees/FR4)
```
$ cargo fmt --all --check            → fmt-rc=0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
warning: `vr-ide` (manifest) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.32s
$ run-heavy.sh cargo test -p vr-core -p vr-ide
test result: ok. 141 passed; 0 failed; 1 ignored   (vr-core)
test result: ok. 79 passed; 0 failed; 2 ignored    (vr-ide)
$ cargo test -p vr-core --lib ten_thousand -- --nocapture   (x5)
watch_dir over 10,000 subdirs took 1.038029ms unwatch_dir took 113.576µs
watch_dir over 10,000 subdirs took 1.00647ms  unwatch_dir took 56.155µs
watch_dir over 10,000 subdirs took 1.021321ms unwatch_dir took 159.352µs
watch_dir over 10,000 subdirs took 1.018971ms unwatch_dir took 135.843µs
watch_dir over 10,000 subdirs took 958.933µs  unwatch_dir took 149.251µs
```

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| dir watch with caller paths, own-save filter, rescan | capabilities.md, watch.rs | extended existing `Scope`/`translate`/`sync`; no new module |
| non-walking cache | notify-debouncer-full cache.rs | `RecommendedCache` is already `NoCache` on Linux; kept |
| watch count probe | — | test helper `inotify_watches` (fdinfo of the fd watching the dir's inode) in watch.rs tests |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git diff --stat`: only crates/vr-core/src/watch.rs; crates/vr-ide untouched |
| BASE-2 | pass | additive `watch_dir`/`unwatch_dir`; existing API and its 13 tests unchanged and green |
| BASE-3 | pass | 6 reds recorded above, each against the no-op stub before the code ran |
| BASE-4 | pass | Commands section |
| BASE-5 | pass | clippy/test via run-heavy.sh; nothing left running |
| BASE-6 | pass | English why-comments, no bug history |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | `cargo test -p vr-core -p vr-ide` as briefed |
| BASE-11 | pass | no git state commands; worktree only |
| BASE-14 | pass | all runs finished in foreground (<1 min) |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table; capabilities.md watch.rs line extended |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | Progress section |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass | fmt check rc=0; clippy --workspace -D warnings finished clean |
| BASE-32 | pass | D44 caller paths/own-save kept; D47 Rescan/Error extended to dir watches; D110 a/b/d met (tests + measurement); D38–D43, D45, D46 untouched behaviour |
| BASE-38 | pass | `rustfmt --edition 2024 crates/vr-core/src/watch.rs` only |
| C3 | pass | 1 inotify watch per expanded folder, no cached path set on Linux |
| COST | pass | related tests only, one build, no polling loops |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20–BASE-23, BASE-26–BASE-31, BASE-33–BASE-37 — not exercised (no UI, no removals, no user-file writes, no fixtures)

## Quick pass 1
Source: 05_review_FR-4.md minors 1 and 2.
- (1) fixed: `dir_watch_of_ten_thousand_subdirs_is_one_watch_and_fast` time bound 5 ms → 50 ms for both `watch_dir` and `unwatch_dir`; `inotify_watches == 1` / `== 0` kept as the O(1) proof (comment says so).
- (2) fixed: `translate` skips a `flat` entry whose resolved key is also in `dirs`, so a directory watched both ways gets one `Rescan`.
  - Test first: `rescan_is_sent_once_for_a_dir_watched_both_ways`. Red before the fix:
    `left: [Rescan("/tmp/.tmp23sp5m"), Rescan("/tmp/.tmp23sp5m")]  right: [Rescan("/tmp/.tmp23sp5m")]`
- Files changed: crates/vr-core/src/watch.rs only.
```
$ rustfmt --edition 2024 crates/vr-core/src/watch.rs; cargo fmt --all --check → fmt-rc=0
$ run-heavy.sh cargo clippy -p vr-core --all-targets -- -D warnings → Finished `dev` profile
$ run-heavy.sh cargo test -p vr-core -- --nocapture
test watch::tests::rescan_covers_dir_watches ... ok
test watch::tests::rescan_is_sent_once_for_a_dir_watched_both_ways ... ok
test watch::tests::rescan_reports_every_watched_file ... ok
watch_dir over 10,000 subdirs took 704.565µs
unwatch_dir took 46.49µs
test result: ok. 142 passed; 0 failed; 1 ignored
```
Rule rows touched: BASE-3 pass (red line above), BASE-4 pass (output above), BASE-25 pass (fmt rc=0, clippy -p vr-core clean), BASE-38 pass (rustfmt on watch.rs only). Nothing left running.
