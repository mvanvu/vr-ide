# Review W-2 — workspace UI (round 0)

Verdict: **CHANGES_REQUESTED** (1 major, 3 minor; security: no high/critical)

## Findings

[major] crates/vr-ide/src/app_workspace.rs:62, :81-101, :167 — a slow workspace job is dropped without a word, or applied over whatever the user is doing; neither path has a test
Scenario A (dropped): `/ws open /mnt/nfs/team.vr-workspace` takes more than 300 ms. The user sees "Open Workspace …: waiting for the file system; it is applied when it answers". They then run `/ws add ~/lib`, which answers at once. `run_ws_job` sets `self.ws_job = None` (line 62), so the promised Open never happens and no notice says it was cancelled.
Scenario B (applied late): the same Open answers after 10 s. Meanwhile the user opened and edited a file and has a Rename prompt (or a close-tab Save/Discard question) open. `drain_ws_job` → `finish_open` → `open_workspace` → `open_dialog` replaces that dialog and closes the F1 window. The typed name or the pending close is lost, and an "Open Workspace: unsaved files" question appears that the user did not just ask for. With no modified file, every tab closes under the open dialog. Nothing is written, because the confinement refuses the old dialog's paths, but the user loses their action with no explanation.
The new-states table lists "second request: replaces the pending job" with no test, and no test covers a late result arriving while a dialog is open.
Fix: (1) when `run_ws_job` replaces a pending job, post an info notice ("Open Workspace … cancelled by Add Folder …"). (2) In `drain_ws_job`, keep the result while `app.dialog.is_some()` (or the palette is open) and apply it once the dialog closes. Alternatively, apply an Open only after a confirm ("Open Workspace team is ready — Open / Cancel"). (3) Add tests with `ws_job_wait = ZERO` for: replace → notice; late Open while a Rename prompt is open → the prompt is kept.

[minor] crates/vr-ide/src/commands/explorer.rs:1237, :1365 — a cut+paste or Move into a folder link that points into another root moves the file across roots
Scenario: the review walk (qa-artifacts/W-2/review/walk_05.txt). Root `api` holds `link -> ../web/sub`. Cut `api/x.txt` and paste it on `api/link`. The file now sits on disk in `web/sub/x.txt` ("moved api/link/x.txt"). `dest` and `dest_root` are lexical, and only the source's real parent is checked (`below_root`). AC-W5 / D283 say moves between roots are refused. Nothing is lost, and this is the D109 class (paste into a linked folder is allowed; with one root the same paste moves a file out of the project).
Fix (leader / user decides, D109 reading): in `move_paths`, canonicalize `dest` and refuse when it does not start with the real path of `from`'s root (same message as `cross_root`). The cost is one `canonicalize` per batch. Copy stays allowed.

[minor] crates/vr-ide/src/app.rs:486 (`sync_explorer_watches`) + commands/explorer.rs:443 (`load_root`) — a missing root raises two startup warnings that hide later errors
Scenario: `vr-ide team.vr-workspace` with `gone` missing. The notice line shows "explorer: cannot watch …/gone: No such file or directory … (+3 more · Esc)" next to "the workspace folder …/gone is missing". In the walk, the later cut+paste refusal ("moving between workspace folders is refused") showed only after 3 presses of Esc (walk_03.txt). The user sees a stale watch error instead of the answer to the action they just took.
Fix: skip `watch_dir` for roots marked `missing` (the explorer already knows), or fold the watch error into the single "missing" warning.

[minor] README.md:93-100, :241-251 — README not updated for several roots
Scenario: README still says "vr-ide takes at most one argument" and "the project root is the folder the explorer shows". It also says the Move prompt is "relative to the project root", and the right-click paragraph does not list "Remove Folder from Workspace", the `root-name/path` Move form or the refusal of moves between folders. docs/keymap.md and docs/commands.md are correct.
Fix: one short "Workspace (several folders)" paragraph plus edits to those 3 sentences.

Verified with no finding:
- Per-root confinement. Every op goes through `root_for` plus `ensure_within` or `below_root` of its own root. Absolute or `../web` typed Move targets are refused. A multi-root selection is refused per path. Cut+paste onto another root is refused and the file stays (walk_03). Copy works (walk_04). Trash, rename and moves from inside a link to another root are refused by `below_root`. Stale dialog paths of a removed root fall back to the first root and are refused.
- Root removal: `set_roots` only drops UI state (expanded, children, selection, clipboard). Tabs and files are untouched (unit test + dev walk 08). Removing the missing root works (walk_07). The last root has no Remove item.
- Missing roots: learned only on listing; `active_root` skips them and stats only on actions, never per frame. Terminal and resolve are covered.
- `read_workspace`: stat before open and fstat after (FIFO, folder, device refused), a `take(1 MiB + 1)` cap, a UTF-8 check, and the call made on a worker. Non-UTF-8 paths: Add refuses them, save refuses, recent skips with a notice.
- Welcome: `root_info` is read only with monitor samples (welcome drawn, several roots) and file counts once per index. No stat or git read per frame. The welcome click maps to `recent_workspaces.entries`, and the numbers match `/ws recent`.
- Terminal: from the palette, several roots → `/term new ` list with the active root highlighted (walk_08/09: pwd = web). The key and `/term new` without a root keep the active root.
- Palette `workspace.*` commands open `/ws …`. No default key is added anywhere: vr-config keymap is untouched, and the menu item has no key (USER-5). D280's unbinding is T24's.
- Nested Add Folder is refused: "…/api/inner is inside the workspace folder api; nested folders are not added" (walk_06).
- Snapshots at 120x40 / 80x24 / 40x12 (explorer and welcome) are consistent. The missing marker stays when the name is cut ("✗ do… (missing)"). Compact welcome shows "3 folders" (D286). The palette snapshot changes are only the new Workspace rows.

## Live pty walk (release binary 13:28, newer than every source file)
`python3 -I .ai-dev-team/workspace/qa-artifacts/W-2/review/walk.py`: 9/9. Three roots come from a `.vr-workspace` file (`api`, `web`, `gone` missing): welcome PROJECT with "missing", explorer "✗ gone (missing)", cross-root cut refused, copy works, probe of the link move (finding 2), nested Add refused, Remove of the missing root, palette terminal root pick → pwd `/web`. Screens: qa-artifacts/W-2/review/walk_01..09.txt. The dev walk with two roots (qa-artifacts/W-2/dev/walk_01..09.txt) was reused. Nothing is left running.

## Files read
- crates/vr-ide/src/commands/explorer.rs — read (diff + ops section 860–1560, mouse/keys)
- crates/vr-ide/src/commands/{explorer_menu,palette,registry,mod,search,slash,terminal}.rs — read (diff; slash candidates/prepare/set_input)
- crates/vr-ide/src/commands/workspace.rs — read
- crates/vr-ide/src/app.rs, app_workspace.rs, event_loop.rs, workspace.rs, lib.rs — read (diff + open_workspace / switch / dialogs)
- crates/vr-ide/src/ui/{welcome,explorer,editor_view}.rs — read (diff)
- crates/vr-config/src/{workspace_file,recent,lib}.rs — read
- crates/vr-ide/src/workspace_ui_tests.rs — read (test list, job/FIFO tests); ui/tests/welcome_tests.rs, slash_tests.rs — diff skimmed
- 6 new + 2 changed .snap files — read
- docs/commands.md, docs/keymap.md, README.md (explorer/root sections), capabilities.md, decisions D240–D248, D281–D289 — read
- vr-fs ops.rs `ensure_within` — read (callee)

## Rule check W-2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | W-2 scope; vr-config change limited to the read_workspace guard + `same` pub |
| BASE-2 | pass | W-1 API kept (`add_root`, `remove_root`, `open_workspace`, `save_workspace`); `rerooted` removal recorded in capabilities.md |
| BASE-3 | pass | red line per test in notes; code-first tests named as deviations |
| BASE-4 / BASE-5 | pass | notes: suite, clippy, release via run-heavy.sh (full-suite.log, release.log present) |
| BASE-6 | pass | English, why-comments |
| BASE-8 | n-o | no manifest change (no audit run) |
| BASE-16 | pass | reuses slash ArgKind::Root/Path, vr_fs::Tree, explorer_menu, monitor::git_branch |
| BASE-25 | pass | notes: fmt + clippy -D warnings clean (not rerun) |
| BASE-32 | pass | D240–D248, D109, D241, D262/D276, no new keys (USER-5); D280 left to T24 |
| new-states | fail | WsJob row: "second request replaces" has no test, and a late result over an open dialog is untested → finding 1 |
| COST | pass | scripts under qa-artifacts, private config/state dirs, nothing left running |
| AC-W1 | pass | explorer roots, status bar `web/src/app.ts`, walk |
| AC-W2 (UI) | pass | open_workspace_drops_results_of_the_old_roots; per-root gitignore test |
| AC-W3 | pass | Add live, Remove leaves files and tabs, missing root removable (walk_07) |
| AC-W4 (UI) | pass | palette → /ws, welcome WORKSPACES click, linked-folder save |
| AC-W5 | pass (minor gap) | direct cross-root move refused; via folder link it is not → finding 2 |
| AC-W6 | pass | walk pwd = active file's root |

## Round 1 re-check

Verdict: **APPROVED** (all 4 findings fixed; 0 blocker/major; 1 nit below)

- F1 (major) fixed. `WsJob.held` keeps the answer while `dialog` or `palette` is open; it is applied once, by `ws_job.take()`, so no double apply. A replaced pending job posts "<old> cancelled by <new>". Tests `a_replaced_workspace_job_is_named_in_a_notice` and `a_late_workspace_answer_waits_for_the_open_dialog` are meaningful: they use a channel-blocked job, check the typed Rename text is kept and the workspace is unchanged while the dialog or palette is open, then check the apply after Enter or Esc.
- F2 fixed. `really_in_another_root` canonicalizes `dest` once per batch in `move_paths`. It refuses when the real path is in another root that is not missing. An unresolvable `dest` or a link to outside every root falls to the old checks (D109 unchanged). Test covers cut+paste and the Move prompt `api/link` (file stays, message shown) and copy into the link (works). Nit: a same-root link move has no test of its own; the logic (`r.path != root`) allows it.
- F3 fixed. `sync_explorer_watches` skips roots marked missing. With several roots, one stat per root about to be watched marks a missing root and posts the single notice; `load_root` skips the notice if already named. This is not per frame (only roots not yet watched). Tests `a_missing_root_is_not_watched` and the welcome-screen ordering variant assert one "gone" notice and no watch entry.
- F4 fixed. README has a "Workspace (several folders)" section, the "at most one argument" sentence is gone, and the Move prompt and "Remove Folder from Workspace" wording is correct. No key proposed or added (D292, USER-5).

Spot-run: `cargo test -p vr-ide workspace_ui_tests` gave 19 passed, 0 failed. `qa-artifacts/W-2/dev-r1/walk.py` on target/release/vr-ide (14:03, newer than every .rs file) gave 10/10 PASS, including "no watch error" at startup and the link move refused. Nothing left running.

## Files read (round 1)
- crates/vr-ide/src/app_workspace.rs — read (diff)
- crates/vr-ide/src/app.rs — read (sync_explorer_watches diff)
- crates/vr-ide/src/commands/explorer.rs — read (really_in_another_root, move_paths)
- crates/vr-ide/src/workspace_ui_tests.rs — read (5 new tests, held_job)
- README.md — read (diff)

## Rule check W-2
| Rule | Result | Evidence |
|---|---|---|
| BASE-3 | pass | red/green per finding in notes; tests above check behavior |
| BASE-4 / BASE-5 | pass | notes: clippy, full suite 922 passed, release build; light test and walk rerun here |
| BASE-32 | pass | D292 applied; D109 kept; no key binding added |
| new-states | pass | WsJob replace notice + held answer (dialog, palette) tested |
| AC-W3 | pass | missing root named once, not watched (2 tests, walk) |
| AC-W5 | pass | move through a folder link into another root refused; copy allowed |
| COST | pass | walk in private config/state dirs |
