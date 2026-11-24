# Review FR-5 — APPROVED (4 minors)

Scope: `rule-checks.sh task-diff FR-5 -- crates/vr-ide` (base 0e5754f), 939 lines.

## Gate (ReplaceGate) verdict
- Deadlock: none. UI (`open`, `open_failed`) and worker (`begin`, `end`) each take the one mutex for a set/option operation only; no nested lock (`current` is released before `begin`); poisoning is tolerated.
- I/O under the lock: none. `canon()` runs before `open()`; `apply_replace` runs between `begin` and `end`, outside the lock.
- Keys match: the UI uses `canonicalize(resolve(path))`, the worker uses `canonicalize(fp.path)`. Root is canonical (cli.rs:74).
- Entry paths: `Document::open` is called only in `App::open_path` (app.rs:381). Explorer, go-to-file/palette, file.open, search result / f4 and the CLI arg all go through it. Reload and retarget keep an existing tab. Save As is the one path that puts a tab on a file without the gate (finding 1).
- Refusal path: every caller shows the message. file.open reopens its prompt; the explorer keeps its focus; palette and search result close or move focus (finding 2). Nothing is lost silently.
- Panic halfway: `writing` stays set until drain drops the job, so at most a transient refusal.
- Old test replaced: `tab_edited_during_a_replace_loses_nothing` → `tab_opened_during_a_replace_is_never_written` is the change D135 approves (amends D120/D126). The "loses nothing" oracle still holds and is stricter. The tab edit (`xfoo`) is kept, the disk is untouched (`foo`), the skip is reported, the 39 others are replaced, and the save writes the user's text. The old save-prompt branch cannot be reached any more for an opened tab. Not a loosened oracle.
- BASE-3: the feature was driven red-first by `tab_opened_during_a_replace_is_never_written` ("written behind the tab"). The gate unit tests came after the code. Their mutation (begin ignoring `opened`) proves the worker side. The refuse-while-writing side is proven deterministically by `opening_the_file_being_written_is_refused_then_allowed` (its first assert). The racing test detects that side only by chance. Accepted.

## Findings

[minor] crates/vr-ide/src/commands/file.rs:431 (save_as_doc) — Save As onto a pending replace file bypasses the gate
Scenario: while a replace runs, the user saves an untitled or unchecked tab (Save As) onto a pending closed file P whose bytes equal P's previewed text. The worker's fingerprint check passes, and the worker writes the replace behind that tab. That contradicts D135 "never written" and the apply doc comment. Nothing is lost: an unmodified tab reloads, and a modified one gets the AC-7 prompt. With different bytes the worker already refuses ("changed on disk").
Fix: in save_as_doc, before the write, call the same claim (`before_tab_opens(app, target)`). Refuse when the target is being written, and call `open_failed` when the write fails.

[minor] crates/vr-ide/src/commands/search.rs:652 (open_selected) + :274 — a refused open still gives the editor the keys
Scenario: during a replace, Enter on a result whose file the worker is writing → error notice "...open it again in a moment". `focus_editor` still runs. The keys go to the previously active tab, and below 40 columns the panel (with the replace progress) is hidden. "Again in a moment" then takes reopening the panel. The refusal also shows as an Error, although it is a transient state.
Fix: call `focus_editor` only when `open_match` succeeded (let it return bool). Show the refusal as `warn`.

[minor] crates/vr-ide/src/commands/explorer.rs:816 (check_tab_free) — the refusal text claims "its file is not on disk" when it is
Scenario: b.txt exists and is open in a tab. The user renames a.txt → b.txt in the explorer and gets "b.txt is open in a tab (its file is not on disk); close that tab first". The text is false. After closing the tab, the retry fails with "already exists".
Fix: run the check only when `to` is absent (`symlink_metadata().is_err()`), so vr_fs reports the existing name. Or drop the parenthesis.

[minor] crates/vr-ide/src/commands/search.rs:1043 (paths_moved) — known gap: a search still running during a rename delivers old paths
Graded minor. Enter shows "No such file" for an existing file. A preview built from such a result marks the file unreadable (not checked), and the fingerprint check guards any write. No data risk, and searching again fixes it.
Fix (cheap): in paths_moved, when a search job is running, restart it (`start(app)`), or set a "files moved; search again" status.

Nit (no change asked): "(FR-5)" task tags in app.rs:379 and search.rs:1889 comments. These are task references, not "why" (BASE-6 spirit).

Six findings of 06_final_review.json: all fixed. Save As/recreate row → note_written (save_doc, save_as_doc; Save (recreate) uses save_doc). Stale paths → paths_moved. Reveal hidden/ignored → root-first walk + not_shown_reason. Replace vs new tab → ReplaceGate. Two tabs on one path → check_tab_free + shared-watch guard in close_doc. Narrow Esc/F6 → focus_editor. Each one has a red-first test.

## Files read
- crates/vr-ide/src/app.rs — read
- crates/vr-ide/src/commands/explorer.rs — read (diff + reload/expand, open callers)
- crates/vr-ide/src/commands/file.rs — read (diff + save/save_as/recreate/keep_mine callers)
- crates/vr-ide/src/commands/search.rs — read (diff + canon, open_match, drop_preview, applying, escape)
- crates/vr-ide/src/explorer_tests.rs — read
- crates/vr-ide/src/search_tests.rs — read (diff + held_apply)
- followed: vr-search/src/replace.rs apply_one (freshness), vr-fs ops rename/move_to, palette.rs/main.rs open callers

## Rule check FR-5
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | 6 files in crates/vr-ide only |
| BASE-2 | pass | additive pub helpers listed; no planned contract changed |
| BASE-3 | pass | red line per test in notes; gate unit tests post-code but feature driven red-first + mutation shown (see above) |
| BASE-6 | pass | English, why-comments; nit on "(FR-5)" tags |
| BASE-16 | pass | reuses reload/rebuild, drop_preview, canon; check_tab_free justified (check_other_tabs is index/equal-only); capabilities.md updated |
| BASE-25 | pass | notes: fmt ok, clippy --workspace -D warnings clean |
| BASE-32 | pass | D82, D95, D118, D120/D126 (amended by D135), D129 unchanged, D132–D137 match code |
| new-states | pass | ReplaceGate listed with quit/cancel/error/input change/second request/concurrent writer/hung call/failed open; racing test + deterministic test |
| AC-3 | pass | tab edits kept during replace (`xfoo`), narrow keys never reach an undrawn buffer |
| AC-8 | pass | save/Save As/recreate rows appear; rename/move onto a tab-held path refused |
| AC-9 | pass | reveal never expands/watches hidden or ignored folders; notice names the toggle |
| AC-12 | pass | only confirmed files written; a file opened during the replace is skipped and reported (Save As residual = finding 1, minor) |
