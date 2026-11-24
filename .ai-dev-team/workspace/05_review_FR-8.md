# Review FR-8 — CHANGES_REQUESTED? no: APPROVED (2 minors)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings
[minor] crates/vr-ide/src/commands/explorer.rs:612 — `filtered` is not cleared when a .gitignore/.ignore changes
Scenario: ignored `sub/x` opened via palette, then saved (enters `filtered`); the user un-ignores it in .gitignore (a Modified event on a listed file dirties nothing). Next save of `x` used to re-read the folder (row appeared); now it is skipped, so the row stays hidden until some other event re-reads `sub`. The listing was already stale for other files, so this is the old gap narrowed, not a new loss; no data effect.
Fix: in note_written, when the written file name is `.gitignore`/`.ignore`, call `ex.filtered.clear()` (cheap), or clear `filtered` in on_fs_event for those names.

[minor] crates/vr-ide/src/commands/file.rs:531 — `file.open` with `CmdArgs::Path` still reports BeingReplaced as an Error (via run_command, with the "Open File:" prefix)
Scenario: command invoked with a path while Replace writes that file → error-level notice, not the warning the other paths now give. Reach is programmatic only (the dialog path re-prompts inline).
Fix: map the error through `search::report_open_error` in that arm, or note it as accepted.

## Findings closed
- Major (Keep mine recreate -> row): closed. keep_mine Ok branch only adds note_written; the write call (save_overwrite/_seen) is untouched. Test is red->green per notes.
- Minor 1 (filtered re-read): closed. reload() clears entries per folder (watcher flush, toggle via reload_all, refresh_after after rename/delete, create). Listed check comes first, so a file that became visible via expand()/toggle shows. Rename: refresh_after reloads the parent, so the stale entry is dropped. Stale entries for collapsed folders are harmless (note_written requires expanded; listed wins).
- Minor 2 (BeingReplaced): closed for explorer open/new, palette, search panel; wording "try again in a moment".
- RELOADS counter and reload_count are #[cfg(test)]; no-op in release; thread-local, so parallel tests do not interfere.

## Files read
crates/vr-ide/src/commands/explorer.rs read; file.rs read; palette.rs read; search.rs read; dialog_tests.rs read; explorer_tests.rs read. 04_dev_FR-8.md read.

## Rule check FR-8
| Rule | Result |
| BASE-1 | pass |
| BASE-2 | pass (keep_mine write path unchanged) |
| BASE-3 | pass (red->green in notes, tests read) |
| BASE-6 | pass (English) |
| BASE-16 | pass (report_open_error reuses existing logic, not duplicated) |
| BASE-25 | pass (notes carry lib 173/0, fmt, clippy) |
| BASE-32 | pass (no new decision needed) |
| new-states (`filtered`) | pass with the minor above; no I/O, no data written |
| AC-3, AC-7, AC-8, AC-12 | pass |

## Quick pass 1 re-check — APPROVE
Diff read: `git diff 73d4ebf -- crates/vr-ide` (explorer.rs, file.rs, search.rs, explorer_tests.rs); only these four files changed.
- Minor 1 closed: `filtered` is cleared on a watcher event (Created/Removed/Modified/Renamed) for `.gitignore`/`.ignore`, and in `note_written` for our own save of one. The next save of a no-longer-ignored file then finds it neither listed nor filtered and re-reads the folder. The clear sits before the `loaded` early-return only in `note_written` (harmless); in `on_fs_event` it sits after the `loaded` check, which is fine because `filtered` is empty when nothing is loaded.
- Minor 2 closed: `file.open` with a path goes through `report_open_error` (warning for BeingReplaced).
- Side change accepted: other open errors stay errors but lose the "Open File:" prefix and no longer pass through run_command's error path; the arm now returns Ok(()). The only CmdArgs::Path caller is programmatic (main.rs uses `open_path` directly), and the messages already carry the path. No behavior is lost.
- Nothing else changed; no new states beyond `filtered` clearing (memory only, no I/O).
- Tests: the new test fails without the fix by construction (row stays hidden after the second save because the entry is in `filtered`); the extended gate test fails without the fix (Error with "Open File:" prefix, per the notes). Notes carry 174/0/3, fmt, clippy clean.

| Rule | Result |
| BASE-1 | pass |
| BASE-2 | pass |
| BASE-3 | pass |
| BASE-6 | pass |
| BASE-16 | pass |
| BASE-25 | pass |
| BASE-32 | pass |
| new-states (`filtered`) | pass |
| AC-3, AC-7, AC-8, AC-12 | pass |
