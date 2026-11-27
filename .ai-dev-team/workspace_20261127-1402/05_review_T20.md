# Review T20 — slash commands in the F1 window, phase 1

Verdict: **CHANGES_REQUESTED** (4 major, 1 minor). Security: no issue beyond finding 2 (no extra shell layer, control characters cannot reach `/run`/`/ai`, completion never fills a Text argument).

Evidence: release binary (cargo: up to date with the tree), reviewer pty walk with legacy bytes
`qa-artifacts/T20/review/rwalk.py` → `rwalk.txt`, `rwalk_01..10_*.txt`; `rwalk2.py` → `rwalk2.txt`, `rwalk2_01_run_after_ai.txt`.

## Findings

[major] crates/vr-ide/src/commands/slash.rs:760-765 (with terminal.rs:1122-1126): `/q!` with a running terminal asks the unsaved question again, and its default button saves the edit
Scenario: a.txt edited ("Xalpha"), one bash tab open → `/q!` → "Quit anyway" → `answer_quit` runs `app.quit`, which shows "Save all / Discard / Cancel" with Save all highlighted → Enter → a.txt on disk = `Xalpha\n` (rwalk_07, rwalk_08). The edit `/q!` should throw away is written over the file. This contradicts D268 ("/q! skips only the unsaved question").
Fix: carry the discard intent through the terminals question, e.g. `DialogAction::QuitTerminals { discard: bool }` (or a `quit_discard` flag next to `quit_confirmed`); for `discard`, "Quit anyway" calls `app::answer_quit(app, 1)` instead of `app.quit`. Also call `quit_unblocked` before the terminals question. Test: `/q!` + running terminal + modified doc → one question → quits with the recovery copy, file on disk unchanged.

[major] crates/vr-ide/src/commands/slash.rs:618-621, terminal.rs:810-814: `/run` types into whatever program has the active terminal, not into a shell
Scenario: `/ai cat` (stands in for `/ai claude`), ctrl+`, `/run echo REVIEW_RUN` → the line goes to `cat`; no shell runs it (rwalk2_01: `echo REVIEW_RUN` twice, no `REVIEW_RUN` output). With `/ai claude` the line is sent to the agent as a prompt; with vim or less it becomes editor keystrokes; at a `sudo` prompt it is used as a password attempt. `running()` only means the tab's shell is still alive. `/ai` followed by `/run` is the normal phase-1 flow.
Fix: reuse the active tab only when its shell is the foreground process group (`tcgetpgrp(master) == shell pid`, exposed by vr-term), otherwise open a new tab (or refuse with "the active terminal is running <program>"). Test with a fake foreground program. If the leader reads the spec's "active terminal" literally, this goes to the user as a question.

[major] crates/vr-ide/src/commands/slash.rs:676-682: `/ws save` on the workspace's own file replaces any version on disk, against D244
Scenario: two vr-ide windows on team.vr-workspace. B adds a root and saves. A runs `/ws save` → `seen` = `DiskVersion::of_path` taken just now → B's root is removed from the file without a word. A hand edit of the file is lost the same way. D244: `Some(seen)` replaces only the version the user was shown.
Fix: record the `DiskVersion` when the workspace file is opened or saved (`Workspace::set_file`) and pass it. On a mismatch, refuse with "changed on disk", or ask a Replace confirm that takes the version when it asks (as D244 describes).

[major] crates/vr-ide/src/ui/palette.rs:126-151: slash row titles are cut without "…" at 40 columns
Scenario: 40x12, `/` or `/theme pink` → the row reads `/theme synthwave|dracula|dark|li` (snapshots render_slash_list_40x12 and render_slash_error_40x12, rwalk_09). The text is cut mid-word, and the description that `ellipsize` was added to keep disappears entirely. Text clipped at a size the app supports.
Fix options for the leader or user: (a) `ellipsize` the title too, so it ends in "…" (one line, cheapest); (b) give the hint a shorter form such as `<theme>` and put the names in the description (registry and docs change, the docs test follows).

[minor] crates/vr-ide/src/commands/slash.rs:456-490: path completion does `read_dir` plus one `stat` per matching entry on the UI thread for every keystroke
Measured in a folder of 60,050 entries (`/save-as <big>/` then `f`, Backspace, `d`): 0.138 s, 0.137 s and 0.104 s from key to settled screen (rwalk_05). The time grows with the folder size. A stalled network folder freezes the UI. The listing also runs again on every file-index batch (`refresh_keeping_selection`, app.rs:1401) and twice per Tab or Enter.
Fix: use `DirEntry::file_type()` and `stat` only symlinks; cache the listing per `dir_part` while the window is open.

Out of scope, for the leader (not T20 code): F1 does not reach vr-ide while the terminal has focus. `/run` leaves focus in the terminal, so the next F1 typed `OP/ai cat` into bash (rwalk_04). D258 says "F1 everywhere". This is pre-existing T21 or terminal behaviour.

Checked and fine: the text reaches the terminal once and only on Enter (dev walk_07; rwalk_02: no Enter + Esc → nothing). A pasted newline becomes a space in `LineInput::insert_str`, and the handler refuses control characters. `spawn` uses `args: []`, so no extra shell layer. Completion: Text arguments get no candidates, so nothing can be injected into `/run` or `/ai`. Control characters in file names are filtered by ratatui. `/ws add` reuses `App::add_root` (W-1 checks). `/q` = `app.quit` (unsaved guard kept). The palette without `/` is unchanged (rwalk_01). Esc returns to READ (rwalk_10). The error at 40x12 sits under the input with the input kept. The registry and docs test compares the whole table between the markers. Spec D264/D265/D267 are open and were not judged.

## Files read
- crates/vr-ide/src/commands/slash.rs — read
- crates/vr-ide/src/commands/palette.rs (diff) — read
- crates/vr-ide/src/commands/terminal.rs (diff + quit_allowed/answer_quit/add_tab/spawn/send) — read
- crates/vr-ide/src/ui/palette.rs (diff) — read
- crates/vr-ide/src/commands/mod.rs, lib.rs — read
- crates/vr-ide/src/slash_tests.rs — read in part (docs test, helpers)
- snapshots render_slash_{list,error}_{80x24,120x40,40x12}.snap — 40x12 and 80x24 read, 120x40 skipped (same code path)
- docs/commands.md, README.md, docs/keymap.md — read
- .ai-dev-team/capabilities.md, decisions.md (D258–D268) — read

## Rule check T20
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | scope = palette/slash/terminal additive/docs |
| BASE-2 | pass | additive only (`Palette.error`, `type_line`, `is_slash`) |
| BASE-3 | pass | red lines per group in notes |
| BASE-4 | pass | test/clippy/fmt/release outputs in notes |
| BASE-16 | pass | no existing path completion (grep `read_dir`); add_root/answer_save_as/answer_quit reused |
| BASE-25 | pass | fmt/clippy clean in notes |
| BASE-32 | pass | no key added; keymap.toml untouched |
| AC-20a | pass | dev walk 2–5, rwalk_10 |
| AC-20b | fail | `/q!` writes the discarded edit (finding 1); `/ws save` overwrites (finding 3) |
| AC-20c | fail | Enter-only holds; `/run` reaches a non-shell program (finding 2) |
| AC-20d | pass with finding | docs test ok; 40x12 snapshot shows clipped title (finding 4) |

## Round 1 re-check

Verdict: **APPROVED**. Findings 1-5 are fixed, each with a test that fails without the fix.

1. `/q!` + terminal: `quit_discarding` (app.rs:125) keeps the discard through the stuck-replace and terminals questions (`QuitTerminalsDiscard`, dialog.rs:148). Test `q_bang_with_a_running_terminal_asks_once_and_discards` checks one question, quit, file unchanged, one recovery copy. `q_bang_cancelled_leaves_no_discard_behind` guards a lingering flag. dwalk: file on disk `alpha\n`.
2. `/run`: `Terminal::shell_in_foreground` (vr-term terminal.rs:339) compares `tcgetpgrp(master)` with the shell pid and returns false when exited, closed or without a pid (the safe side: new tab). Additive and correct for shell pid vs foreground group. Test uses a real `cat` under `set -m` and asserts a second tab, nothing in tab 0, the notice. rwalk2: tabs `['bash','bash']`, `REVIEW_RUN` printed by a shell, one raw echo line only (cat's).
3. `/ws save`: `Workspace.file_version` is taken before the read and refreshed after the save; a mismatch is refused with "changed on disk" and the file is kept. Test covers hand edit, a second window, reopen, and a consecutive save. dwalk confirms.
4. Titles: "…" with a description tail, test at 40x12 and 120x40; only the 40x12 error snapshot changed.
5. Completion: `DirListing` cached in `Palette.dir_cache`, `file_type()` with `metadata` only for symlinks. Test counts folder reads across keys and an index refresh, includes a symlinked folder. Measured 0.054 s per key in 60k entries (was 0.138 s).

Spot runs: `cargo test -p vr-ide --lib slash_tests` 44 passed, 1 ignored; `cargo test -p vr-term --test pty` 20 passed; dwalk.py 5/5; rwalk2.py as above. No vr-ide process left running.

No new findings. Open and untouched (user questions): D264, D265, D267, D271. Leader note: D268's text on `/ws save` is outdated (D268 note r1 covers it).

## Rule check T20
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | round diff limited to findings 1-5 |
| BASE-2 | pass | additive API (`shell_in_foreground`, new variants, fields) |
| BASE-3 | pass | tests per finding, red-then-green in notes |
| BASE-4 | pass | notes carry outputs; spot runs reproduced |
| BASE-16 | pass | portable-pty `process_group_leader`, `answer_quit` reused |
| BASE-25 | pass | fmt/clippy clean per notes |
| BASE-32 | pass | no key change |
| AC-20a | pass | unchanged |
| AC-20b | pass | `/q!` discard kept; `/ws save` refuses unseen version (tests + dwalk) |
| AC-20c | pass | `/run` never types into a non-shell program (test + rwalk2) |
| AC-20d | pass | ellipsis title; docs test ok |

## Files read (round 1)
- crates/vr-term/src/terminal.rs (diff), crates/vr-ide/src/commands/app.rs, dialog.rs (cited lines), slash_tests.rs (round 1 block) — read
- slash.rs, workspace.rs, ui/palette.rs, snapshots, docs — skipped except cited behaviour; verified through tests and walks
