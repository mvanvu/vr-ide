# Review T8 — Explorer panel + go-to-file

Verdict: **CHANGES T8** (2 major, 6 minor, 1 uncertain)

Tools used: Read, Bash (git diff/status, grep, cargo test/clippy/fmt --check once). The test and lint runs I did: `cargo test -p vr-ide` → 98 passed, 0 failed, 2 ignored. `cargo clippy -p vr-ide --all-targets -- -D warnings` → clean (only the vr-search manifest note). `cargo fmt --all --check` → rc=0.

## Findings

[major] crates/vr-ide/src/app.rs:296-308, crates/vr-ide/src/main.rs:89 — the recursive `watch_root` runs on the UI thread and watches every folder, ignored ones included (focus e)
Scenario: the project has node_modules / target / vendor. `Watcher::watch(root)` → notify inotify `RecursiveMode::Recursive` adds one watch per directory, and notify-debouncer-full's `FileIdMap::add_path` walks every entry with `follow_links(true)`. Both run synchronously before `event_loop::run`, so keys wait until the walk ends. Real trees here: vr-ide/target has 2,931 dirs and 17,901 entries; referral-frontend/node_modules has 13,325 dirs and 134,951 entries. That means 13k inotify watches out of the user-wide 65,536, which other tools (VS Code, webpack, cargo-watch) share and then hit ENOSPC on. It also keeps 135k cached paths, which goes against the 60 MB idle RSS target (C3), and on every `npm install` / `cargo build` each new directory gets another recursive cache walk. A symlink to $HOME inside the project gets walked too (notify follows links; D17 says the index never does). This breaks spec NFR "no blocking I/O on the UI thread (… file watch run off-thread)".
Fix: excluding ignored directories is not enough. notify's recursive mode cannot exclude paths, and a large non-ignored tree still costs O(dirs). It needs a vr-core change: a non-recursive directory watch (`RecursiveMode::NonRecursive` dirs in `Scope`). The explorer then watches the root plus each expanded folder, and unwatches on collapse. That matches the lazy tree and costs O(expanded) watches with depth-1 cache walks. Go-to-file can keep `index_stale` from those events and also rebuild in the background each time the palette opens (the walk is already ignore-aware and off-thread). Leader decision: vr-core is outside T8's file scope.

[major] crates/vr-ide/src/ui/explorer.rs:25-30 with commands/explorer.rs:459-466 — wheel scrolling stops working
Scenario: a long tree, cursor at row 10, view of 20 rows. ScrollDown → `top` += 3, then render runs "keep the cursor visible" and resets `top` to the cursor once the cursor row would leave the view. So the wheel can never scroll the tree past the cursor. The editor avoids this with `follow_cursor = false`, and the explorer has no such flag. Also, the event loop handles queued events before redrawing, so a wheel tick followed by a click in the same batch computes `i` from the new `top` while the screen still shows the old rows. The click then selects or opens a different row than the one clicked.
Fix: add a `follow_cursor` flag (cleared by the wheel, set by keys and clicks) that render honours. For hit tests, use the `top` drawn in the last frame (store it in render). Add a test that scrolls past the cursor.

[minor] crates/vr-ide/src/commands/explorer.rs:414-419 — explorer swallows unmodified keys before the keymap (focus h)
Scenario: a user binds `[explorer] "space" = "explorer.reveal"` (or a letter, tab or backspace). The config validates, but the built-in arm returns `true` first, so the binding never runs (USER-1: command keys live in the keymap).
Fix: swallow these keys only when `keymap.command_for_in(Context::Explorer, chord)` has nothing for them.

[minor / uncertain] keymap and D103 — VS Code's default explorer `ctrl+x` (filesExplorer.cut) is missing
Scenario: VS Code binds ctrl+x/ctrl+c/ctrl+v to cut/copy/paste in its explorer on Linux as well (when filesExplorerFocus). D103 says there is no cut key. Here ctrl+x in the explorer does nothing, and a move needs the palette plus a typed path. USER-1 is a VS Code default-key rule.
Fix: leader call. Either add `explorer.cut` (paste then = `ops::move_to`, same per-path batch) or record the deviation.

[minor] crates/vr-ide/src/app.rs:904 (drain_index → palette::refilter) — index arrival resets the go-to-file selection
Scenario: ctrl+p with a stale index → the user types and presses Down twice while the rebuild finishes. `refilter` sets `selected = 0`, and Enter opens a different file than the one that was highlighted.
Fix: when refilter is triggered by drain_index, keep the selection by `path` (fall back to 0).

[minor] crates/vr-ide/src/explorer_tests.rs:60-62 — dead statements in `explorer_shows_root_tree` (`let _: PathBuf = …`, `let _ = key(KeyCode::Down)`). Remove them.

[minor] (focus g) — `test_app` hides the explorer. The T5/T6 tests still cover what they claim, because the layout with the panel hidden is identical to before. No test covers the default (panel shown) layout for editor click-to-position, drag and tab click: `char_at_screen` and `tabs::hit` are offset-aware (`area.x`, `l.tabs.x`), but nothing proves it. Fix: one test with `explorer.visible = true` that clicks a known editor cell and a tab.

[minor] crates/vr-ide/src/commands/explorer.rs:906-912 — `drop_nested` is O(n²) and called twice per batch. Shift+End over an expanded tree of 20k rows means about 4×10^8 `starts_with` calls, so the UI freezes. Fix: sort, then one linear pass that keeps a path only if it is not under the last kept path.

[minor] BASE-3 — 4 tests were written after their code and shown red only by mutation: `operation_errors_name_their_paths`, `trash_of_unsaved_file_stops_when_recovery_fails`, and both `*_never_loses_a_concurrent_write`. The ledger warning asks for red before the code. Coverage itself is real (the mutation reds are recorded).

[uncertain] crates/vr-ide/src/commands/explorer.rs:548 — `ensure_within` is lexical (D70). Inside an expanded `Symlink(Dir)` that points outside the project, New File, Paste, Move and Delete act on the target outside the root. This matches VS Code. The leader should confirm it is intended, or refuse operations below a symlinked folder whose canonical path is outside the root.

## Focus answers
- a) pass. Typed names go through `validate_name`, then `ensure_within`, and the op uses the returned path (new, rename from+to, paste src+each try, move dest+each src, trash each). The root is refused by row depth and by `below_root`. `drop_nested` runs in `targets` and again in answer_trash/answer_move. Errors are listed per path.
- b) pass. Confirm has `selected = 1` (Cancel) and opens via `open_dialog`, so ENTER_SETTLE applies. Only `Chosen(0)` trashes. `keep_discarded` runs before the trash and returns early on failure (test).
- c) pass. No replace is possible: each try is `File::create_new` / `create_dir` / `symlink`, and EEXIST → `AlreadyExists(to)` → next name. A concurrent creator only makes the next number be used. This depends on vr-fs's atomic no-replace (ops.rs:631-676), not on a check.
- d) pass. `retarget` uses a component prefix, so folder renames and modified buffers are re-targeted, plus `rewatch`. The watcher's later `Renamed` → `recheck(to)` does not prompt falsely (stamp mismatch falls back to content hash, D40). Racing tests are green.
- e) see major 1.
- f) The index rebuilds only when the palette opens while stale, in the background, keeping the old index meanwhile: OK. Leftover notice and go-to-file opening: OK, apart from the selection minor.
- g, h) see the minors.
- Hand repairs: palette.rs (read whole) and main.rs (read whole) are intact.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/commands/explorer.rs (new) | read |
| crates/vr-ide/src/ui/explorer.rs (new) | read |
| crates/vr-ide/src/explorer_tests.rs (new) | read |
| crates/vr-ide/src/commands/palette.rs | read (whole) |
| crates/vr-ide/src/main.rs | read (whole) |
| crates/vr-ide/src/app.rs | read (diff + handle_key/mouse/fs-event/keep_discarded/close_doc/rewatch) |
| crates/vr-ide/src/app_tests.rs, dialog_tests.rs, event_loop.rs, lib.rs | read (diff) |
| crates/vr-ide/src/commands/{dialog,mod,registry}.rs | read (diff) |
| crates/vr-ide/src/ui/{layout,mod,palette}.rs | read (diff) |
| ui/snapshots/render_explorer_{80x24,narrow}.snap (new) | read |
| ui/snapshots/render_palette_{80x24,40x12}.snap (re-accepted) | read — one added row "Focus Next Part … f6", expected |
| Followed: vr-fs ops.rs (ensure_within, create/rename/copy/move_to, copy_entry, io_err), tree.rs (is_expandable), vr-core watch.rs, document.rs (check_disk), notify-debouncer-full cache.rs, notify inotify.rs, vr-config keymap.toml | read (callees only) |

## Rule check T8
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-ide/src/** + team files changed |
| BASE-2 | pass | additive only (PaletteItem fields, layout::compute arg, App fields/methods), listed in notes |
| BASE-3 | fail (minor) | 4 tests red only by mutation after the code (see finding) |
| BASE-4 | pass | notes carry test/clippy/fmt output; my rerun: 98 passed, clippy clean |
| BASE-6 | pass | English, why-comments, no bug history |
| BASE-16 | pass | reuses vr-fs ops/Tree/FileIndex, T6 dialogs, keep_discarded, rewatch; capabilities.md updated |
| BASE-25 | pass | fmt --check rc=0; clippy -p vr-ide -D warnings clean |
| BASE-32 D9 | pass | confirm + system trash, default Cancel |
| BASE-32 D15–D18 | pass / note | tree and index policies used as is; the watcher follows symlinks, unlike D17 (major 1) |
| BASE-32 D24, D25, D32 | pass | CrossDevice message; only no-overwrite vr-fs ops |
| BASE-32 D76.4 | pass | leftovers named once, never deleted (test) |
| BASE-32 D79–D99 | pass | open_dialog + Enter settle; recovery before discard |
| BASE-32 D100–D107 | fail | D105 breaks spec NFR "file watch off the UI thread" (major 1); D103's "no cut key" claim is doubtful (minor) |
| BASE-32 USER-1 | fail (minor) | explorer ctrl+x missing; unmodified keys shadow [explorer] bindings |
| AC-1 | pass | explorer_shows_root_tree, render_explorer_* |
| AC-8 | pass | create/rename/copy/move/delete tests, tabs follow, racing tests green |
| AC-9 | pass | explorer_toggle_ignored; goto_file_opens_selected excludes the ignored file |

## Round 1 re-check

Verdict: **APPROVE T8 R1** (no blocker or major left; 1 new minor, optional)

Scope: `git diff HEAD -- crates/vr-ide crates/vr-config` (HEAD 467e2c5 with FR-4) plus the untracked explorer files, and the notes under `## Fix round 1` / D108–D115. I ran `cargo test -p vr-ide -p vr-config` once: vr-config 3 + 16 passed, vr-ide 110 passed, 0 failed, 2 ignored.

### Earlier findings
| Finding | Status | Evidence |
|---|---|---|
| M1 recursive root watch on the UI thread | fixed | app.rs `watch_root` → `sync_explorer_watches`: `watch_dir` for the root and each expanded folder, `unwatch_dir` on collapse or vanish. It runs after keys, mouse, commands and fs-event flushes and costs O(expanded) when nothing changed. No `w.watch(root)` is left (the remaining `watch` calls are per open file). `watch_dir` (FR-4) is one canonicalize plus one notify request, with no walk and no cache on Linux. `notify_roots` merges a file's parent watch with a flat dir watch under the same key and mode, so an unwatch of the folder keeps the open file's parent watch. Test `explorer_watches_only_shown_folders`; pty Big: 1 watch (563 before) |
| M2 wheel pinned / click after scroll | fixed | `follow_cursor` (the wheel clears it; keys, clicks and `select_path` set it) is honoured by render. Clicks use `drawn_top` from the last frame. The wheel clamps to rows−height. Test `explorer_wheel_scrolls_freely_and_clicks_hit_drawn_rows` |
| Plain keys shadow [explorer] bindings | fixed | route_key runs the built-ins only when `command_for_in(Explorer, chord)` is None. No default global or explorer binding uses a built-in key (checked keymap.toml) |
| explorer.cut / ctrl+x (D108) | fixed | `explorer.cut`; a cut is pasted once through `move_paths` (shared with explorer.move: `below_root`, no-replace `move_to`, retarget, per-path errors, same-folder no-op). keymap.toml `[explorer] "ctrl+x" = "explorer.cut"`; config.rs asserts the explorer binding and that the editor keeps `edit.cut`; KNOWN list updated. Racing test `explorer_cut_paste_never_loses_a_concurrent_write` |
| drain_index resets the selection | fixed | `refresh_keeping_selection` matches by (name, path); typing still resets. Test `goto_file_keeps_selection_when_index_arrives` |
| Dead statements | fixed | removed |
| Default-layout coverage | fixed | `default_layout_with_explorer_clicks_editor_and_tabs` (red by mutation of `area.x`) |
| drop_nested O(n²) | fixed | sort by path (component order, so a folder's descendants follow it directly) plus one pass against the last kept path, then an order-keeping HashSet filter: O(n log n). Duplicates are kept once. It is called once, in `targets`; trash and move paths all come from `targets` |
| BASE-3 | noted | this round's tests each have a red line; two stated exceptions (layout test red by mutation, cut racing test written after) |
| D109 symlinked folder outside the root | fixed | `below_root` refuses rename, move, cut-paste and trash when the canonical parent is outside the canonical root. Trashing the link row itself and new/paste into it stay allowed. Test `linked_folder_outside_the_project_refuses_destructive_ops` |
| QC: folder paste into its own parent | fixed | `copy_into` skips the candidate equal to the source; every try is still O_EXCL / mkdir, so nothing is replaced. Test `paste_into_own_parent_makes_a_copy_name` |
| QC: ignored rows shown by colour only | fixed | trailing " ·" kept when the name is cut, toggle notice says "(marked ·)". Test `ignored_rows_carry_a_text_marker` |

### D109 remaining window (judged)
`below_root` canonicalizes the parent, then `vr_fs::ops` acts on the path. For the op to land outside the project, a process writing in the project would have to replace an in-project directory component with a symlink to an outside folder within those microseconds. Even then, rename and move stay no-replace and the trash is recoverable, so nothing is overwritten or permanently deleted. The anchor's racing-writer clause does not apply. It is the same lexical class as D70 / `ensure_within`. Closing it fully needs dirfd-relative `renameat` with `O_NOFOLLOW` per component inside vr-fs. Accepted as **low**; no change needed for M2.

### New
[minor] crates/vr-ide/src/app.rs (drain_index) — since D115 the index rebuilds on every go-to-file open, so in a project above the 200,000-file cap every ctrl+p adds the info notice "go to file lists the first N files…". Fix: show it once per session (a flag on App).

### Files read (round 1)
app.rs, main.rs, commands/explorer.rs (changed regions: keys, mouse, fs events, below_root, cut/paste/copy_into, move_paths, drop_nested, targets), commands/palette.rs (relist / open_files), ui/explorer.rs (render), explorer_tests.rs (new tests: watches, D109, wheel, list), vr-config keymap.toml + tests/config.rs diff, vr-core watch.rs (watch_dir / unwatch_dir / notify_roots, as a callee). Snapshots: unchanged this round.

### Rule check T8 R1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-ide/src/** plus the leader-approved vr-config line and test (D108) |
| BASE-2 | pass | additive only (`explorer.cut`, Explorer fields, `sync_explorer_watches`, `refresh_keeping_selection`); `index_stale` removed (crate-private) |
| BASE-3 | pass | red line per new test; exceptions stated |
| BASE-4 | pass | notes plus my run: 110 + 16 + 3 passed |
| BASE-6 | pass | why-comments (wheel vs cursor, D109 lexical check, D110 cost) |
| BASE-16 | pass | move_paths shared by move and cut; FR-4 `watch_dir` reused |
| BASE-25 | pass | notes: fmt rc=0, workspace clippy clean |
| BASE-32 D9, D15–D18, D24, D25, D32, D76.4, D79–D107 | pass | unchanged behaviour plus D107 amended by D115 |
| BASE-32 D108–D115 | pass | each applied as recorded |
| BASE-32 USER-1 | pass | ctrl+x cut in the explorer; keymap bindings win over built-ins |
| AC-1 / AC-8 / AC-9 | pass | tests above; pty flows PASS in notes |
