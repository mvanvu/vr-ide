# Review FR-29 (F4-1..F4-5), round 1

Tools available to the reviewer: Read, Bash (read-only use plus one `run-heavy.sh cargo test -- f4_`), Write (this report only), Skill, SendMessage, SubagentHandback.

`cargo test -p vr-ide --lib -- f4_` (run-heavy): 6 passed, 0 failed.

## Findings

[major] crates/vr-ide/src/app.rs:1108-1111 — deferred info notices after the first still expire unseen (F4-1 only partly fixed)
Scenario: SSH A is in full screen. Sessions B and C become ready meanwhile, so two notices are queued ("SSH B ready …", "SSH C ready …"). Each tick in full screen sets both `expires` to the same `now + TTL`. After ctrl+` only the front notice is drawn ("SSH B ready … (+1 more · click)"). Both notices are dropped together 5 s later, so C's notice is never drawn unless the user clicks within that time. Before this fix the IDE layout rarely had several notices at once. Full screen now collects them for minutes, and README says they "are shown when it is back". The F4-1 scenario itself (a second session finishing its probe or login) reaches this state.
Fix: stagger the floor by queue position: the k-th timed notice gets `max(t, now + TTL * (k+1))`, so each one gets a full TTL once it reaches the front. A more robust option is to start a notice's TTL only when it becomes the front notice. Test: two `info()` calls in full screen, tick +30 s, ctrl+`, tick +6 s → the second notice is still there and drawn.

[minor] crates/vr-ide/src/ui/ssh_view.rs:78 — at 17-21 columns the notice count is shown in place of the key back to the editor (design QA)
Scenario: in a window 17-21 cells wide, `hint_w` is 0 because the hint needs 15 cells + 2 and does not fit. `count_w` still passes `0 + 10 + 2 <= room`, so the row shows ` SSH  w… 1 notice ` without ` ctrl+` editor `. This breaks the row's own rule that the way back outranks everything else (AC-S3: the status row shows the key back).
Fix: show the count only when the hint is shown: `let count_w = if hint_w > 0 && hint_w + count_w + 2 <= room { … }`.

[minor] crates/vr-ide/src/commands/terminal.rs:1333-1335, 1343 — the click check compares only the column
Scenario: press on a logged-in SSH tab, drag straight down into the grid in the same column, release there → full screen starts, although the mouse moved. On a bar that draws only the active tab, with hidden tabs before it, a same-column drag over its first two cells calls `drag_tab_to`, which moves the tab one place toward the front. `full_on_release` survives that move, so the release still enters full screen on a tab that was just moved. No mouse bytes reach the pty in either case.
Fix: store `(column, row)` and clear it on any Drag event to a different cell. Alternatively, clear it whenever `drag_tab_to` moves the tab.

[minor] crates/vr-ide/src/ssh_tests.rs:1191 — f4_4 does not assert that only one notice is raised
`held_in_full_screen` claims that repeated presses do not stack notices. The test runs grow, shrink and toggle_position (the same notice text three times) but checks only `contains("IDE layout")`, so a regression that stacks three copies (shown as "3 notices") would still pass.
Fix: assert that `f.app.notices.iter().filter(|n| n.text.contains("IDE layout")).count() == 1`.

Checked, no defect: release outside the window (the next Down clears both fields); tab closed between press and release (close_tab clears tab_drag, and the stale `full_on_release` is rewritten by every tab press); session exits before the release (`enter_full` checks `active_session_live`); remote program with ?1002h (the press is on the bar, Drag/Up are consumed in the tab_drag branch, so no bytes reach the pty, as f4_2 asserts on the program's input). The floor runs every 16 ms in full screen (`poll_interval`) and before key handling, so a notice has ≥ 1 TTL left when the layout returns. Dialog and error notices (`expires = None`) are not touched. The queue grows only through event-driven notices, so I am not filing unbounded growth. `focus_editor` → `hide`: the palette closes before the command runs, focus is back on the terminal, and `hide` moves it to the editor, which is the same path as ctrl+`. `held_in_full_screen` also covers the palette open over full screen.

Flaky `ssh::tests::sec_s4_a_hanging_probe_is_killed_with_its_group_at_the_timeout` (Failed after 2.98 ms): this is not a product defect. In `probe` (ssh/mod.rs:258) the only path that returns Failed before the deadline, given a script that sleeps, is `cmd.spawn()` failing (or a poll/read error, which is unlikely). The test writes its fake `ssh` script just before running it while other test threads fork pty children in parallel. A fork in that window inherits the script's write fd until its exec, and exec of the script then fails with ETXTBSY: the known Rust multi-threaded test race. Returning `Probe::Failed` on a spawn error is the correct fail-closed result in the product (the real ssh binary is not freshly written). The fix belongs in the test: retry the spawn on ETXTBSY in a test helper, or have the script create a marker file and retry when it is missing. Cause marked `uncertain` because the failing run left no errno.

## Files read
| File | Status |
|---|---|
| README.md | read |
| docs/keymap.md | read |
| crates/vr-ide/src/app.rs | read (diff + tick, notify, dismiss callers) |
| crates/vr-ide/src/commands/terminal.rs | read (diff + hide, enter_full, full_screen, step_size, drag_tab_to, handle_mouse) |
| crates/vr-ide/src/ui/ssh_view.rs | read |
| crates/vr-ide/src/ssh_tests.rs | read (diff) |
| crates/vr-ide/src/ssh/mod.rs, ssh/tests.rs | read (probe + test, for the flake question) |

## Rule check FR-29
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to the findings' files; F4-6 left to the leader |
| BASE-3 | pass | red_tests.txt cited; f4_1..f4_4 fail for the expected reason before the change (notes) |
| BASE-6 | pass | English why-comments at app.rs:1105, terminal.rs:501, 1341, 1414, ssh_view.rs:71 |
| BASE-23 | pass | notice via info() and ctrl+f8; full screen via click, ssh list, login; Focus Editor via command; grow/shrink/position each tested |
| BASE-25 | pass | fmt + clippy exit 0 (guard.log per notes) |
| BASE-32 | pass | D325 deferral implemented (partly, see major finding); D316 keys still kept by vr-ide and not sent to the session; D317/D320 flow unchanged; D312 drag now also works for SSH tabs |
| BASE-37 | pass | replay before 60/62, after 62/62, FR-19 walk 19/19 on sha1 321888bc1beb |
| BASE-47 | pass | readers of `Notice.expires` (app.rs:956, 1110, 1116) and of `notices.len()` (ssh_view) match the notes' Readers section |

CHANGES_REQUESTED

## Re-check round 1

`cargo test -p vr-ide --lib -- f4_` (run-heavy): 9 passed, 0 failed.

| Earlier finding | Result |
|---|---|
| [major] app.rs one expiry for every waiting notice | fixed. app.rs:1105-1116: in full screen, place k gets `max(t, now + (k+1)·TTL)`, so after ctrl+` each notice is in front for at least one TTL. Test f4_1_each_waiting_notice… |
| [minor] ssh_view.rs:78 count without the key back | fixed: `hint_w > 0 &&` guard; f4_1_the_notice_count… covers 17..=21 and 40 columns |
| [minor] terminal.rs column-only click check | fixed: `(column, row)`, cleared on a drag off the cell and when `drag_tab_to` reports a move (every move path returns `true`, every no-move path returns `false`); f4_2_a_press_that_moved… |
| [minor] f4_4 single notice | fixed: asserts count == 1 |

Questions from the brief:
- **The k-TTL rule cannot keep notices forever.** The floor is applied only while `full_screen` is true, and holding notices back then is the intent (D325). Once full screen ends nothing raises `expires` any more, so the last notice goes at most `n·TTL` after the last full-screen tick. Notices raised in the IDE layout age normally (5 s). The queue grows only from event-driven notices (session ready or logged in, at most 16 sessions; the held notice is deduplicated), as in round 0. No periodic source exists, so the queue cannot grow without bound.
- **The test-helper change cannot hide a product failure.** `write_executable` (ssh_tests.rs:98) only changes how the fake ssh file is created. The write handle on the final path exists only inside the `install` child, which the test waits for, so no fork in the test process can inherit it. The `.src` file is never executed. The product `probe` is unchanged. `sec_s4_a_hanging_probe…` still asserts `Failed` and ≥ 400 ms, so a product path that fails fast would still turn the test red.
- The sixteen-sessions test now waits for ≥ 16 `S ` lines and then asserts `sessions().len() == 16`. A 17th spawn that has already logged still fails the test, and `tabs.len() == MAX_SESSIONS` is checked before that. This is a valid wait, not a weaker check.

No new findings. Files read this round: app.rs, ui/ssh_view.rs, commands/terminal.rs (handle_mouse drag branch, drag_tab_to), ssh_tests.rs (helper, sixteen-sessions test, new f4_* tests), ssh/tests.rs (diff).

Rule check: unchanged rows still pass. BASE-3: r1_red.log (3 new tests red; f4_4's assertion is a guard). BASE-25: r1_guard.log exit 0. BASE-37: walks 2/2 and 62/62 on sha1 aace835b140e. BASE-47: `drag_tab_to` bool has one caller; readers of `expires` unchanged.

APPROVE
