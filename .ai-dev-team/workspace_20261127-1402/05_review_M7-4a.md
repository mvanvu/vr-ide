# Review M7-4a — editor git display (gutter, status bar, inline blame)

Reviewer, one pass: code + security (SEC-15, SEC-17) + design QA. Scope: uncommitted M7-4a diff on master (7a4a27c).
Tools available: Bash, Read, Write, Skill, SendMessage, SubagentHandback.

**Verdict: CHANGES_REQUESTED** (1 major; security: SECURE)

## Findings

[major] crates/vr-ide/src/git.rs:742 (with :144, :145, :282-305) — gutter marks of a file reopened after it changed on disk stay stale
Scenario: open `sub/deep/c.txt` (clean, buffer version 0, `asked_marks[c] = (serial, 0)`), close the tab (the app unwatches the file; its folder is not expanded in the explorer, so nothing watches it), append a line from another program, open it again. The new document starts again at version 0, `asked_marks` still holds `(serial, 0)` for the path, so no request is sent and the old `MarkSet` (fresh, same serial) is drawn: the added line 3 has no mark, and stays so until the user types or `.git` changes. Reproduced in the release build: `qa-artifacts/M7-4a/review/walk.txt` (FAIL "reopened file ..."), screen `c_reopen.txt`. Same for `asked_blame` / `blame` when the reopened file was the last one blamed and no other file was active in between (closing the last tab): the old blame's lines are shown against the new text (wrong author/subject on a line).
Fix: bind the "already asked" state to the document, not only path + version: drop `asked_marks` / `marks` / `asked_blame` / `blame` for a path when its document is opened or closed (hook in `open_path` / `close_doc`), or key them by (doc id, version). The worker cache (file stat + text hash) keeps a re-ask cheap. Add a test: open, close, change on disk, reopen → marks == `git diff -U0`.

[minor] crates/vr-ide/src/git.rs:247-254, git_tests.rs:590-612 — design risk for M7-2/M7-3: a `Repo` handed out by `repo_of_root` is killed with the root
Scenario: `repo_of_root` returns a clone sharing the worker's runner; `drop_unused_repos` (:394-395, any root removal / workspace replace) and exit call `shutdown`, which SIGKILLs every live git process of all clones (M7-1 note). If M7-3's popup runs its stage/commit/push through this clone, removing that root or replacing the workspace mid-commit kills `git commit` and can leave `index.lock` behind (the user's next git command fails), or cut a push half-way. No writer exists yet, hence minor; the test pins the shared-kill behaviour as a contract.
Fix: document `repo_of_root` as read-only and have M7-2/M7-3 writes run on a separately discovered `Repo` (own runner), or add a vr-git way to clone with a fresh runner; root removal / workspace replace while a write runs should wait for it or ask, never SIGKILL it. Record the rule in decisions before M7-3 starts.

[minor] crates/vr-ide/src/git.rs:421-423 — a root becomes a repo only at discovery
Scenario: open a plain folder, run `git init` (or `git clone .`) in the integrated terminal → root stays `NoRepo` for the session: no branch, no marks until restart or root re-add. Likewise after `.git` is removed and re-created the flat watch is gone and HEAD is never asked again.
Fix: the explorer already watches each root flat; on `FsEvent::Created(<root>/.git)` (or Removed) in `on_fs_event`, re-run discovery for that root (new generation). Cheap; the user decides whether it is in M7-4a.

[minor] crates/vr-ide/src/git.rs:349-368, 913-915 — discovery threads are not covered by shutdown
Scenario: exit while a discovery `git rev-parse` hangs (slow NFS): the process group was setsid'd by the runner and the runner's timeout lives in the discovery thread, which dies with vr-ide → the git child can outlive vr-ide (SEC-1 spirit). Rare.
Fix: keep the discovering `Repo`/runner handle (or a shared kill registry) in `GitState` so `shutdown` kills it too.

No other findings. Checked and fine: discovery, worker start and every git call off the UI thread (`buffer_text` copy only after the debounce, capped by `max_file`); worker is sequential so answers arrive in request order and serial + version filters hold (SEC-17 for the editor part); `.git` watch: flat git dir (+ common dir), recursive `refs/`, packed-refs/FETCH_HEAD/index covered, no loop (idle CPU 2 ticks / 4 s in the pty walk); multi-root shares one worker per workdir and drops it only when unused; exit and root removal shut workers (no process left in the walk); no new key binding (keymap.toml untouched); status bar tiers drop the branch before the file name, parent label never dropped while a branch shows; theme colours reuse each theme's existing tokens (`dim` at 3:1 is the project's convention for dim text).

## Security (SEC-15, SEC-17) — SECURE
Live, raw pty stream with a branch `feat‮rev\u009bx`, author `Eve\x1b]52;c;ZXZpbA==\x07‮X`, subject `fix\x1b[2J\u009b31m\rfake‮TXT`: the stream contains no U+202E, no U+009B, no `ESC]52`; status bar shows `feat<U+202E>rev<U+009B>x`, inline blame `Eve^[]52;c;ZXZpbA==^G<U+202E>X, 3 years ago • fix^[[2J<U+009B>31m^Mfake<U+202E>TXT` (`hostile_screen.txt`). Zero-width/format chars (U+200B..U+200D, U+2060, U+FEFF) pass through unchanged; they are outside SEC-15's list (note only).

## Design QA
Snapshots 120x40 / 80x24 / 40x12 and the live screens match D302: marks in the trailing gutter column, no layout shift, blame dim italic two cells after the line and cut at the edge, `detached f34483a` (`detached_screen.txt`), `main ↑1` at 40 columns. Big file (60k lines, 30 commits): first inline blame 0.35 s after the rest, key echo ~0.1 s (harness granularity), `You, uncommitted` 0.8 s after typing stops (`big_screen.txt`).

## Rule check M7-4a
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to vr-ide UI, one vr-config setting, docs; vr-git untouched |
| BASE-2 | pass | vr-git API used as documented (Request/Reply, InlineBlame, invalidate_path) |
| BASE-3 | pass | notes: compile-red tests before git.rs; setting test not seen red (stated) |
| BASE-4 | pass | test / clippy / fmt / release build output in notes |
| BASE-16 | pass | reuse table; capabilities.md updated; no duplicate sanitizer found |
| BASE-32 | pass | no key binding added; `/git blame line` only |
| SEC-15 | pass | ui/safe_text.rs:18 is used for branch/short SHA (git.rs:869,873), parent path (:378), author + subject (:820,822), watch errors (:504); raw-pty walk shows no BIDI/C1/OSC 52; TestBackend test at 3 sizes |
| SEC-17 | pass (editor part) | answers matched in their own worker's pending map (git.rs:592-596), drawn only when serial == active file's root repo (:220, :227, :790) and version current (:790, :852); `parent repo: ~/x` label (:895-898); popup binding / `/git <root>` n-o (M7-3) |
| SEC-1 (app side) | pass, see minor | workers shut on root removal / replace / exit; discovery thread not covered (minor) |
| SEC-7 | pass | InlineBlame on the automatic timeout; nothing asked over `max_file` (git.rs:744) |
| AC-16 / 16a / 16b / 16d / 16e / AC-1 | pass except 16a after reopen | tests + walk; 16a fails for a file reopened after an outside change (major) |

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/git.rs | read |
| crates/vr-ide/src/ui/safe_text.rs | read |
| crates/vr-ide/src/git_tests.rs | read (test list, shutdown, AC-1 helpers) |
| crates/vr-ide/src/ui/editor_view.rs, ui/status_bar.rs, ui/mod.rs | read |
| crates/vr-ide/src/app.rs, app_workspace.rs, event_loop.rs, main.rs, lib.rs | read |
| crates/vr-ide/src/commands/slash.rs, slash_tests.rs | read |
| crates/vr-ide/src/theme.rs, theme/synthwave.rs | read |
| crates/vr-config/defaults/settings.toml, src/settings.rs, tests/config.rs | read |
| Cargo.toml, crates/vr-ide/Cargo.toml, README.md, docs/commands.md | read |
| Cargo.lock | skipped — lockfile, one internal path dep |
| snapshots/vr_ide__git_tests__*.snap | read (40x12), others skipped — same content wider |
| .ai-dev-team/capabilities.md, 02_tasks.md, decisions.md | skipped — team files |

Artifacts: `qa-artifacts/M7-4a/review/rev.py`, `walk.txt` (16/17), `hostile_screen.txt`, `detached_screen.txt`, `c_open1.txt`, `c_reopen.txt`, `big_screen.txt`.

## Round 1 re-check

**Verdict: APPROVE** (no new findings). Reviewed the round-1 diff in git.rs, app.rs, vr-ide Cargo.toml / Cargo.lock.

- Stale marks/blame (major): `forget_doc` (git.rs:330) drops asked + answered marks, asked/answered blame and the shown inline for the path. Called in `open_path` only when a new document is created (app.rs:670; an already open path just activates, so two tabs on one path cannot exist) and in `close_doc` only when no other tab is on the path (app.rs:766). Save-as: the new path has no entry (a closed doc's entries were forgotten), the old path's entries are cleared on its next open. Fixed.
- Re-discovery (minor): `root_git_changed` reacts only to Created/Removed/Renamed of exactly `<root>/.git` (`.gitignore` etc. ignored). The `unborn_git` flat watch exists only while `.git` is a dir without a found repo, is removed when the repo is found or the root goes. Storm control: `drain_git_events` starts at most one discovery per root per tick, and while the root is `Discovering` it only sets `look_again`; `look_again` is consumed once per finished discovery, so a new discovery needs a new fs event (no self-loop). A `git clone` gives one discovery at a time. Fixed.
- Discovery kill (minor): tid stored by each thread, children read from `/proc/self/task/<tid>/children` only for threads still running (`is_finished` filtered each pass); tid 0 skipped; kill loop retried every 5 ms up to 1 s; `closed` flag stops threads that have not started git. Only the discovery threads' own children are hit (not terminals). Finished-thread race: a tid could in theory be reused between `is_finished` and the read; needs a full pid wrap in microseconds, negligible. Non-Linux: `current_tid` returns 0, so no-op. Fixed.
- rustix: `rustix = { workspace = true, features = ["process"] }` (workspace 1.1.6, already in Cargo.lock); no new version. OK.
- D304 note: `repo_of_root` documented read-only, test carries the note; matches decision D304 (writes use `vr_git::Writer::open`).
- Spot-run: `cargo test -p vr-ide git_tests` 24 passed, 0 failed. `rediscover.py` on target/release/vr-ide: 4/4 PASS; no vr-ide process left (pgrep empty).

## Rule check M7-4a
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | round-1 changes limited to the four findings |
| BASE-3 | pass | notes list tests red first; git_tests 24/24 green here |
| BASE-4 | pass | test output in notes, re-run here |
| BASE-16 | pass | reuses the explorer's root watch and GitState watcher |
| SEC-1 (app side) | pass | discovery children killed at exit (test a_hung_discovery_git_is_killed_at_exit) |
| SEC-17 | pass | forget_doc stops an earlier document's answers standing for a reopened one |
| AC-16a | pass | reopen test + walk |
| COST-9 | pass | no process left |
