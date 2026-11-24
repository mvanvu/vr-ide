# Review W-1 — workspace core (round 0)

Verdict: **APPROVED** (minor findings only)

## Findings

[minor] crates/vr-ide/src/commands/search.rs:529 (`roots_changed`) — results of the previous workspace stay after Open Workspace
Scenario: search "foo" shown, the user edits the query field to "bar" without pressing Enter, then opens another workspace. `shown != fields`, so nothing reruns: the panel keeps listing (and a running walk keeps streaming) files of the old workspace. Enter on a row opens a file outside every root. No write is possible: a preview is always dropped by `fields_changed` before the fields can differ from the shown query, and confirm checks `is_current`. This follows the letter of D247, which was written for add/remove. After a full switch, though, the list belongs to another project.
Fix: in `switch_workspace`, when no rerun happens, cancel the running search and clear the groups, or leave them and add a "results are for the previous workspace" line. Leader decides.

[minor] crates/vr-ide/src/workspace.rs:229 (`active_root`) — a missing first root is still the fallback
Scenario: a `.vr-workspace` file with `roots = ["gone", "api"]`. With no active file, a new terminal spawns with `cwd = gone` and fails, and `resolve("x")` points into a folder that does not exist. Uncertain: I did not check how the spawn error is shown.
Fix: fall back to the first root that is not `missing()`, and the primary only when every root is missing.

[minor] crates/vr-config/src/workspace_file.rs:81 and recent.rs:101 — non-UTF-8 root paths
Scenario: the root `/data/caf\xe9`. `to_string_lossy` writes `caf\u{FFFD}`, so on reopen that root comes back as "missing" with no warning. For the same root, `toml::to_string` of the recent list fails, so every `remember_workspace` shows the "cannot write" warning.
Fix: `save_workspace` refuses with a message naming the root; `remember_workspace` skips that entry.

[minor] crates/vr-config/src/workspace_file.rs:72 — the base folder is not canonicalized
Scenario: saving to `/home/u/link/team.vr-workspace` (where `link` points to `/data/p`) with roots `/data/p/api`. The roots are canonical but the base is not, so `strip_prefix` fails and the file holds absolute paths. It stops being portable, against the module doc.
Fix: canonicalize `file.parent()` in `save_workspace` before formatting, or in `format_workspace` when it exists.

[minor] crates/vr-config/src/workspace_file.rs:34 — `read_workspace` reads FIFOs and unbounded files
Scenario: `x.vr-workspace` is a FIFO. `read_to_string` blocks forever. That is harmless at CLI time (Ctrl-C works), but it is a stuck UI thread once W-2's Open / Open Recent calls `Workspace::from_file`.
Fix: open through a regular-file check plus a size cap (the D129 `open_regular` pattern), or note it in W-2's brief.

[minor] UI-thread grade — `Workspace::add` (metadata and canonicalize), `check` (canonicalize per root) and `Root::missing` (stat) all run on the caller's thread. That is acceptable for W-1: there is no UI caller yet, and the CLI runs before the TUI. W-2 must not call `missing()` per frame in the explorer render, and should run Add Folder's checks off the UI thread or accept a hung mount as the notes say. Carry to W-2's brief.

Verified with no finding:
- every former `app.root` site: explorer `ensure_within` on `explorer.root()` (D109; one tree until W-2), watches per root with removed roots unwatched, the index restarted on change, recovery names, terminal cwd, status bar, tabs and title
- `plan_replace_roots`: owner taken by the canonical root, `apply_replace` re-checks `preview.root`, single-root errors unchanged
- search: dir links not followed, and a file link whose target is inside any root is skipped, so no file is listed or replaced twice; per-root `.gitignore` with `parents(false)`
- `save_workspace`: the `None` path refuses an existing file or dangling link, and the absent-version check refuses a racing create
- recent list: bounded to 10, written temp+rename, a corrupt file starts fresh, only `state_dir()` is written
- CLI: mixed, duplicate, nested and missing arguments
- one root: index, display, search and replace output are the same as before

## Files read
- crates/vr-config/src/lib.rs, recent.rs, workspace_file.rs — read
- crates/vr-fs/src/file_index.rs — read (diff)
- crates/vr-search/src/lib.rs, project.rs, replace.rs — read (diff + `visit`/`walk_builder`)
- crates/vr-ide/src/workspace.rs, app_workspace.rs — read
- crates/vr-ide/src/workspace_tests.rs — read (168–497; helpers skimmed)
- crates/vr-ide/src/app.rs, cli.rs, main.rs, lib.rs, event_loop.rs, recovery.rs — read (diff)
- crates/vr-ide/src/commands/{dialog,explorer,palette,search,terminal}.rs — read (diff + search.rs preview/fields_changed/is_current)
- crates/vr-ide/src/ui/{search_panel,status_bar,tabs,welcome}.rs — read (diff)
- crates/vr-ide/src/{explorer,search,terminal}_tests.rs — read (diff)

## Rule check W-1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | vr-fs/vr-search public API additive only (`build_roots`, `absolute`, `search_roots`, `plan_replace_roots`); `walk`/`inside` private; no explorer UI, keymap or palette commands |
| BASE-2 | pass | `add_root -> Result<RootId,_>`, `remove_root(RootId)`, `open_workspace`, `save_workspace(path, Option<&DiskVersion>)` match the brief |
| BASE-3 | pass | notes list a red line per test; the first-run greens are named and guard reused paths |
| BASE-6 | pass | English, why-comments |
| BASE-25 | pass | notes: fmt clean, clippy -D warnings clean (not rerun, per the reviewer rules) |
| BASE-32 | pass | compared against D226, D240–D248, D119–D129, D64/D87 (save_as_seen), D109, D110 |
| new-states | pass | notes table covers add/remove, the Open Workspace question, save, recent and index; tests named per row |
| AC-W1 / AC-W2 / AC-W4 (core) / AC-1 | pass | workspace_tests, vr-search/vr-fs unit tests, bench in notes |
