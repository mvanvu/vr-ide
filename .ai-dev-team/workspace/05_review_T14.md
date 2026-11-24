# Review T14 — read mode / edit mode (D145, D149-D155)

Verdict: APPROVED (no blocker or major). Three minors for the leader to route. No `uncertain` correctness item, so no opus re-check is needed.
Tools available to me: Read, Bash (non-mutating), Write, Skill, SendMessage, SubagentHandback. I did not run tests (notes carry 219 + vr-config green).

## Findings

[minor] crates/vr-ide/src/ui/status_bar.rs:123-134 — badge costs EOL/encoding at 40 columns
Scenario: 40x12, `README.md`: before T14 the row showed `· Ln 1, Col 1 · LF · UTF-8`; now ` READ README.md · Ln 1, Col 1` (snapshots render_*_40x12). A visible loss the developer lists as a trade-off, so it is filed; the user decides.
Fix options: (a) keep as is (D155 order: badge shrinks only before the file name is cut); (b) shrink to ` R ` / ` E ` before dropping EOL/encoding (cost: the word READ/EDIT is gone at 40 cols, only the colour and letter remain); (c) put the badge after the position. Also `E`/`R` (last tier) sits flush against the path, no space.

[minor] crates/vr-ide/src/app.rs:379-385 + commands/palette.rs:174-177 — Go to File on the already-active tab keeps edit mode
Scenario: edit mode on a.txt, ctrl+p, pick a.txt: palette closes with prev_focus Editor, open_path only calls set_active(same index), `sync_mode` sees no change, the tab stays EDIT. Picking any other file gives READ. D145 (1) says "returning from palette" is read; D150 exempts only a palette-run command. Not data loss.
Fix: in palette::accept for a file item call `app.editor_seen = None` (or a small `App::reset_mode()`), or accept and note it in D150.

[minor] tests — click-outside palette path and the same-tab case have no direct assertion
Scenario: `palette_dismissed` after a mouse Down outside (app.rs ~941) is only exercised through ui/mod.rs snapshot setup (ctrl+i added), never asserted; a regression there would not fail a test. Dark/light badge colours (ANSI Cyan/Black, Blue/White, Indexed 28/White) are not in the contrast table (terminal palette unknown); only dracula is checked at 4.5.
Fix: add a mode_tests case: edit, F1, left click outside the palette area, type -> refused.

## Verified (no finding)
- One gate: every user text change goes through `editable_doc` — grep over `crates/vr-ide/src` (non-test): typing/Enter/Backspace/Delete/Tab (edit.rs 253-271, insert_tab 325), cut (110), paste + bracketed paste (insert_pasted 160), undo/redo (61-69), line ops/indent/outdent/comment (with_buffer 178), find replace_one/replace_all (find.rs 432, 460). `active_doc_mut` users only move selection (select_all, move_with, page, find select, nav.goto_line, mouse). Cut calls the gate first, then `active_doc_mut` (edit.rs 118): safe. Mouse has no paste/drag-move. Project replace (search.rs 1198) and reload (app.rs 1108, file.rs 640) are the intended exceptions. Save does not go through the gate (not a user edit).
- Mode rule: `sync_mode` at end of key/mouse/fs events, `run_command`, `open_path`, and inside `editable_doc`; staleness only errs toward read mode. Checked tab close (`editor_seen` shift/forget, app.rs 516-521), Save As and external-change prompt (dialog -> reset; edits kept), quit guard (dialog), recovery (writes `docs`, not mode dependent), ctrl+n (`answer_new` runs after the dialog is cleared, so `enter_edit_mode` survives the next sync; test green), palette (prev_focus keeps mode while open; accept closes first so picked command runs in the tab's mode), find bar (keeps mode; Esc excluded from the keymap path via `editor.read_mode` check, find.rs 530-534).
- Esc order: edit -> read before notice dismiss (app.rs 846-856), read mode dismisses notice then clears selection via keymap; Esc in find bar, palette, dialogs, search, explorer unchanged (those branches run earlier).
- Tab: read -> edit (edit.rs 258), edit -> indent, shift+tab refused in read.
- Cursor: written only on change after a frame; `restore_terminal` (main.rs 31) is the same fn used by exit, setup failure and the panic hook -> shape restored in all three.
- Badge: tiers `READ/R/R` fall back before the file name is cut; colours via theme.mode_read/mode_edit; dracula pairs in the contrast table at 4.5 for both depths; distinctness test for every theme. keymap.toml, KNOWN list, docs/keymap.md, README agree with the code (ctrl+i, escape, Tab note, find-replace row).
- Existing tests: changes are only added `ctrl+i` (plus one comment reword in explorer_tests, and `app_with_file` pressing ctrl+i); snapshots differ only in the status row. Red table is consistent with the notes (12 red / 5 green guards); the 4 post-code tests are labelled as such and assert behaviour.
- BASE-1: the two one-liners (explorer.rs `answer_new`, main.rs cursor restore) are needed by D145 (1)/(5), disclosed under Deviations: accept.

## Files read
README.md read; crates/vr-config/defaults/keymap.toml read; crates/vr-config/tests/config.rs read; app.rs read; app_tests.rs read; commands/edit.rs read; commands/explorer.rs read; commands/find.rs read; dialog_tests.rs read; event_loop.rs read; explorer_tests.rs read; lib.rs read; main.rs read; mode_tests.rs read; search_tests.rs read; theme.rs read; ui/mod.rs read; ui/status_bar.rs read; docs/keymap.md read; ui/snapshots/*.snap (22) read as a changed-line summary (status row only).

## Rule check T14
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | two out-of-list one-liners accepted: commands/explorer.rs:762-766, main.rs:31 |
| BASE-2 | pass | additive commands/fields only; keymap + KNOWN + docs updated |
| BASE-3 | pass | red table in 04_dev_T14.md matches mode_tests; 4 post-code tests labelled; minor test gap filed |
| BASE-6 | pass | English; why-comments at sync_mode, editable_doc, Tab arm, find Esc |
| BASE-16 | pass | reuses editable_doc, key_hint, notices; no duplicate |
| BASE-25 | pass | notes show fmt/clippy clean (not rerun) |
| BASE-32 | pass | D145 (1)-(7), D149-D155 implemented; Esc elsewhere unchanged |
| new-states | pass | section present; tab close, Save As, prompt, quit, recovery, palette, find checked in code |
| AC-3 | pass | edit/undo/redo/indent in edit mode, tests via ctrl+i |
| AC-5 | pass | keymap.toml + KNOWN list + default_keymap_validates_cleanly |
| AC-10 | pass | find works in read mode, replace refused (find.rs 432/460) |
