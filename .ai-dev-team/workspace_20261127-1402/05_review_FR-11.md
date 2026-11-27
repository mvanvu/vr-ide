# Review FR-11 — Phase 5 minors (reviewer, opus)

Verdict: **CHANGES_REQUESTED**

Tools used: Read, Bash (git diff, grep, headless google-chrome, one test filter through run-heavy.sh), Skill.

## Status of the 5 Phase 5 findings
1. Read-only stays READ: closed. Every edit-mode entry (`commands/edit.rs:47` for ctrl+i / Tab / palette, `commands/explorer.rs:765` for a new file) goes through `App::enter_edit_mode`, which now refuses (`app.rs:651-655`). `sync_mode` (`app.rs:634`) only sets edit for path-less docs, and those are never read-only.
2. "(Esc)" hint: closed. `esc_dismisses` (`status_bar.rs:20-25`) matches `route_key` (`app.rs:887-922`) for dialog, palette, find bar, search panel, explorer, and the editor in read and edit mode. The two edge cases where they differ (focus `Search` while the panel is hidden, and `editor.read_mode` rebound away from Esc) only leave the hint out when Esc would still work, so the hint never promises something false.
3. Wording: closed. README.md:132/321 and docs/keymap.md:37 match `app.rs:679`, and the mode_tests HINT matches too. See m4 for one remaining wording gap.
4. Worker panics: closed for the runtime behaviour. The main-thread path still restores the terminal, writes recovery and prints the replace message in the same order as before; the deferred print is added after them. There is no deadlock: the hook's main path still reads entries with `try_lock` (`snapshot`), and the `deferred` lock is only held for one `push` or one `mem::take`, neither of which can panic. Issues remain in the test (M1) and in m1–m3.
5. Preview: closed. The 40x12 size is added, the Combined scene is added, and the labels and the Confirm dialog are fixed (see Design QA).

## Findings

[major] crates/vr-ide/src/recovery.rs:393-422 — the new hook test races the process-global panic hook and breaks `panic_hook_acts_only_for_the_main_thread`
Scenario: `worker_panic_while_the_tui_is_active_is_kept_not_printed` calls `set_hook` / `install_hook_for` / `take_hook` on the process-wide hook while `panic_hook_acts_only_for_the_main_thread` (which shares the `RESTORED` counter through `count_restore`) does the same on another test thread. Each test's worker and main panics then go through the other test's hook. I ran `cargo test -p vr-ide recovery::` 40 times and all 40 runs failed (`.ai-dev-team/workspace/qa-artifacts/FR-11/race.log`). The failures were `worker panic acted (1,0)`, `RESTORED 2 != 1`, and the new test's own `kept.len()` / `PREVIOUS_CALLS` asserts, whose kept list contained the other test's "worker thread panic". The developer's full-suite pass was scheduling luck, so CI and sign-off runs will go red at random.
Fix: add one `static HOOK_TEST: Mutex<()>` in `recovery::tests` and hold it (`lock().unwrap_or_else(|p| p.into_inner())`) for the whole body of every test that replaces the panic hook. Give the new test its own restore counter instead of `count_restore`/`RESTORED`. Also check whether `event_loop::tests::edits_are_published_before_a_frame_is_drawn` (its `catch_unwind` panic reaches whichever hook is installed) needs the same lock.

[major] BASE-25 — unsupported guard claim
Scenario: the notes say only "Guard: fmt + clippy clean" and "273 passed". They do not include the trimmed output of `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` (rule-packs §7: a pass without evidence is a fail).
Fix: run the guard on the final code (after M1) and paste its trimmed output into `04_dev_FR-11.md`.

[minor] crates/vr-ide/src/event_loop.rs:145-148 — deferred panic messages are printed before `finish()` writes recovery
Scenario: the comment just above says nothing may wait before the terminal restore and the recovery write. `eprintln!` can block (stderr is a full pipe or under XOFF flow control) or panic (stderr is gone with EIO after a terminal hang-up) before `finish` writes the buffers. If it panics, the main hook still writes from the last published snapshot, so the data is probably not lost, but the order of the earlier guarantee is broken.
Fix: keep `set_tui_active(false)` right after `restore_terminal()`, and move the `take_deferred` print loop after `let result = finish(...)`.

[minor] crates/vr-ide/src/recovery.rs:235-248 — `tui_active` is not cleared on the main-thread panic path
Scenario: main panics → the hook restores the terminal → while main unwinds (dropping the App joins or waits on workers), a worker panics. Its message is deferred and never printed, because nothing takes `deferred` again.
Fix: call `state.set_tui_active(false)` as the first line of the main branch, before `restore_terminal()`.

[minor] crates/vr-ide/src/recovery.rs:258-262 — the `deferred` list has no bound
Scenario: a worker that panics on every run (for example a search worker on one bad file, repeated each search) adds one formatted message per run for the whole session, and all of them are printed at exit.
Fix: keep at most about 20 messages, plus a count of the ones dropped ("… and N more worker panics").

[minor] crates/vr-ide/src/app.rs:647 — the read-only edit warning always says "(not valid UTF-8)"
Scenario: for a file that was recognised as another encoding, the open warning says "x.txt is UTF-16LE, not UTF-8; opened read-only" (`app.rs:425-432`), but ctrl+i then says "x.txt: read-only (not valid UTF-8)".
Fix: build the reason from `encoding_name(doc)` the same way the open warning does ("x.txt: read-only (UTF-16LE, not UTF-8)").

[minor] new-states rule — the notes have no `## New states` section
Scenario: FR-11 adds a process-wide flag (`tui_active`) and a kept list (`deferred`). The bound (m3) and the exit paths (m2) are exactly what this section must list.
Fix: add the section, listing for each state where it is set and cleared and what bounds it.

## Design QA
I rendered `mockups/FR-11/synthwave.html` (40x12 scenes for Edit mode, Combined and Confirm) with headless Chrome → `.ai-dev-team/workspace/qa-artifacts/FR-11/combined40.png`.
- Combined 40x12: pass. The explorer and editor split is clean, and the find bar sits on its own row with no overlap. The editor is scrolled sideways by 4 columns to keep Col 24 visible, which is expected. The notice "settings: example warning notice  (Esc)" fits in 39 columns, and the hint is right for this state (editor focus, read mode). The badge shrinks to "R" so the full status line fits. There are no clipped glyphs and no colour clashes.
- Edit mode 40x12 and Confirm 40x12: pass. The modified dot on a.txt and the READ badge on second.rs are correct.
- Label nit, no finding: "the syntax highlighting scenes show EDIT too" does not hold for the Combined scene, but that scene says "(READ)" in its own title.

## Files read
- README.md — read
- docs/keymap.md — read
- crates/vr-ide/src/app.rs — read (diff, plus route_key, notify, the open warning)
- crates/vr-ide/src/event_loop.rs — read
- crates/vr-ide/src/main.rs — read
- crates/vr-ide/src/mode_tests.rs — read
- crates/vr-ide/src/recovery.rs — read (hook and tests)
- crates/vr-ide/src/ui/status_bar.rs — read
- crates/vr-ide/src/ui/tests/preview.rs — read
- callers checked: commands/edit.rs:40-50, commands/explorer.rs:471 and :765

## Rule check FR-11
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to the 5 findings; main.rs and event_loop.rs are outside the board's file list, but finding 4 needs them |
| BASE-3 | pass | red lines in the notes for 3 new tests (self-attested); the tests exist and assert the new behaviour |
| BASE-6 | pass | English; comments explain why (D151 reference in status_bar.rs:15-18) |
| BASE-25 | fail | no guard output in the notes — finding above |
| BASE-32 | pass | D145 and D151 (Esc order in route_key unchanged), D153 (editable_doc checks read_only before the mode), D181/D182/D183 implemented as written |
| new-states | fail | no `## New states` section for `tui_active` / `deferred` |

## Round 1 re-check
Verdict: **APPROVED**

Scope: the round-1 changes in recovery.rs, event_loop.rs, app.rs and mode_tests.rs (all read), plus the `## New states` and `## Round 1` sections of the notes.

- M1 hook-test race: closed. `recovery::tests::HOOK_TEST` is held for the whole body of every test that replaces the panic hook (`panic_hook_acts_only_for_the_main_thread`, `worker_panic_while_the_tui_is_active_is_kept_not_printed`, `deferred_panics_are_bounded_and_the_rest_counted`, `worker_panic_while_main_unwinds_is_printed_not_kept`) and by the `catch_unwind` test in event_loop. A grep finds no other `set_hook`, `take_hook` or `catch_unwind` in the crate. Each test has its own counters, and a poisoned lock is recovered. `race.sh` (`cargo test -p vr-ide recovery::` 40 times) gave **0 / 40 failures** (`qa-artifacts/FR-11/race_r1.log`); round 0 gave 40 / 40 (`race.log`).
- BASE-25: closed. The notes now hold the guard command and its trimmed output with exit 0.
- m1: closed. `event_loop::exit` runs `restore_terminal` → `set_tui_active(false)` → `finish` (recovery) → deferred prints.
- m2: closed. `set_tui_active(false)` is the first line of the main branch, and `worker_panic_while_main_unwinds_is_printed_not_kept` covers it.
- m3: closed. At most `MAX_DEFERRED` = 20 messages are kept, and the rest are counted in `dropped`; `take_deferred` resets the counter and adds "and N more worker panics".
- m4: closed. The shared `not_utf8_reason` is used by both warnings, the open-warning text is unchanged ("is not valid UTF-8" / "is UTF-16LE, not UTF-8"), and `mode_tests::read_only_warning_names_the_real_encoding` covers it.
- new-states: closed. The table lists where each state is set and cleared and its bound, and matches the code.
- No regression: the main-thread hook still restores the terminal, writes recovery and prints the replace message in the original order. No new lock is taken on the hook path: `defer` and `take_deferred` only hold `deferred` around a push or a take.

| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | round 1 only touches the findings' files |
| BASE-3 | pass | red lines in the notes for M1, m2, m3, m4 |
| BASE-6 | pass | English, why-comments |
| BASE-25 | pass | guard output in `## Round 1` |
| BASE-32 | pass | D151/D153/D181–D183 unchanged |
| new-states | pass | `## New states` table |
