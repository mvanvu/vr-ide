# FR-29 Final review 4 fixes (F4-1..F4-5)
Worktree: .worktrees/FR-29 (branch fr-29 from 260d08b)

## Progress
- read findings 06_final_review_4.json, code of ui/mod.rs, app.rs, commands/terminal.rs, commands/ssh.rs
- docs: docs/keymap.md Terminal row (SSH reconnect), README SSH section (recent timing, full-screen keys, notices, tab drag)
- red: 4 tests in crates/vr-ide/src/ssh_tests.rs (f4_1..f4_4) all fail for the expected reason — qa-artifacts/FR-29/red_tests.txt
- green: f4_1..f4_4 pass (fix in app.rs tick, ui/ssh_view.rs marker, commands/terminal.rs)
- built before (git archive 260d08b) / after binaries; replay.sh running
- walks: fr25 before 60/62 (F4-1, F4-2 red), after 62/62; FR-19 walk 19/19 on the fixed binary
- related tests + guard rerun to logs; notes completed

## Findings
| Finding | Reply | Change |
|---|---|---|
| F4-1 MAJOR | fixed | `App::tick` (crates/vr-ide/src/app.rs:1104-1114): while `full_screen`, every timed notice keeps `expires >= now + INFO_NOTICE_TTL`, so it is never dropped unseen and has at least a whole TTL once the IDE layout is back (D325). `ui/ssh_view.rs` render_status: ` 1 notice ` / ` N notices ` before the key hint (dropped first when the row is narrow, target is cut before it) |
| F4-2 MAJOR | fixed | `commands/terminal.rs`: new `TerminalPanel.full_on_release: Option<u16>` (press column on a logged-in SSH tab). Press = tab_drag only; a Drag to another column clears it and reorders as any tab (FR-19); the release at the press column calls `enter_full` after the release, so no part of the click reaches the program; a new press clears it |
| F4-3 minor | fixed | `focus_editor`: in `full_screen` it calls `hide` (exactly what ctrl+` does there) instead of the "editor covered" early return |
| F4-4 minor | fixed | `held_in_full_screen` guards `step_size` (grow/shrink) and `toggle_position`: no change, one info notice "The terminal panel changes in the IDE layout (ctrl+` gives it back)" (not stacked on repeats; deferred per F4-1). `/term right|bottom` (explicit `set_position`) unchanged |
| F4-5 minor | fixed | docs/keymap.md Terminal row: "(an SSH tab connects to the same target again)"; README SSH: recent list "recorded after a successful login, or after a clean exit when `ssh_fullscreen = false`"; also the full-screen bullets now say ctrl+f7/f8/f9 only notify there, notices wait (row counts them), and dragging a tab moves it instead of entering full screen |
| F4-6 | not in brief | capabilities.md left to the leader (not assigned to FR-29) |

## Files changed
- crates/vr-ide/src/app.rs, crates/vr-ide/src/commands/terminal.rs, crates/vr-ide/src/ui/ssh_view.rs
- crates/vr-ide/src/ssh_tests.rs (4 tests), README.md, docs/keymap.md
- qa: qa-artifacts/FR-25/qc_walk.py (+ `login_full`, N-F41, T-F42; Enter retry when the window opened late), qa-artifacts/FR-29/fr29_walk.py (only the two cases), replay.sh, red_tests.txt, fr25_walk_before.txt, fr25_walk_after.txt, fr19_walk_after.txt, related_tests.log, guard.log

## Tests added (crates/vr-ide/src/ssh_tests.rs)
- f4_1_a_notice_raised_in_full_screen_waits_for_the_ide_layout — marker on the SSH row, tick +30 s still there, ctrl+` → drawn, gone only after a further TTL
- f4_2_dragging_a_logged_in_ssh_tab_moves_it_and_sends_nothing — remote has ?1002h/?1006h and a raw one-byte reader; press alone keeps the bar, drag reorders (ssh web1 → index 1, active), not full screen, the reader's first byte is the typed `q`; click (press+release in place) → full screen
- f4_3_focus_editor_from_full_screen_gives_the_editor_back
- f4_4_panel_size_and_place_keys_do_nothing_in_full_screen

Red first (qa-artifacts/FR-29/red_tests.txt): f4_4 `left: Right right: Bottom`; f4_3 screen still the session; f4_1 status row without "1 notice"; f4_2 "the press alone keeps the bar".

## Commands run + result
```
$ run-heavy.sh cargo test -p vr-ide --lib -- f4_
test result: ok. 6 passed; 0 failed   (4 new + 2 unrelated f4 key tests)
$ run-heavy.sh cargo test -p vr-ide --lib -- ssh terminal app_tests ui::   (log: qa-artifacts/FR-29/related_tests.log)
first run after the walks: 256 passed; 1 failed (ssh::tests::sec_s4_a_hanging_probe_is_killed_with_its_group_at_the_timeout: took 2.98ms < 400ms)
single rerun: test result: ok. 2 passed; 0 failed
earlier run on the same code: test result: ok. 257 passed; 0 failed; 2 ignored
$ rule-checks.sh related-tests FR-29 → extra files mentioning changed names (README/keymap.md): vr-config tests/config.rs, vr-git tests/security.rs, vr-syntax tests/registry.rs, vr-term tests/env.rs, leak.rs, pty.rs; ui/tests/* are in `ui::`
$ run-heavy.sh cargo test -p vr-config --test config -p vr-git --test security -p vr-syntax --test registry -p vr-term --test env --test leak --test pty
ok 24 / 27 / 6 / 1 / 1 / 21 passed, 0 failed
$ run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'   (log: guard.log)
Finished `dev` profile ... exit 0
```
not mine: crates/vr-ide/src/ssh/tests.rs — sec_s4_a_hanging_probe returned Failed after 2.98 ms under load (the fake script ran while 3 pty walks had just finished; passes on rerun and in the earlier related run).

Snapshots: none regenerated; insta snapshots in `ui::` and ssh_tests (`ssh_full_screen_60x8`, welcome_*, preview) pass unchanged (the notice count shows only with notices).

## Replay on the binary (BASE-37)
Before: `git archive 260d08b` built with the worktree target dir → scratch `vr-ide-before` sha1 0cd909cd880c. After: worktree target/debug/vr-ide sha1 321888bc1beb (worktree 260d08b + this diff).
```
before  FAIL N-F41 ... marker=' SSH  web1   ...  ctrl+` editor '            (no count, notice gone after 7 s)
before  FAIL T-F42 ... moved=False full_after_drag=True keys=b'\x1b' full_after_click=True
before  RESULT 60/62 passed
after   PASS N-F41 [F4-1] notice raised in full screen: counted on the SSH row, kept past its TTL, drawn after ctrl+`
after   PASS T-F42 [F4-2] drag of a logged-in SSH tab reorders it, no full screen, no mouse byte reaches the program; click still enters full screen
after   RESULT 62/62 passed
FR-19 qc_walk.py <after> 24 80: [80x24] SUMMARY 19/19 pass
```
The first after-run failed T-F42 at the harness's first `/ssh` (Enter inside the window's settle time on a loaded machine, so the reader command went into the command window); `login_full` now re-sends Enter while the window is still open; fr29_walk.py then passed 4/4 runs, and the full walk 62/62.

## New states (round 1)
`full_on_release` (press column on a logged-in SSH tab):
- the release lands outside vr-ide: the next press clears it (MK::Down branch); other resets of tab_drag (hide, set_position, enter_full, step) make the branch unreachable, and every tab press rewrites it.
- the session exits between press and release: `enter_full` checks `active_session_live`, so a click on a dead session does nothing.
- a dialog opens between press and release: `full_screen` requires no dialog; enter_full sets `full`, which only shows once the dialog closes (as a tab click before).
Notice floor while full screen: a dialog or ctrl+` ends full_screen → aging resumes from the floor (≥ one TTL left).

## Readers (BASE-47)
`Notice.expires`: App::tick (retain, now floored in full screen), App read-mode hint (app.rs ~955 refreshes it), status_bar::notice_rows / render_notice (draw only). `app.notices.len()`: new reader ui/ssh_view.rs render_status. Dismiss (Esc / click on notice) unchanged; in full screen Esc goes to the session as before.

## Deviations
- F4-2: fixed as the reviewer's first suggestion (enter on release), not by swallowing events.
- The `full_on_release` field and its init were inserted with one `perl -0pi` command (two lines) instead of the Edit tool; every other edit used Edit/Write.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| leave full screen | terminal.rs `hide` (what ctrl+` runs) | reused in focus_editor |
| one-shot notice without stacking | app.rs read-mode hint pattern | same `notices.iter().any(text)` check |
| binary walk harness | qa-artifacts/FR-25/qc_walk.py `Run`, late_probe_walk.py exec pattern | extended in place; fr29_walk.py execs its helpers |
| tab label positions in tests | `ui::terminal_view::tab_bar` (FR-19 tests) | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to the 5 findings' files (git diff --stat above); F4-6 left to leader |
| BASE-2 | pass | no contract change; AC-S3..S5, AC-F3, AC-F6 behaviours kept (ac_s4_* tests pass, click still enters full screen) |
| BASE-3 | pass | red_tests.txt: 4 tests failing for the expected reason before the code |
| BASE-4 | pass | trimmed outputs above, logs related_tests.log, guard.log |
| BASE-5 | pass | tests/builds/guard through run-heavy.sh; walks ran as background tasks, all pty children quit/killed by the script |
| BASE-6 | pass | English, why-comments only (app.rs:1104, terminal.rs:503, 1340, 1413, ssh_view.rs:71) |
| BASE-7 | pass | no commit, nothing staged |
| BASE-8 | n-a | no dependency |
| BASE-9 | pass | no new input boundary; status row text is a count, the target text path unchanged (SEC-S12) |
| BASE-10 | pass | only `ssh terminal app_tests ui::` + rule-checks related files + guard; no full suite |
| BASE-11 | pass | worked in .worktrees/FR-29 only; before-binary from `git archive` into scratch, no stash/checkout |
| BASE-12 | pass | nothing written outside the repo except scratchpad |
| BASE-13 | pass | only my scratch dirs removed (w-before, w-after, w29, w-19) |
| BASE-14 | pass | long walks as background tasks, turn ended, no polling |
| BASE-15 | pass | no kill by name; walk script stops its own children |
| BASE-16 | pass | ## Reuse table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created first, progress lines per step |
| BASE-20 | n-a | nothing removed (the press-time `enter_full` call moved to the release) |
| BASE-21 | pass | walk output holds only the fake password constant's absence checks; no secrets |
| BASE-22 | pass | `full_on_release`, `held_in_full_screen`, test names `f4_*` follow neighbours (`tab_drag`, `sec_s*`/`ac_s*`) |
| BASE-23 | pass | entry paths: notice via ctrl+f8 (walk) and info() (test); full screen via click (test+walk), ssh list (ac_s4), Focus Editor via command (f4_3); grow/shrink/position each in f4_4 |
| BASE-24 | pass | no git index/history command |
| BASE-25 | pass | guard exit 0 (guard.log) |
| BASE-26 | n-a | no rewrite |
| BASE-27 | n-a | no shared data |
| BASE-28 | n-a | own code only |
| BASE-29 | n-a | no browser |
| BASE-30 | n-a | no shared app |
| BASE-31 | n-a | no containers |
| BASE-32 | pass | D325 notices deferred (now implemented), D316 ctrl+f8/f9 still kept by the terminal (they no longer resize while full screen, per F4-4), D317/D320 full-screen flow unchanged, D312 terminal tab drag now also for SSH tabs |
| BASE-33 | n-a | no external system |
| BASE-34 | n-a | no blocked step |
| BASE-35 | pass | USER-1/USER-5: key in the notice from key_hint("terminal.toggle"), not hard-coded |
| BASE-36 | pass | within vr-ide terminal/SSH scope |
| BASE-37 | pass | replay before 60/62 (both red) / after 62/62 on rebuilt binary sha1 321888bc1beb; FR-19 walk 19/19 |
| BASE-38 | pass | rustfmt only on app.rs, ssh_view.rs, terminal.rs, ssh_tests.rs |
| BASE-39 | n-a | no schema |
| BASE-40 | n-a | no outward write |
| BASE-41 | n-a | no credentials |
| BASE-42 | n-a | no test settings changed |
| BASE-43 | n-a | no access rule |
| BASE-44 | pass | the ssh shim fake drives both flows (mouse modes via `post`, raw reader) |
| BASE-45 | pass | AC-S3..S5, AC-F3, AC-F6 unchanged in 01_spec_FR17-21.md; no new criterion |
| BASE-46 | pass | walks ran on sha1 321888bc1beb, built after the last code edit |
| BASE-47 | pass | ## Readers |
| BASE-48 | pass | binary content checked (grep for the new notice text) before the replay; walk states read from the screen |
| COST-1 | pass | heavy via run-heavy.sh |
| COST-2 | pass | dev tests + replay requested by brief only |
| COST-3 | pass | no sleep loops/pgrep (the walk harness's own pty pumps only) |
| COST-4 | pass | no full suite |
| COST-5 | pass | no browser/server; pty children ended |
| COST-6 | pass | only the SSH full-screen states swept |
| COST-7 | n-a | no web |
| COST-8 | fail | batched work; one `perl -0pi` two-line insert instead of Edit (see Deviations); all other edits via Edit |
| COST-9 | pass | nothing left running (both background tasks completed) |
| COST-10 | pass | rule-checks related files run (6 extra test targets) |
| COST-11 | n-a | no e2e target |
| COST-12 | n-a | leader rule |
| COST-13 | pass | scripts under qa-artifacts/FR-29 (FR-25 walk extended in place, allowed by brief) |
| COST-14 | n-a | no CI |
| COST-15 | pass | no snapshot regenerated; snapshot tests run once on the final code |
| COST-16 | pass | final related/guard runs logged to qa-artifacts/FR-29/*.log, only tails read |
| COST-17 | n-a | no image conflicts |
| COST-18 | n-a | no full run |
| COST-19 | pass | the two binary checks live in the one FR-25 walk (fr29_walk.py runs that same section) |
| COST-20 | pass | re-checks of the flake used fr29_walk.py (2 cases) instead of the full walk |

## Fix round 1
- read 05_review_FR-29.md (1 major, 3 minor) + ride-along flakes (sec_s4_a_hanging_probe, ac_s6_sec_s16_sixteen)
- r1 red: 3 new tests fail as expected (qa-artifacts/FR-29/r1_red.log); code fixed; green run r1_green.log
- r1: 10x `cargo test -p vr-ide --lib ssh` green; related + guard green; binary rebuilt (sha1 aace835b140e); walks 2/2 and 62/62

### Findings round 1
| Finding | Reply | Change |
|---|---|---|
| MAJOR app.rs:1108 one expiry for all waiting notices | fixed | `App::tick`: in full screen the notice at queue place k gets `expires >= now + (k+1)·TTL` (the row draws the front notice), so after ctrl+` each one is in front for at least one whole TTL. Test `f4_1_each_waiting_notice_gets_its_own_ttl_after_full_screen` (two notices, +30 s, ctrl+`, +6 s → first gone, second drawn; +11 s → both gone) |
| minor ssh_view.rs:78 count without the key back | fixed | the count is shown only when the hint is shown (`hint_w > 0 && …`). Test `f4_1_the_notice_count_never_takes_the_key_back_s_place` (17..=21 cols: no count; 40 cols: count + `editor`) |
| minor terminal.rs:1333/1343 column-only click check | fixed | `full_on_release: Option<(u16, u16)>` stores (column, row); a Drag to another cell clears it, and `drag_tab_to` now returns whether it moved the tab, which also clears it; the release must land on the press cell. Test `f4_2_a_press_that_moved_off_its_cell_is_no_click` (vertical drag in the same column; drag off and back; plain click still enters) |
| minor ssh_tests.rs f4_4 | fixed | asserts exactly one "IDE layout" notice after grow + shrink + toggle_position |
| ride-along sec_s4_a_hanging_probe flake | fixed (test only) | new test helper `ssh_tests::write_executable` writes the body to `<name>.src` and has `install -m 0755` copy it, so the test process never holds the script open for writing; a child another test thread forks can no longer inherit a write fd and make the exec fail with ETXTBSY. Used by `ssh/tests.rs::script` and the `fx_with` shim. Product `probe` (spawn error → Failed) unchanged |
| ride-along ac_s6_sec_s16 flake | fixed (test only) | the shim being alive (`running()`) does not mean it has written its `S …` line to the log yet: the test now waits for 16 log lines before asserting `sessions().len() == 16`. The shim is also written with `write_executable` (16 quick spawns were the most exposed to ETXTBSY) |

Red first (r1): qa-artifacts/FR-29/r1_red.log with the app.rs/ssh_view.rs changes temporarily undone and terminal.rs not yet changed: `f4_1_the_notice_count…` FAIL (`17: " SSH  … 1 notice "`), `f4_1_each_waiting_notice…` FAIL (second note not drawn at +6 s), `f4_2_a_press_that_moved…` FAIL (full screen after a vertical drag). f4_4's new assertion passes on the old code too: it guards a regression and is not a red test.

Commands round 1:
```
$ run-heavy.sh cargo test -p vr-ide --lib -- f4_ sec_s4_a_hanging ac_s6_sec_s16   (r1_green.log)
test result: ok. 12 passed; 0 failed
$ for i in 1..10: run-heavy.sh cargo test -p vr-ide --lib ssh   (r1_ssh_10x.log)
10 × "test result: ok. 61 passed; 0 failed"; sec_s4_a_hanging_probe… ok 10/10, ac_s6_sec_s16_sixteen… ok 10/10
$ run-heavy.sh cargo test -p vr-ide --lib -- ssh terminal app_tests ui::   (r1_related_tests.log)
test result: ok. 260 passed; 0 failed; 2 ignored
$ run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'   (r1_guard.log)
exit 0
$ walks on target/debug/vr-ide sha1 aace835b140e (rebuilt after the last edit)
fr29_walk.py: PASS N-F41, PASS T-F42, RESULT 2/2   (r1_fr29_walk.txt)
FR-25 qc_walk.py: RESULT 62/62 passed              (r1_fr25_walk_after.txt)
```
Snapshots: none regenerated; the snapshot tests in the related run pass unchanged.
rustfmt ran only on app.rs, ui/ssh_view.rs, commands/terminal.rs, ssh_tests.rs, ssh/tests.rs. The unused `PermissionsExt` imports in ssh_tests.rs and ssh/tests.rs were removed (Edit).

### New states (round 1)
- `full_on_release` is now a cell. It is cleared on any Drag off the cell, on any tab move, and on a new press. A release elsewhere does not enter. Release outside the window, session exit, and a dialog behave as in round 0.
- Staggered floor: a warning or error in the queue (no expiry) still counts as a place, so the info notices behind it keep extra TTLs. They can still expire behind a warning that stays, which is the normal IDE behaviour outside full screen.

### Readers round 1
`Notice.expires`: App::tick (staggered floor + retain); the read-mode hint refresh (app.rs ~955); status_bar draw. `notices.len()`: ssh_view count (now only together with the hint). `drag_tab_to`'s new bool has one caller (handle_mouse Drag).

### Rule compliance changes (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | r1_red.log: 3 new tests fail before the fix (the code was temporarily undone to see red, then restored) |
| BASE-4 | pass | outputs above + r1_*.log |
| BASE-10 | pass | only the related set, the 10× ssh loop the brief asked for, and the guard |
| BASE-23 | pass | count/hint at 17..=21 and 40 columns; click versus vertical drag, drag-and-back and tab move |
| BASE-25 | pass | r1_guard.log exit 0 |
| BASE-37 | pass | fr29_walk 2/2 and FR-25 walk 62/62 on rebuilt sha1 aace835b140e |
| BASE-38 | pass | rustfmt on my 5 files only |
| BASE-46 | pass | walks ran after the last code edit, on the binary built from it |
| BASE-47 | pass | Readers round 1 |
| COST-8 | pass (r1) | all round-1 edits with Edit; the round-0 perl insert stays recorded as that round's fail |
| COST-15 | pass | no snapshots regenerated |
| COST-16 | pass | r1 logs under qa-artifacts/FR-29, only tails read |
All other rows unchanged from round 0.
