# Review T6: palette, dialogs, go-to-line, external-change prompt, quit guard (incl. D64 wiring)

Verdict: **CHANGES T6**

Commands run by the reviewer: `cargo test -p vr-ide` gave `67 passed; 0 failed; 2 ignored`. `cargo clippy -p vr-ide --all-targets -- -D warnings` finished clean; its only output is the manifest note that `vr-search` is unused (until T10).

## Findings

[blocker] crates/vr-ide/src/commands/file.rs:202-218 — Save As to the document's own path skips the D64 check and overwrites a newer disk version
Scenario: an agent rewrites `a.txt` while the buffer is modified, so the tab shows `[changed on disk]`. The user runs Save As. The prompt starts from the current name (D86), and the user presses Enter. `exists && path != target` is false, so `save_as_doc` → `Document::save_as` → `write_to(p, None)` runs. That replaces the agent's version with no prompt and leaves no conflict copy. The same happens with a writer racing a plain Save As of an unchanged file. The safety that `save`, `save_overwrite_seen` and D83 add is bypassed through the default path of a standard command.
Fix: when the target equals the document's own path, call `save_doc(app, i)`, which gives `ChangedOnDisk` → prompt → `save_overwrite_seen`. Test: buffer modified, file rewritten on disk, Save As + Enter on the prefilled name. Expect the file unchanged and the external prompt open. Add a RacingWriter variant on the same path.

[blocker] crates/vr-ide/src/commands/file.rs:202-216 and commands/dialog.rs:83-88 — Save As to a different target writes with no freshness expectation (Replace and new-file paths)
Scenario (a): the Replace / Cancel dialog sits open while an agent writes `other.txt`. The user clicks Replace, and the agent's write, which the user never saw, is replaced. Scenario (b): the name does not exist when `symlink_metadata` is checked, a writer creates it before `write_to(p, None)`, and that file is replaced with no confirmation at all. The severity anchor treats any window like this as a blocker. The developer's notes flag it under "Out of scope noticed", so it is known and still open.
Fix: needs a vr-core API, so the leader routes it. Add `Document::save_as_seen(&Path, &DiskVersion)` plus a way to take a `DiskVersion` of an arbitrary path (D74 already treats an absent file as a version). T6 then takes the version when the Replace dialog opens, or at submit for a target that does not exist, and re-prompts on `ChangedOnDisk`. Add a racing-writer test for each case.

[major] crates/vr-ide/src/commands/file.rs:375-382 (reload_doc, reached from both external-change prompts) — Reload discards unsaved work with no recovery copy
Scenario: the user picks Reload (event or save prompt), and the buffer becomes unmodified (`Document::reload`: "unmodified afterwards"). The user's text now survives only in the undo history. The user then presses ctrl+q, or ctrl+w on the tab. `quit` / `close` see no modified document, so they exit with no dialog and the user's edits are lost for good. That breaks the focus rule: never lose unsaved work without a recovery copy. The button text promises "undo brings yours back", which holds only until close or quit.
Fix: in `reload_doc`, when the document is modified, call `app.keep_discarded(&[i])` first. On `Err`, show the error and do not reload, the same as Discard. Say where the copy went. Test: modified buffer → Reload → one recovery file holding the old buffer; a failed recovery write leaves the buffer as it was.

[major] crates/vr-ide/src/ui/dialog.rs:179, 295 and commands/palette.rs:142 — Enter auto-repeat on legacy terminals, or an Enter already typed, answers dialogs
Scenario: without keyboard enhancement (a supported mode: the app shows a notice for it), auto-repeat arrives as `KeyEventKind::Press`. D65 and the removed T5 test `held_quit_or_close_key_never_confirms` covered exactly this case. Only `Repeat` is filtered now. Three cases:
- (1) Palette "close" + Enter held → the Close dialog opens and the next repeated Enter picks Save, writing the file without being asked. The same applies to "quit" → Save all.
- (2) Prompts that open on their own (watcher event: Reload / Keep mine; deleted: Save (recreate) / Close) take the Enter the user was typing into the editor. On the deleted prompt the default is "Save (recreate)", so an Enter typed at that moment recreates a file someone just deleted.
- (3) The user's keystroke is swallowed and the prompt closes before the user has seen it.

No test covers legacy timing any more.
Fix: let a confirm dialog accept Enter (and the palette accept) only after a quiet gap since it opened. Something like the old `CONFIRM_REPEAT_GAP` works: no Enter press in the last ~500-700 ms, on every terminal or at least when `!keyboard_enhanced`. Restore a time-injected `handle_key_at` test: legacy presses every 33 ms after the dialog opens choose nothing, and a deliberate press after the gap chooses.

[minor] crates/vr-ide/src/dialog_tests.rs:576-584 — the racing test does not assert that both outcomes happened
Scenario: if timing shifts so that every run is refused (or every run succeeds), the test still passes and one branch of `save_overwrite_seen` is no longer exercised. The counts are only printed.
Fix: `assert!(ok_count > 0 && refused > 0)`, or require at least one of each per mode.

[minor] crates/vr-ide/src/app.rs:556-560, 585-593 — modal routing is not tested at App level
Scenario: no test types letters, pastes text or presses ctrl-chords while a Confirm dialog is open and then checks that the buffer is unchanged. Paste is tested only on the widget, so a routing regression would go unnoticed.
Fix: one App-level test: quit dialog open → `typ("abc")`, `handle_paste("x")`, ctrl+z → buffer unchanged, dialog still open.

[minor] crates/vr-ide/src/commands/file.rs:203 — Save As / Replace onto a file that is already open in another tab
Scenario: two tabs now share one path. Editing and saving the other tab gets the D64 prompt, so nothing is lost, but the result is confusing.
Fix: after Replace, close the other unmodified tab or warn about it (it can also go to T8).

Note (BASE-16, judged): ranking through `vr_fs::FileIndex::query` is an acceptable reuse while the manifest is frozen. Titles hold no `/`, so `whole_path_only` and the path bonuses stay inactive, and whitespace is stripped ("go to" matches). Hits come from the title only, which is fine. Recommend `nucleo-matcher.workspace = true` in vr-ide at the next manifest window; `rank` is the single place to change.

## Checked and OK
Discard is never the default button. Quit and close write the recovery copy first and block on a failed write (tests cover both). `disk_version()` is taken first in `prompt_external_change`, and no disk text is displayed. Keep mine calls `save_overwrite_seen`, and `ChangedOnDisk` re-prompts with a fresh version ("changed again"). The racing test drives `App::handle_key(Enter)` → `keep_mine` with `assert_no_lost_write_swapping` and `kept` (the strict D77/D78 oracle). `rewatch` unwatches the old path only when no other tab has it open. Dialogs take keys, mouse and paste first, and Release events are ignored. Palette keys come from `Keymap::keys_for`, it opens through the keymap (F1, ctrl+shift+p), and it answers click, wheel and click-outside. Go-to-line clamps line and column, and a bad input reopens the prompt with a hint. Render code cannot underflow at small sizes. The four snapshots read correctly at 80x24 and 40x12. The removed T5 tests are covered by `quit_with_unsaved_asks`, `close_with_unsaved_asks`, `modified_doc_changed_on_disk_prompts` and `palette_*`, except legacy auto-repeat timing (finding 4).

## Files read
- crates/vr-ide/Cargo.toml — read
- crates/vr-ide/src/app.rs — read (diff + touched functions)
- crates/vr-ide/src/app_tests.rs — read (diff)
- crates/vr-ide/src/commands/app.rs — read
- crates/vr-ide/src/commands/file.rs — read
- crates/vr-ide/src/commands/mod.rs — read
- crates/vr-ide/src/commands/registry.rs — read (diff)
- crates/vr-ide/src/commands/dialog.rs — read
- crates/vr-ide/src/commands/nav.rs — read
- crates/vr-ide/src/commands/palette.rs — read
- crates/vr-ide/src/dialog_tests.rs — read
- crates/vr-ide/src/lib.rs, main.rs, theme.rs — read (diff)
- crates/vr-ide/src/ui/mod.rs, ui/editor_view.rs — read (diff)
- crates/vr-ide/src/ui/dialog.rs, ui/palette.rs — read
- crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_{confirm_dialog,palette}_{80x24,40x12}.snap — read
- crates/vr-core/src/testutil.rs — skipped (FR-1, per brief)
- Contracts followed: vr-core document.rs `disk_version`, `save_overwrite_seen`, `save_as`, `reload`, `write_to`; vr-fs file_index.rs `from_paths`, `query`; recovery.rs `write_docs`

## Rule check T6
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-ide/** and the approved dev-dependency line |
| BASE-2 | pass | registry contract unchanged; additive APIs listed in notes; `DialogAction::ExternalChange.seen: vr_core::DiskVersion` |
| BASE-3 | pass | red line per test in Progress; palette_enter/escape and mouse red by mutation; racing test red on the interim adapter 3/3 |
| BASE-4 | pass | trimmed test/clippy output in notes; matches my run (67 passed) |
| BASE-6 | pass | English, comments explain why |
| BASE-16 | pass | FileIndex reuse judged acceptable (note above); keep_discarded extended, not duplicated |
| BASE-25 | pass | clippy -p vr-ide clean; fmt diff only in testutil.rs (FR-1, not T6); rustfmt --check on vr-ide is clean |
| BASE-32 D57/D65 | pass | replaced by D79/D80 dialogs |
| BASE-32 D64/D74 | fail | prompt and Keep mine are correct, but Save As to the own path bypasses it (finding 1) |
| BASE-32 D77 | pass | strict `_swapping` oracle with `kept` |
| BASE-32 D79 | pass | built-in dialog keys as documented |
| BASE-32 D80 | fail | only `Repeat` filtered; legacy auto-repeat chooses (finding 4) |
| BASE-32 D81, D82 | pass | prompts and defaults as decided (Reload copy: finding 3) |
| BASE-32 D83, D84, D85 | pass | code matches |
| BASE-32 D86 | fail | Replace confirm present, watcher re-targets; write has no expectation (finding 2) and own path skips the check (finding 1) |
| BASE-32 USER-1 | pass | palette and keys from keymap; no hard-coded chords besides documented built-ins |
| AC-6 | pass | palette_filters_by_fuzzy_name_and_shows_keys, render_palette |
| AC-7 | pass | prompts work; data-safety gaps listed as findings 1-3 |

## Round 1 re-check

Verdict: **CHANGES T6 R1**. The only open item is the flaky racing test R1-1.

Reviewer run: `cargo test -p vr-ide` → `76 passed; 1 failed; 2 ignored`. The failure was `dialog_tests::replace_never_loses_a_concurrent_write`, which panicked at dialog_tests.rs:602 with "a refused write asks again". The same test run alone 3 times passed each time (ok 18-20 / refused 20-22 per mode). The no-lost-write oracle (line 594) passed in the failing run, so no data was lost.

### Earlier findings
| Finding | Status | Evidence |
|---|---|---|
| B1 own-path Save As | fixed | `is_own_path` (same path or same canonical location) routes the prompt and `CmdArgs::Path` to `save_doc`; `save_as_to_own_path_checks_freshness_like_save` + racing `save_as_to_own_path_never_loses_a_concurrent_write` (red by mutation recorded) |
| B2 Save As to another target | fixed | `DiskVersion::of_path` is taken at submit, before the Replace prompt is built; `save_as_seen` is used for Replace and for a new name; `Changed` → warning + fresh Replace prompt; `CmdArgs::Path` → checked `save_as` (D89); deterministic `replace_never_overwrites_a_version_written_after_the_prompt` red → green |
| M1 Reload without recovery | fixed | `reload_doc` writes recovery first and refuses on `Err`; `file.reload` goes through it; `reload_keeps_unsaved_edits_in_recovery` |
| M2 legacy / in-flight Enter | fixed | `route_key`: Enter ignored while modal if Repeat, < 500 ms since open, or < 700 ms since the previous Enter (`last_enter` refreshes on every Enter, so a held key stays ignored until released); timed test `held_or_in_flight_enter_never_answers_a_dialog` covers palette, Close dialog and watcher prompt |
| minor both outcomes | fixed | `race_enter_on` asserts ok>0 && refused>0 per mode |
| minor App-level modal routing | fixed | `input_while_a_dialog_is_open_never_edits_the_buffer` (red by mutation) |
| minor two tabs per path | fixed | see below |

### New items
- Enter settle (500 ms) / repeat gap (700 ms): correct. Both timers apply only while a dialog or the palette is modal, and editor Enter still updates `last_enter`. Cost: a deliberate Enter within 500 ms of a prompt opening (e.g. accepting the prefilled Save As name right after choosing it in the palette) is swallowed with no feedback, and the user presses again. That is acceptable. Optionally dim the selected button during the settle time.
- Deleted prompt with "Keep" as default: correct. `[Save (recreate), Close, Keep]` with `selected = 2`, `answer_deleted` ignores index 2, and the message says Keep writes nothing. Every prompt that opens by itself now defaults to an answer that writes nothing.
- One tab per path: correct. `check_other_tabs` refuses a target that is open with unsaved changes, both in the prompt and in `save_as_doc` (covers `CmdArgs::Path`). After the write, other unmodified tabs are closed before `rewatch`, so the target stays watched, and `close_doc` keeps `active` on the saved tab.
- `save_as_new_name_never_loses_a_concurrent_create`: never red. A vr-ide mutation to unchecked `save_as` would still pass, because FR-2 checks at commit start. It still guards the vr-ide wiring: a version taken of the wrong path or before submit would make ok=0, and treating `Changed` as `Failed` (no Replace prompt) breaks `asks_again`. It is also an end-to-end regression check of vr-core's absent-target commit. Kept as a useful guard, not as proof of B2. B2 rests on the deterministic test and on the Replace racing test, which was red by mutation.

### Findings
[major] crates/vr-ide/src/dialog_tests.rs:602 with commands/file.rs:201-205, 222 — `replace_never_loses_a_concurrent_write` is flaky under the full suite (R1-1)
Scenario: in a refused run, nothing is written but no Replace prompt is open. Two code paths reach this. (a) After `Changed`, `reopen_replace` calls `DiskVersion::of_path` while the writer is still rewriting; D78's retries run out, so it returns "keeps changing", shows an error and opens no prompt. (b) `save_as_seen` ends with a conflict copy ("kept at" `Io`), giving `Failed`, an error and no prompt. Both are safe, but `asks_again` requires the prompt, so the required `cargo test -p vr-ide` fails intermittently: 1 of my 4 runs, and the failing one was the full parallel suite. `keep_mine` / `save_as_new` share the oracle and the same branches (`keep_mine` `Err(e)` → error only).
Fix: pick one and apply it to all three tests.
- (1) Make the oracle accept "refused → prompt open OR an error notice naming the file", with nothing replaced (the no-loss oracle already proves that).
- (2) Make `reopen_replace` / `keep_mine` always reopen the prompt, with the error shown in it, when a fresh version cannot be read or a conflict copy was kept.

Also put `notices(app)` in the assertion message so a failure names the branch. Then run the racing tests about 10 times in the full suite.

## Rule check T6 R1
| Rule | Result | Evidence |
|---|---|---|
| BASE-3 | pass | red lines for B1, B2 (deterministic), M1, M2, minors; Replace racing and non-regular red by mutation; new-name racing not red (judged above) |
| BASE-4 | fail | the notes report 77 passed with 6 clean racing runs; my full-suite run failed once (R1-1) |
| BASE-32 D64/D74/D87-D89 | pass | own path → D81 prompt; Replace and new name through `save_as_seen` with the version from submit |
| BASE-32 D80/D82 (amended) | pass | Enter guard; Keep default |
| BASE-32 D86 | pass | Replace confirm, one tab per path, rewatch |
| AC-7 | pass | B1/B2/M1 closed |

## Round 2 re-check

Verdict: **APPROVE T6 R2**

Reviewer run: full `cargo test -p vr-ide` ×3 → `79 passed; 0 failed; 2 ignored` each time (0.81-0.82 s). The developer's notes show a further 10 clean full-suite runs.

### R1-1
Fixed, using option (2) from round 1 (D98).
- `answer_replace`, `keep_mine` and `prompt_external_change_with` reopen the same prompt after every refusal (`Changed`, `Failed`/"kept at", other errors, unreadable version). The reason is shown first through `with_errors`.
- A new-name `Failed` returns to the name prompt with the error.
- The racing assertion messages now print the dialog and the notices.
- Two deterministic seam tests cover branch (a) "keeps changing" and branch (b) conflict copy. Their red lines are recorded in the notes.

### No path writes when the version is None
- `answer_replace(.., None)` calls `reopen_replace` and does nothing else.
- `keep_mine(.., None)` calls `prompt_external_change_with` and does nothing else.
- `answer_save_as`: a failed `take_version` sends the user back to the name prompt (`retry`), and only `Some(&seen)` ever reaches `save_as_doc`.
- `save_as_doc(.., None)` is reached only from `file.save_as` with `CmdArgs::Path`, which has no prompt. That path uses the checked `Document::save_as` (D89), so it is not a "no version" case.
- The tests show no write while `seen` is None: `refused_choice_reprompts_when_no_version_can_be_taken` checks `other.txt` stays "agent v2" and the Keep mine file stays "v3 longer" / "v2".
- A Replace or Keep mine that is refused again can never loop on its own: every retry needs a new user choice, and the Enter settle time applies again because each reopen resets `modal_since`.

### The seam cannot be reached in a release build
- `mod seam` and `keeps_changing` are `#[cfg(test)]`.
- The calls in `take_version`, `doc_version` and `injected_commit_error` are statements under `#[cfg(test)]`.
- Outside a test build, `injected_commit_error()` always returns `None`, so `save_as_doc` and `keep_mine` always call vr-core.
- `cfg(test)` is set only for vr-ide's own unit-test build. Release, dev and integration-test builds of the binary compile without it.
- The state is thread-local, and libtest runs each test on its own thread, so one test's injected failure never leaks into another.

### Findings
None.

## Rule check T6 R2
| Rule | Result | Evidence |
|---|---|---|
| BASE-3 | pass | both seam tests red before the wiring (notes) |
| BASE-4 | pass | 10-run loop in notes; 3 clean runs by the reviewer |
| BASE-32 D64/D78/D87/D98 | pass | refusal → prompt with reason; None writes nothing |
| AC-7 | pass | prompts always come back after a refusal |
