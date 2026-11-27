# FR-18 Terminal panel scroll jumps / flickers while a program streams output (AC-F2)
Worktree `.worktrees/FR-18` (branch fr-18, base master b2d1b61). Mode: bug, side fe. Skills called: `ai-dev-team:implementation`.
Tools available: Bash, Read, Edit, Write, Skill, ToolSearch, Artifact (unused), SubagentHandback.

## Progress
- 2026-11-27 10:02 read: rules_run3, AC-F2, M5 spec, capabilities, vr-term snapshot.rs / terminal.rs, vr-ide commands/terminal.rs, ui/terminal_view.rs, alacritty_terminal 0.26 grid scroll code.
- 10:04 red: two vr-ide tests added, both fail on the base code (`qa-artifacts/FR-18/red.log`).
- 10:06 code: vr-term view offset moved into the emulator; vr-ide uses `scroll_by` / `scroll_to_bottom`; output no longer resets the view.
- 10:07 related tests green (vr-term 32, vr-ide terminal 76).
- 10:08–10:10 live replay: base binary (b2d1b61) vs fr-18 binary, `walk.py` before = FAIL, after = PASS.
- 10:11 final related run + guard green; scratch base build removed.

## Root cause
Two faults, both on the path every output wake takes:
1. `commands::terminal::refresh` (runs on each wake from new output) set `tab.scroll = 0` for every tab with output — the M5 rule "any key or output returns to the bottom". A program that writes 20 times a second (claude's redraw loop, a flood) therefore threw a scrolled-back view to the bottom on every frame; each further wheel / shift+pageup went up again and was thrown down again → the jump / flicker. Evidence: red test `scrolled_view_holds_while_a_program_streams_output` failed at `assert!(offset > 0)` — scroll was 0 right after shift+pageup because pending output reset it; `wheel_scrolled_view_holds_while_output_arrives` failed `left: 0, right: 9` (`red.log`). Live: before-replay shows 49/49 distinct views and the live region visible in every sample after shift+pageup.
2. Even without the reset, the offset was kept by vr-ide as "lines above the bottom" and passed to `Terminal::snapshot(offset)`; every line the program pushed into the history moved the content under a constant offset, so the view would slide with the output instead of holding.
Not the cause (checked): `fit` resizes only on a size change; mouse routing — claude/the emitter do not enable mouse reporting, and programs that do still get the wheel at offset 0 (unchanged condition `modes.mouse_reporting && tab.scroll == 0`); sync output (DEC 2026) is handled inside vte's `Processor`.

## Fix
- `crates/vr-term/src/terminal.rs`: the view offset now lives in the emulator (alacritty's `display_offset`), which already anchors a scrolled view: `Grid::scroll_up` adds the pushed lines to the offset, capped at the scrollback limit. New `Terminal::scroll(lines: isize) -> usize` (clamped move, returns the offset) and `Terminal::scroll_to_bottom()`; `snapshot()` takes no offset any more and reports the emulator's (`Grid.scroll_offset`). Lock taken with the same reader-yield as snapshots (`Terminal::term()`).
- `crates/vr-term/src/snapshot.rs`: `capture` reads `display_offset` instead of a caller value.
- `crates/vr-ide/src/commands/terminal.rs`: `TermTab::scroll_by(isize)` / `TermTab::scroll_to_bottom()` (fake tabs clamp locally); shift+pageup/pagedown and the wheel call `scroll_by`; a key, a paste and `type_line` call `scroll_to_bottom`; `refresh` no longer resets the scroll on output. `TermTab.scroll` stays as the per-frame mirror of the snapshot's offset (set in `TermTab::refresh`).
- Contract change inside the workspace: `vr_term::Terminal::snapshot(scroll_offset)` → `snapshot()` + `scroll` / `scroll_to_bottom` (only caller is vr-ide; vr-term tests updated). capabilities.md line for vr-term needs this update (leader: `$TEAM_STATE` is outside my write scope).
- Decision for the leader to record: AC-F2 replaces the M5 spec line "any key or output returns to the bottom" (01_spec_M5.md:28-29) by "any key returns to the bottom; output keeps a scrolled view on its lines (capped at the scrollback)".

## Files changed
crates/vr-term/src/terminal.rs, crates/vr-term/src/snapshot.rs, crates/vr-term/tests/pty.rs, crates/vr-term/tests/flood.rs, crates/vr-term/tests/common/mod.rs, crates/vr-ide/src/commands/terminal.rs, crates/vr-ide/src/terminal_tests.rs (`git diff HEAD --stat`: 7 files, +252 −35; diff sha1 d167d1c5).

## Tests added
- `crates/vr-term/tests/pty.rs::scrolled_view_stays_on_its_lines_while_output_arrives` — screen identical after 30+ lines arrive while scrolled, offset growth == history growth, offset sits at the cap (200) past it, `scroll(-1)` → 199, `scroll_to_bottom` follows.
- `crates/vr-ide/src/terminal_tests.rs::scrolled_view_holds_while_a_program_streams_output` — real `/bin/sh` panel, output fed through a FIFO: Ink-style frames (DEC 2026 + cursor-up/erase-line + bracketed paste) then a 2000-line flood; shift+pageup view text unchanged, `↑` indicator shown, offset == start + pushed lines; a key → scroll 0 and new output followed.
- `crates/vr-ide/src/terminal_tests.rs::wheel_scrolled_view_holds_while_output_arrives` — same via the mouse wheel (offset 9 after 3 notches, held through 25 frames), wheel down to the bottom → follows output.
- `scroll_offset_shows_history_and_scrollback_is_capped` updated to the new API.

## Live replay (BASE-37) — `qa-artifacts/FR-18/walk.py` + `emitter.py`
vr-ide in a 100×30 pty, legacy keys (as Ubuntu's VTE), NUL opens the panel, `python3 emitter.py 1.5 4 3 5` runs in it (300 history lines, Ink-style redraw loop at ~20 fps, a 6000-line burst, a plain `yes` flood past the 10 000-line cap, redraw loop again); shift+pageup during the stream, a key, then 3 wheel notches (SGR) during the redraws. Each sample = emitter lines visible in the panel.
Before (binary built from b2d1b61, `walk-before.txt`):
```
shift+pageup during Ink redraws and a 6000-line burst: samples=49 distinct_views=49 with_live_region=49
after a key (follows the bottom): samples=10 distinct_views=10 with_live_region=10
wheel up during Ink redraws: samples=23 distinct_views=23 with_live_region=23
RESULT scrolled view held: False | follows after a key: True
```
After (fr-18 build of the final code, diff sha1 d167d1c5, `walk-after.txt`):
```
shift+pageup during Ink redraws and a 6000-line burst: samples=50 distinct_views=1 with_live_region=0 first=('hist 281', 'hist 282')
shift+pageup during a yes flood past the scrollback cap: samples=25 distinct_views=1 with_live_region=0 first=('y', 'y')
after a key (follows the bottom): samples=10 distinct_views=10 with_live_region=10
wheel up during Ink redraws: samples=23 distinct_views=1 with_live_region=0 first=('done 120', 'done 121')
RESULT scrolled view held: True | follows after a key: True
```
Past the scrollback cap the view sits on the oldest kept lines (the lines it showed are dropped) — the xterm.js / VTE model; with `yes` those lines are all `y`, so it looks still. A real `claude` session was not run (no account in the sandbox); the emitter reproduces its byte pattern.

## Commands run + result (trimmed)
- red: `run-heavy.sh cargo test -p vr-ide --lib -- scrolled_view_holds wheel_scrolled_view_holds` → `test result: FAILED. 0 passed; 2 failed` (`red.log`)
- `run-heavy.sh cargo test -p vr-term` → `ok. 7 passed`, `ok. 1 passed`, `ok. 1 passed; 1 ignored` (leak), `ok. 1 passed`, `ok. 21 passed` (pty), `ok. 0 passed` (`final-vr-term.log`)
- `run-heavy.sh cargo test -p vr-ide --lib -- terminal ui::tests::preview welcome` → `ok. 110 passed; 0 failed; 537 filtered out` (`final-vr-ide.log`)
- related-tests extras (`rule-checks.sh related-tests FR-18` also lists vr-config/tests/config.rs, vr-git/tests/{security,write,write_security}.rs): `ok. 23 passed`, `ok. 27 passed`, `ok. 26 passed`, `ok. 19 passed` (`final-other.log`)
- guard: `run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'` → `Finished dev profile … guard exit 0` (`guard.log`)
- rustfmt only on the 7 changed files.

## Deviations
- vr-term API changed (`snapshot()` without offset + `scroll` / `scroll_to_bottom`): the offset has to live where lines are pushed, under the emulator lock; a caller-held offset is stale by the time the next snapshot is taken.

## Out-of-scope issues noticed
- `refresh` still clears the mouse selection on every output of the active tab, so selecting text in a scrolled view while a program streams is lost each frame (selection is not part of AC-F2).
- vte's synchronized-update timeout (`Processor::sync_timeout`) is not enforced in `read_loop`: a program that starts DEC 2026 and stalls without ending it keeps its bytes buffered until more output arrives.

## Readers (BASE-47) — the view offset
| Reader | Status |
|---|---|
| `vr-term/src/snapshot.rs` capture (rows read at `Line(r - offset)`, cursor row + offset, `Grid.scroll_offset`) | fixed — reads the emulator's offset |
| `TermTab::refresh` (`commands/terminal.rs:161-162`) mirrors `grid.scroll_offset` into `tab.scroll` | already right |
| `TermTab::grid_now` / `screen_text` / selection copy (`selected_text` on the snapshot grid) | fixed — snapshot at the emulator's offset |
| `ui/terminal_view.rs:342-343` "↑ n/len" indicator | already right — mirror updated every frame (now grows with anchored output) |
| `ui/terminal_view.rs:363` cursor shown only at offset 0 | already right |
| `commands/terminal.rs:1185` wheel/mouse to the program only at offset 0 with mouse reporting | already right (unchanged condition, AC: programs that asked for the mouse keep the wheel) |
| shift+pageup/pagedown, wheel, key, paste, `type_line`, `refresh`, `restart` (new terminal starts at 0) | fixed (scroll_by / scroll_to_bottom; no reset on output) |
| vr-term tests (`pty.rs`, `flood.rs`, `common/mod.rs`) | fixed to the new API |

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| anchored scrollback offset | alacritty_terminal 0.26 `Grid::scroll_up` / `Term::scroll_display` | reused (emulator's `display_offset`) |
| pty walk of the real binary + screen model | capabilities.md → `qa-artifacts/T10/vt.py`, `pty_smoke.py` (T19 walk.py does not exist) | reused `T10/vt.py`, walk pattern from `pty_smoke.py` |
| PTY test helpers | `crates/vr-term/tests/common/mod.rs` | reused `spawn_opts`, `wait_for`, `screen`, `has_text` |
| panel test helpers | `terminal_tests.rs` | reused `term_app`, `draw`, `typ`, `wait_screen`, `frame_text`; new `fifo_program` / `emit` / `ink_frames` (no FIFO feeder existed) |
| Ink-style emitter | capabilities.md, qa-artifacts | new `qa-artifacts/FR-18/emitter.py` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | 7 files, all scrolling (`git diff HEAD --stat`); no keymap, tab bar, git or explorer change |
| BASE-2 | pass | no external contract; internal vr-term API change stated under Deviations |
| BASE-3 | pass | `red.log`: 2 failed before the fix; vr-term test written with the API change (compile-red, behaviour covered by the vr-ide red tests) |
| BASE-4 | pass | trimmed outputs above |
| BASE-5 | pass | tests/builds/guard through run-heavy.sh; walk kills vr-ide in `finally`; nothing running |
| BASE-6 | pass | English, why-comments only |
| BASE-7 | n-a | no commit |
| BASE-8 | n-a | no dependency |
| BASE-9 | n-a | no input boundary changed |
| BASE-10 | pass | only related tests + guard |
| BASE-11 | pass | no stash/checkout/reset; base built from `git archive b2d1b61` into qa-artifacts (deleted) |
| BASE-12 | pass | nothing outside the repo |
| BASE-13 | pass | only own scratch deleted: `qa-artifacts/FR-18/{base-src,run-base,run-after,vr-ide-base}` |
| BASE-14 | pass | foreground run-heavy runs, no waits |
| BASE-15 | pass | killed only the vr-ide child the walk forked (by pid) |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed; nothing in $TEAM_TOOLS |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines with `date` times |
| BASE-20 | pass | removed `snapshot(offset)` param: all callers (`grep -rn "\.snapshot("` → vr-ide terminal.rs:153/161, vr-term tests) updated |
| BASE-21 | n-a | no secrets |
| BASE-22 | pass | `scroll_by` / `scroll_to_bottom` / `scroll` follow neighbour verbs (`refresh`, `resize`, `write`) |
| BASE-23 | pass | paths: shift+pageup, wheel, key, wheel-down-to-bottom — each tested and walked |
| BASE-24 | pass | no index/history commands |
| BASE-25 | pass | guard exit 0 (`guard.log`) |
| BASE-26 | n-a | no rewrite |
| BASE-27 | n-a | no shared data |
| BASE-28 | pass | used alacritty_terminal's API, no third-party change |
| BASE-29 | n-a | no browser |
| BASE-30 | n-a | no records |
| BASE-31 | n-a | no containers |
| BASE-32 | pass | D207/D211 kept (keys built in, wheel 3 lines, mouse to programs); M5 "output returns to the bottom" replaced per AC-F2 — leader to record the decision |
| BASE-33 | n-a | no external system |
| BASE-34 | n-a | no blocked step |
| BASE-35 | n-a | no USER line on the terminal panel |
| BASE-36 | pass | inside the terminal panel scope |
| BASE-37 | pass | `walk-before.txt` FAIL / `walk-after.txt` PASS on real binaries + unit tests on `TermTab` / `Terminal` the panel renders |
| BASE-38 | pass | rustfmt on the 7 changed files only |
| BASE-39 | n-a | no schema |
| BASE-40 | n-a | no outward writes |
| BASE-41 | n-a | no credentials |
| BASE-42 | n-a | no test settings |
| BASE-43 | n-a | no access rules |
| BASE-44 | pass | fake tabs (`TermTab::fake`) clamp `scroll_by` locally |
| BASE-45 | pass | AC-F2 is in 01_spec_FR17-21.md:10 |
| BASE-46 | pass | walk-after ran on the build of diff sha1 d167d1c5, after the last runtime change |
| BASE-47 | pass | `## Readers` |
| BASE-48 | n-a | no machine state claimed |
| COST-1 | pass | heavy runs via run-heavy, named tests |
| COST-2 | pass | developer tests + own walk only |
| COST-3 | pass | no sleep/pgrep loops in my commands (test helpers poll with a deadline as existing `wait_screen` does) |
| COST-4 | pass | no full suite |
| COST-5 | pass | vr-ide killed in walk `finally`; no emitter left (`ps` empty) |
| COST-6 | n-a | no web sweep |
| COST-7 | n-a | no web data |
| COST-8 | pass | batched edits (perl for multi-site, Edit for blocks) |
| COST-9 | pass | nothing left running |
| COST-10 | pass | `rule-checks.sh related-tests FR-18` list run |
| COST-11 | n-a | no e2e |
| COST-12 | n-a | leader rule |
| COST-13 | pass | scripts in `qa-artifacts/FR-18/` |
| COST-14 | n-a | no CI |
| COST-15 | n-a | no snapshots changed |
| COST-16 | pass | logs in `qa-artifacts/FR-18/`, read via grep/tail |
| COST-17 | n-a | no goldens |
| COST-18 | n-a | no full run |
| COST-19 | pass | `walk.py` is the single check script (red before, green after) |
| COST-20 | pass | re-check = `python3 walk.py <bin> <workdir>` on the existing build |

## Quick pass 1
Finding (review minor): `refresh` cleared the mouse selection on every output wake, so a selection in a scrolled-back view was lost while a program streamed. Base: HEAD 4e46846 (wip FR-18 + master merge).
- 10:27 read finding + merged `commands/terminal.rs` (FR-20 quit-guard edits present, untouched).
- 10:29 red: `selection_in_a_scrolled_view_survives_streaming_output` FAILED `left: None, right: Some("287\n288\n289")` — nothing copied, the selection was cleared mid-drag (`qp1-red.log`); `selection_is_dropped_when_its_lines_leave_the_scrollback` passed on the base (guards the drop case).
- 10:30 fix, before/after replay, 10:31 related tests + guard.

Fix (`crates/vr-ide/src/commands/terminal.rs`):
- `TermTab::refresh_holds_view()` — takes the snapshot and tells whether the scrolled view still shows the same lines in the same rows: the old snapshot was in step with the view (`old.scroll_offset == scroll > 0`), the offset did not go down (a history clear `ESC[3J` or a scroll to 0 drops), same size, and either the view is below the top of the history (`offset < scrollback_len`: the emulator added exactly the pushed lines, so nothing slid) or the cells are unchanged (at the top the cap may have dropped lines under the view; only an unchanged screen proves it did not).
- `refresh`: the active tab's selection is kept only when the panel is visible, a selection exists and `refresh_holds_view()`; otherwise cleared as before. The extra snapshot happens only while a selection exists in a visible panel.
- `paste`: clears the selection when it returns the view to the bottom (a key already did, `handle_key`). A new press replaces it, the wheel clears it — unchanged.
- At the bottom (following output): today's behaviour kept (cleared on output). Keeping it there would need the number of lines each wake pushed, which the emulator does not report once the history is full; not simple and correct, so not done.
- Edge stated: at the scrollback cap a selection is dropped as soon as the view's text changes, also when only its top lines left (the selection's own rows may still be in the scrollback); moving it up by the dropped count needs that same unknown number.

Tests added (`crates/vr-ide/src/terminal_tests.rs`): `selection_in_a_scrolled_view_survives_streaming_output` (press, Ink frames, drag, frames, release → clipboard == text of those rows before the stream; more frames → still selected on the same text; paste → scroll 0, no selection), `selection_is_dropped_when_its_lines_leave_the_scrollback` (scrollback 400, flood 600 → view at the cap → selection dropped). Helpers `scrolled_fifo_app`, `left`.

Live replay (`qa-artifacts/FR-18/walk.py`, new `drag_copy` phase: SGR press / drag / release over three rows of the wheel-scrolled view while frames stream, copy read from the OSC 52 the binary writes):
- before (build of HEAD 4e46846, `qp1-walk-before.txt`): `selection dragged while streaming: want='done 123\ndone 124\ndone' copied=None` → `RESULT … selection copied while streaming: False`
- after (build of the final code, diff vs HEAD sha1 dda5b065, `qp1-walk-after.txt`): `want='done 122\ndone 123\ndone' copied='done 122\ndone 123\ndone'` → `RESULT scrolled view held: True | follows after a key: True | selection copied while streaming: True`
- earlier phases unchanged (distinct_views=1 while scrolled, follows after a key).

Commands (trimmed):
- `run-heavy.sh cargo test -p vr-ide --lib -- terminal ui::tests::preview welcome` → `ok. 113 passed; 0 failed; 477 filtered out` (`qp1-vr-ide.log`; vr-term not changed this round)
- guard `run-heavy.sh cargo fmt --all --check` && `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `Finished … guard exit 0` (`qp1-guard.log`)
- rustfmt only on the 2 changed files. Scratch base build (`base-src`, `vr-ide-base`, `run-*`) deleted.

Readers of the selection (BASE-47): `refresh` (fixed), `paste` (fixed), `handle_key` clears (already right), mouse press / drag / release / `copy_selection` / `selected_text` (already right — read the snapshot grid, which now keeps its lines), wheel clears (already right), resize / close / tab switch clear (already right, not affected), `ui/terminal_view.rs` draws the selection on the grid (already right).

Rule rows touched this round: BASE-3 pass (`qp1-red.log`), BASE-4 pass, BASE-25 pass (`qp1-guard.log`), BASE-37 pass (qp1-walk before/after), BASE-46 pass (walk on the build of diff dda5b065), BASE-47 pass (above), BASE-1 pass (2 files, selection only; DEC 2026 timeout left out), COST-19 pass (same `walk.py`, red before / green after). Other rows unchanged. Nothing left running (no emitter / vr-ide process).
