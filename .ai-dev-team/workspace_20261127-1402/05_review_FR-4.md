# Review FR-4 — APPROVE FR-4

Scope: `git diff master -- crates/vr-core/src/watch.rs` (221+/7-). Tests rerun in worktree: `cargo test -p vr-core -p vr-ide` 141 + 79 passed; `watch::` tests (19) 10x loop all green.

## Findings
[minor] crates/vr-core/src/watch.rs (test `dir_watch_of_ten_thousand_subdirs_is_one_watch_and_fast`) — 5 ms wall-clock bound
Scenario: measured ~1 ms (about 5x headroom); a loaded CI runner or a cold /tmp can exceed 5 ms with no regression -> flaky red. The O(1) property is already proven by `inotify_watches == 1`.
Fix: loosen to ~25-50 ms (a walk over 10k dirs in a debug build costs far more), or keep the timing as `eprintln!` only.

[minor] crates/vr-core/src/watch.rs:364-369 (translate, rescan) — duplicate Rescan
Scenario: the same directory is both `watch`ed (recursive) and `watch_dir`ed -> one overflow emits `Rescan` twice (once per map). Harmless (a re-read is idempotent) but redundant.
Fix: skip `flat` entries whose resolved key is also in `dirs`.

[minor, uncertain] crates/vr-core/src/watch.rs:200-205 — doc claim "no walk" on macOS/Windows
Scenario: on non-Linux, FileIdMap `add_path(NonRecursive)` stats the direct entries (O(entries), not O(1)); the doc comment says "debouncer caches only the direct entries", which is accurate, but the heading claim "O(1)" and the 10k/1 ms measurement hold on Linux only. Not reachable on the Linux target; noted for later ports.

## Checks asked for
- Cost: Linux `RecommendedCache` is `NoCache`; the diff does not touch the debouncer or cache type, so rename tracking, own-save suppression and `Renamed` for existing file watches are unchanged (existing 13 watch tests green). watch_dir = canonicalize + one `inotify_add_watch` request; `sync` is O(active roots), no walk.
- Overlap: flat dir inside a recursive root adds no extra notify watch (`notify_roots` skips it) and is still `contains`-covered via `dir_root`; a file watch and a `watch_dir` on the same dir share one NonRecursive root; `unwatch_dir` retains `files` and re-syncs (test covers the open file still reporting). Mode change recursive<->flat handled by `sync`'s stale check.
- Path spelling: `caller_path` uses `tree_root`, so events use the spelling given to `watch_dir`; `unwatch_dir` matches by given spelling (same convention as `unwatch`); `unwatch(p)` does not remove a flat watch and vice versa, by design.
- Rescan/Error: Rescan per flat dir added and tested; `FsEvent::Error` is watcher-wide.
- Symlinks: non-recursive inotify, no follow; test asserts no watch on the target. A symlink passed as `dir` is canonicalized.
- Thread-safety: runs on caller thread; `Scope` mutex is held only for map edits; `debouncer.watch` waits on notify's loop thread (mio, responsive) and the cache mutex (NoCache, trivial). No UI-block path found. On failure the flat entry is rolled back and re-synced. One caveat shared with `watch`: `sync` adds every wanted root, so a failure on an unrelated stale root fails the call; pre-existing behaviour.

## Files read
- crates/vr-core/src/watch.rs — read (diff plus Watcher/sync/translate context)
- .ai-dev-team/workspace/04_dev_FR-4.md — read

## Rule check FR-4
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only watch.rs changed (`git diff --stat`) |
| BASE-2 | pass | additive `watch_dir`/`unwatch_dir`; existing API/FsEvent unchanged, old tests green |
| BASE-3 | pass | per-test red lines recorded in notes (tests 2-6 run against a restored stub; accepted) |
| BASE-4 | pass | notes carry fmt/clippy/test output; reran tests green |
| BASE-6 | pass | English why-comments, no bug history |
| BASE-32 | pass | D44 spelling, D47 Rescan/Error, D110 a/b/d honoured |
| C3 | pass | one inotify watch per folder, no path cache on Linux |

## Quick pass 1 re-check — APPROVE FR-4 QP1
- Minor 1: timing bounds in the 10k test are now 50 ms (watch_dir and unwatch_dir); the `inotify_watches == 1` assert still proves O(1). Fixed.
- Minor 2: `translate` filters flat dirs whose resolved key is also in `dirs`, so Rescan is emitted once; new test `rescan_is_sent_once_for_a_dir_watched_both_ways` asserts a single `Rescan`. Flat-only dirs still get theirs (`rescan_covers_dir_watches`). Fixed, nothing new opened.
- `cargo test -p vr-core watch::` -> 20 passed, 0 failed.
