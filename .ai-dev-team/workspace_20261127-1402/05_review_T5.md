# Review T5 (opus) — CHANGES T5

(Written by the leader from the reviewer's final message; the reviewer did not write the file.)

`cargo test -p vr-ide` 38 passed, 1 ignored; clippy clean (manifest unused-dep warnings only).

## Findings
- [major] app.rs:401-410, 435-438, commands/app.rs:347, commands/file.rs:256 — D57 "press twice" confirm can discard unsaved changes: (1) a held ctrl+w auto-repeat arrives as a second Press (no REPORT_EVENT_TYPES; legacy terminals too) → tab closes, changes lost, no recovery; (2) Esc dismisses the warning notice but does not clear `confirm`, so a later ctrl+q (minutes later) quits without warning. Fix: Esc clears confirm; ignore repeats (REPORT_EVENT_TYPES + drop Repeat; legacy: time gap/limit) or write recovery for discarded buffers; tests for both.
- [major] commands/edit.rs:114-123 (app.rs:457) — pasted text not normalised: VTE bracketed paste sends `\r`, Windows clipboard `\r\n`; with cr_lines, an LF file gets mixed CR/CRLF lines on save. Fix: normalise to the document line ending in insert_pasted; tests "a\rb", "a\r\nb" into an LF doc.
- [major] ui/editor_view.rs:379-383, 428; status_bar.rs:68; app.rs:541 — per-frame O(line length): a 100 MB single-line file with the cursor at the end freezes the UI for seconds per key (col_of walks graphemes from line start, walk_line per row, char_to_pos allocates a Cow). Fix: approximate columns past N chars or cache per buffer.version(); release test with a 10 MB line.
- [major] recovery.rs:265-268, 306 — recovery file name truncated by chars, not bytes: a long Vietnamese path gives ENAMETOOLONG and `Err(_) => break` skips that buffer at panic. Fix: cut ~200 bytes at a char boundary; on non-AlreadyExists errors retry `<stamp>-<n>-untitled`; test with a 200-char accented path.
- [major] event_loop.rs:108-112 — `publish` runs after `draw`: a panic while rendering loses the last batch's edits from recovery. Fix: publish before terminal.draw.
- [major] status_bar.rs:89-92, 106-124 — file name disappears from the status bar at 40 columns (snapshots `render_tabs_modified_40x12`, `render_notice_and_read_only_status_40x12`): shorten_left keeps the flags, not the name. Fix: shorten only the path, then append (short) flags; update snapshots.
- [major] app.rs:550-568 — FsEvent::Rescan, Renamed, Error branches untested. Tests: Rescan(root) reloads unmodified + marks modified docs; Renamed(tmp, file) reloads; Error shows a notice.
- [minor] commands/file.rs:211-213 — ChangedOnDisk notice points to the palette, which says "not available yet" in T5: use key_hint("file.save_overwrite").
- [minor] BASE-3 — ~9 tests never seen red (event_loop ×4, registry ×2, read_only, pending/palette, title).
- [minor] USER-1 — Esc to dismiss notices and the "(Esc)" text are hard-coded (app.rs:435, status_bar.rs:21,25), not in the keymap nor the plan's built-in list.
- [minor] BASE-16 — editor_view.rs:320-346 col_of/char_at_col duplicate vr_core::display_col/char_at_display_col with different control-char rules → insert_tab/motions may disagree with rendering.
- [minor] app.rs:223-231 — paths not normalised: `sub/../a.txt` and `a.txt` open as two tabs.
- [uncertain] recovery.rs:341-347 — hook runs for panics on any thread (notify's too) and restores the terminal while the main loop continues; main.rs:70-72 — `run` returning Err (EIO) writes no recovery.

## Passed
User files written only via Document::save*; save_overwrite only by explicit named command, no default binding, no automatic path. Recovery 0600 create_new, dir 0700, under state_dir, stamp prefix. Hook restores terminal first, uses try_lock. Chord conversion and legacy branch correct; grapheme/width cursor; own-save not echoed.

## Files read
main.rs (diff), lib.rs, app.rs, app_tests.rs, event_loop.rs, clipboard.rs, recovery.rs, theme.rs, commands/{mod,registry,app,file,edit,tab}.rs, ui/{mod,layout,editor_view,tabs,status_bar}.rs; 7 of 12 snapshots.

## Rule check T5
BASE-1 pass · BASE-2 pass · BASE-3 fail (minor) · BASE-4 pass · BASE-6 pass · BASE-16 fail (minor) · BASE-25 pass · BASE-32 pass except D57 fail (major) · D8 n-o · USER-1 fail (minor) · AC-1 pass (title) · AC-3 pass · AC-4 pass · AC-5 pass · AC-7 pass (T5 part) · AC-18 fail (major, long line) · RF-1 pass · RF-4 fail (major)

## Round 1 re-check — APPROVE T5 R1

`cargo test -p vr-ide`: 54 passed, 2 ignored (release-only). Scope: every file under crates/vr-ide/src (untracked), with the regions round 1 changed read line by line, plus notes `## Fix round 1` and D65–D72.

### Earlier findings
| Finding | Result | Evidence |
|---|---|---|
| M1 held key / stale confirm (D57) | fixed | app.rs `confirm_twice`: a Repeat, or a press < 700 ms after the previous press, only refreshes `last`; 30 s TTL; Esc (`DISMISS_KEY`) and any other input call `cancel_confirm`, which also removes the warning; main.rs pushes REPORT_EVENT_TYPES; Release ignored. Tests `held_quit_or_close_key_never_confirms`, `escape_or_time_cancels_a_pending_confirmation` |
| M2 paste line endings | fixed | edit.rs `with_line_ending` (CR, LF, CRLF → doc line ending) in `insert_pasted` for bracketed paste and edit.paste; `paste_uses_the_document_line_ending` |
| M3 long lines (AC-18) | fixed | editor_view `LONG_LINE` mode: `follow_long` walks at most one screen, rows start at `grapheme_start(line, left)`, status Col = char offset (D67); notes: 10 MB line 0.1 ms per frame (release) |
| M4 recovery name length / fallback | fixed | recovery.rs `sanitize` keeps 200 bytes at a char boundary; any other error → `<stamp>-untitled`; tests for both |
| M5 publish before draw | fixed | event_loop `draw_frame` publishes before `terminal.draw`; test panics inside render |
| M6 file name at 40 columns | fixed | status bar keeps the name and drops detail first; snapshots show `third.md [changed] · Ln 1, Col 2` and `l1.txt [read-only] · Ln 1, Col 1` |
| M7 Rescan/Renamed/Error untested | fixed | `rescan_renamed_and_error_events`; mutation reds recorded |
| minor: overwrite hint | fixed | notice names `keys_for("file.save_overwrite")` or says how to bind it |
| minor: BASE-3 | fixed | mutation table in the notes |
| minor: USER-1 Esc | fixed (documented) | `DISMISS_KEY`, D66 |
| minor: BASE-16 col duplicates | fixed | `vr_core::display_col` and `char_at_display_col` reused |
| minor: `sub/../a.txt` | fixed | `App::resolve` in open_path and save_as; test |
| uncertain: hook threads / run Err | fixed | hook acts only on the main thread; `event_loop::finish` writes recovery on Err and names the files |
| QC minor: keyboard notice | fixed | `until_next` notice is dropped by the next push, 15 s TTL; test |
| `keep_discarded` | ok | quit/close write modified buffers to `state_dir()/recovery` (0700/0600, create_new) before discarding; test |

### New minors (do not block)
- [minor] commands/app.rs `quit` → app.rs `keep_discarded`: the recovery path is shown as an info notice, but `app.quit` is set in the same call. The user never sees where the buffers went, and never sees the error when the write failed. Fix: keep the written paths in `App` and have main.rs print them to stderr after `restore_terminal`; on failure, do not quit and show the error.
- [minor] commands/file.rs `close`: when `keep_discarded` fails, the tab is still closed. The user did confirm the discard, so this is not a silent loss. Fix: keep the tab open when the recovery write fails.

### Rule check T5 (round 1)
BASE-1 pass · BASE-2 pass · BASE-3 pass · BASE-4 pass · BASE-6 pass · BASE-16 pass · BASE-25 pass (per notes) · BASE-32 pass (D57 as refined by D65, D66–D72) · USER-1 pass (D66) · AC-1 pass (title) · AC-3 pass · AC-4 pass · AC-5 pass · AC-7 pass (T5 part) · AC-18 pass · RF-1 pass · RF-4 pass
