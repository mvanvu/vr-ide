# W-1 Workspace core (multi-root model, CLI, .vr-workspace, recent, index/search/watch across roots)

Worktree: .worktrees/W-1, branch w-1-workspace. Base commit: 9c45cd1 (the brief's 96200e5 is a tree hash, not a commit; HEAD of the worktree = master 9c45cd1).

## Progress
- read brief-common, spec 01_spec_W.md, implementation skill
- read code: app.rs, cli.rs, main.rs, explorer/search/palette/terminal/recovery sites of `app.root`; vr-fs FileIndex, vr-search project/replace
- baseline build of the 4 crates (own target/) ok
- red vr-config: recent::recent_list_is_bounded_newest_first_and_deduplicated, recent::recent_list_round_trips_and_survives_a_corrupt_file, workspace_file::{workspace_file_round_trips_relative_inside_absolute_outside, workspace_file_reads_dot_dot_and_refuses_bad_files, read_workspace_reports_a_missing_file} — 5 FAILED (panicked at the stubbed bodies, recent.rs:95 / workspace_file.rs:45)
- green vr-config: 8 passed (unit) — workspace_file.rs, recent.rs added
- red vr-fs: file_index::tests::file_index_over_two_roots_names_each_root_and_keeps_its_gitignore FAILED (panicked at the stubbed build_roots, file_index.rs:49); green: 5 passed (FileIndex::build_roots / absolute, walk split into walk_roots + walk_one)
- red vr-search: project::tests::search_roots_walks_each_root_in_order_with_its_own_gitignore FAILED (stub panicked, project.rs:94); green after run_search took a root list (then a wrong file count in my own assert, 53 -> 54, fixed in the test)
- red vr-search: replace::tests::plan_replace_roots_confines_each_file_to_its_own_root FAILED (stub panicked, replace.rs:118); green: vr-search 39 passed + 2 (racing_writer) passed
- red vr-ide workspace:: (4 tests: names/disambiguation, nested/duplicate/linked refused, display_path/active root, from_paths/from_file round trip) — 4 FAILED with check()/rename() stubbed; green 4 passed (crates/vr-ide/src/workspace.rs)
- red cli: cli::tests::cli_opens_each_folder_as_a_root FAILED (old one-argument rule kept as a stub, cli.rs:197); green 4 passed. cli_opens_a_workspace_file_alone was green on its first run (single argument; the parsing behind it was red-first in vr-config / workspace::tests)
- wired App: `root` field -> `workspace: Workspace` (+ `root()`, `active_root()`), title/label, display_path, resolve, watches per root, index build_roots, search_roots, plan_replace_roots, terminal cwd, palette resolve via index.absolute, explorer uses explorer.root(); 5 test lines `app.root` -> `app.root()` (field became a method)
- red workspace_tests (8): go_to_file / search+replace FAILED with the wiring cut back to the first root (assert left==right at workspace_tests.rs:134 / :182); add/remove/open/save FAILED at the todo!() stubs of app_workspace.rs; recovery FAILED (names without the root, workspace_tests.rs:414)
- green workspace_tests 8 passed after app_workspace.rs (add_root/remove_root/open_workspace/answer_open_workspace/save_workspace/remember_workspace/roots_changed), Explorer::rerooted, DialogAction::OpenWorkspace, search::roots_changed, recovery Entry.root + publish_in / write_docs_in. Two test fixes on the way: focus (typing went into the search field) and the replace flow (ctrl+shift+h from the start)
- red terminal_tests::a_new_terminal_starts_in_the_root_of_the_active_file FAILED with cwd cut back to the first root (screen "/tmp/.../rootA"); green with cwd = active_root()
- added new-state tests: open_workspace_switches_nothing_when_unsaved_work_cannot_be_kept, roots_do_not_change_while_a_project_replace_writes (green on the first run: they guard paths that reuse keep_discarded / save_doc / quit_block)
- guard: clippy found 3 (is_multiple_of in file_index.rs, a match on a block in palette.rs, clone-to-slice in a cli test), fixed; fmt clean
- release bench + 3 roots x 5k files bench (private VR_IDE_STATE_DIR) done
- decisions.md: 9 `D? (W-1)` lines appended; capabilities.md: 4 lines appended

## Files changed
New: crates/vr-ide/src/workspace.rs, crates/vr-ide/src/app_workspace.rs, crates/vr-ide/src/workspace_tests.rs, crates/vr-config/src/workspace_file.rs, crates/vr-config/src/recent.rs

Changed (vr-ide): app.rs (field `root` -> `workspace`, `with_workspace`, `load_workspace`, `root()`, `active_root()`, title, resolve, display_path, watches per root, index over roots, recovery write), cli.rs (several arguments, workspace file), main.rs, lib.rs, event_loop.rs (recovery publish_in / write_docs_in), recovery.rs (Entry.root, `<root>+<path>` names), commands/dialog.rs (OpenWorkspace), commands/explorer.rs (explorer.root() instead of app.root; `Explorer::rerooted`), commands/palette.rs (index.absolute when chosen), commands/search.rs (search_roots, plan_replace_roots, roots_changed), commands/terminal.rs (cwd = active root), ui/search_panel.rs (root names in file labels), ui/status_bar.rs + ui/tabs.rs (workspace label when no file), ui/welcome.rs (2 one-token edits `app.root` -> `app.root()`; no key display code touched), terminal_tests.rs (+1 test), explorer_tests.rs / search_tests.rs (5 lines `app.root` -> `app.root()`: the field became a method).

Additive only: vr-fs file_index.rs (`build_roots`, `absolute`; walk split, `build` delegates with one root, same output), vr-search project.rs (`search_roots`; run_search takes a root list, `search_project` = one root, same messages), replace.rs (`plan_replace_roots`; `plan_replace` = one root, same errors), re-exports in vr-search/vr-config lib.rs.

Not touched: keymap.toml, key display code, palette command list (T18), docs, Cargo manifests.

## API summary for W-2
- `crate::workspace::Workspace` (`app.workspace`): `roots() -> &[Root]` (order = explorer order), `primary()`, `get(RootId)`, `len()` / `is_multi()`, `paths()`, `root_of(path) -> Option<&Root>`, `active_root(file)`, `display_path(path)` (one root: relative as before; several: `name/rel`; outside every root: full path), `label()` (title, status bar and tab bar when no file), `file()` (the .vr-workspace), `recent_entry()`. `Root { id: RootId, path, name }`, `Root::missing()` (folder gone; listed and removable), `Root::branch()` (git branch, read on demand).
- `WorkspaceError` (Display = notice text): NotFound, NotADirectory, Io, Duplicate{path, root}, Nested{path, root}, Contains{path, root}, LastRoot, UnknownRoot, File(msg). Spec: Nested/Contains as a notice; Duplicate may be ignored silently.
- `App::add_root(&Path) -> Result<RootId, WorkspaceError>`: live; watches, index (rebuilt if one was built or building), shown search rerun.
- `App::remove_root(RootId) -> Result<Root, WorkspaceError>`: closes nothing, touches nothing on disk; first root removed -> explorer re-rooted on the next one.
- `App::open_workspace(Workspace)`: build it with `Workspace::from_file(path)` or `Workspace::from_paths(&[..])` (both return refused folders as errors for a notice). Asks Save all / Discard / Cancel (DialogAction::OpenWorkspace) when buffers are modified, closes all tabs, keeps terminals, records in recent; refused with a warning while a project replace writes.
- `App::save_workspace(path, Option<&DiskVersion>) -> Result<(), String>`: None creates and refuses an existing file ("... exists; it is replaced only when confirmed"); Some(seen) replaces only that version (take `vr_core::DiskVersion::of_path(path)` when the Replace prompt is shown). Sets `workspace.file`, records in recent.
- Recent: `app.recent_workspaces.entries: Vec<RecentWorkspace{file: Option<PathBuf>, roots}>`, newest first, max 10; `App::remember_workspace()`; `app.recent_workspaces_file` (state_dir/recent.toml from App::load_workspace, None in tests). Open Recent = `Workspace::from_file(file)` or `from_paths(&roots)` then `open_workspace`.
- `App::root()` = first root path; `App::active_root()` = root of the active file, else the first (terminal cwd uses it; W-2's root pick for New Terminal can set `SpawnOptions.cwd` from the chosen Root).
- Explorer: still one tree on `app.root()`; `sync_explorer_watches` already watches every root folder plus the expanded folders, so W-2's root nodes only add their expanded sets.
- CLI: `cli::Launch { workspace, open_files, notices }`.

## Deviations / notes
- The brief's task-base `96200e5` is a tree hash; the worktree base is master 9c45cd1.
- Go-to-file items keep the index path (root-relative, or `name/rel`) in `PaletteItem.path`; `accept` maps it with `FileIndex::absolute` (keeps the explorer_tests palette assertion unchanged).
- `Launch` shape changed (`root`/`open_file` -> `workspace`/`open_files`); the old test's "more than one argument -> usage error" assertion is gone because several arguments are now the feature; its other assertions kept (as `roots(&l)`).
- Out of scope noticed: add_root, Root::missing and the CLI call metadata/canonicalize on the calling (UI) thread, like today's Open File; a hung network mount blocks there (W-2's Add Folder prompt may want a worker).

## New states (round 0)
| State | quit | cancel/Esc | error halfway | input changes | second request | concurrent writer | hung call | test |
|---|---|---|---|---|---|---|---|---|
| Root set changing live (add/remove) | nothing pending | n-a (no dialog in W-1) | validated first; on Err nothing changes | running index build: receiver dropped (walk stops), new build; running search: cancelled and rerun; preview: dropped; replace writing: untouched | duplicate/nested refused; unknown id -> UnknownRoot; last root kept | no file writes | metadata/canonicalize of the added path on the UI thread (noted above) | add_and_remove_root_update_index_search_and_watches_live, add_root_refuses_nested_and_duplicate_folders_and_the_last_root_stays, removing_the_first_root_moves_the_explorer_to_the_next |
| Open Workspace question (DialogAction::OpenWorkspace) | modal | Cancel/Esc: nothing changes | a failed save -> no switch; a failed recovery write -> nothing discarded, no switch | target fixed when asked | modal | saves use the freshness-checked save | recovery/save on the UI thread, as quit | open_workspace_asks_for_unsaved_files_then_replaces_the_roots, open_workspace_switches_nothing_when_unsaved_work_cannot_be_kept, roots_do_not_change_while_a_project_replace_writes |
| save_workspace write | n-a (synchronous) | n-a | temp file never placed on error (vr-core) | n-a | each call re-checks | refused when the file changed since `seen` or appeared since None | FIFO/device target refused by vr-core before any read | save_workspace_round_trips_and_never_overwrites_an_unseen_version |
| recent.toml | written at startup / open / save | n-a | write error -> warning, list kept in memory | n-a | re-read before each write (another instance's entries kept) | temp + rename: never half-written; at worst the later writer drops one entry | state dir on a hung mount blocks like recovery | recent_list_round_trips_and_survives_a_corrupt_file |
| Index over roots (`name/rel`) | detached walk ends with its receiver | n-a | unreadable root skipped; all unreadable -> Error | roots change -> rebuilt | a running build is kept (start_index) | n-a | background thread | file_index_over_two_roots_names_each_root_and_keeps_its_gitignore, go_to_file_lists_every_root_by_name_and_opens_from_the_second |

## Commands run + result (trimmed)
- `run-heavy.sh cargo test -p vr-ide -p vr-search -p vr-fs -p vr-config -j 4`: vr-config 8 + 18 + 8 passed; vr-fs 44 passed (1 ignored) + racing_writer 2; vr-ide 371 passed (4 ignored); vr-search 39 passed (1 ignored) + racing_writer 2. After the clippy fixes, `cargo test -p vr-ide -p vr-fs`: 371 / 44 / 2 passed; then `workspace_tests` 10 passed (2 new-state tests added).
- `cargo fmt --all --check`: clean. `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`: `Finished dev profile`, no warnings.
- `VR_IDE_STATE_DIR=<private> run-heavy.sh scripts/bench-startup.sh` (release): first_frame_ms=23, max_rss_mb=6, rust_200kb_first_frame_ms=23, rss after colours 18 MB, welcome 8 MB.
- `qa-artifacts/W-1/bench-roots.sh` (3 roots x 5,000 files, `vr-ide r1 r2 r3`): first frame 23 / 23 / 24 ms, RSS 6 MB; max RSS after 4 s with the 15k-file index built: 8 MB. The user's ~/.local/state/vr-ide was not written.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| freshness-checked write of the .vr-workspace file | capabilities.md (vr-core Document) | reused `Document::untitled` + `save_as_seen` (D87) |
| unsaved-files question | commands/app.rs quit | reused SAVE_ALL / DISCARD / CANCEL, `save_doc`, `App::keep_discarded` |
| replace-running check | commands/search.rs | reused `quit_block` |
| TOML and state paths | vr-config | new workspace_file.rs / recent.rs there (vr-config already owns toml + state_dir; no new dependency) |
| index / search / replace per root | vr-fs, vr-search | extended additively (build_roots, search_roots, plan_replace_roots) |
| git branch | monitor::git_branch | reused in Root::branch |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | workspace core only; no explorer UI, commands, welcome lists, keymap (Files changed) |
| BASE-2 | pass | brief's API shapes (add_root -> Result, remove_root(id), open_workspace, save_workspace); additive helpers listed in the API summary |
| BASE-3 | pass | red line per test in Progress; first-run greens named (cli_opens_a_workspace_file_alone, 2 new-state tests) |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | tests, clippy, release bench through run-heavy.sh |
| BASE-6 | pass | English, why-comments |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | tests of the 4 named crates only |
| BASE-11 | pass | own worktree; no history-rewriting git commands |
| BASE-14 | pass | long runs as background tasks |
| BASE-15 | pass | nothing killed; test shells /bin/sh with ENV=/dev/null (use_sh) |
| BASE-16 | pass | Reuse table; capabilities.md appended |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file, progress lines |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | fmt clean, clippy -D warnings clean |
| BASE-32 | pass | D226 / spec W; D109 (explorer confinement unchanged on its root; replace confined per root), D110/D115 (non-recursive per folder, per root), D119-D129 (search/replace semantics kept, plan per root, apply re-checks the root), D185/D205-D219 (terminal unchanged except cwd) |
| BASE-38 | pass | rustfmt only on changed files |
| COST | pass | related tests only, one release build, throwaway scripts in qa-artifacts/W-1, bench state private and removed, nothing left running |
| new-states rule | pass | New states table + tests |
| AC-W1 | pass (core) | go_to_file_lists_every_root_by_name_and_opens_from_the_second (index, `web/src/main.rs` status name); explorer roots are W-2 |
| AC-W2 | pass | project_search_and_replace_cover_both_roots_each_with_its_own_gitignore, search_roots_walks_each_root_in_order_with_its_own_gitignore, plan_replace_roots_confines_each_file_to_its_own_root |
| AC-W4 | pass (core) | workspace_file tests, from_paths_keeps_missing_roots_and_reports_refused_ones, save_workspace_round_trips_and_never_overwrites_an_unseen_version, open_workspace_asks_for_unsaved_files_then_replaces_the_roots, recent tests; palette / welcome entries are W-2 |
| AC-1 | pass | one root: first frame 23 ms, 6 MB; 3 roots x 5k files: 23-24 ms, 8 MB with the index |
