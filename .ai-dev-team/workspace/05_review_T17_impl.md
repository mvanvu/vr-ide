# Review T17 (implementation + design QA) - CHANGES_REQUESTED

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings
[major] crates/vr-ide/src/ui/welcome.rs:337-345 (`keep`) - Compact layout shows no start keys on terminals narrower than 64 columns once 6 or more rows are free (AC-1, USER-1)
Scenario: `keep(hints, n)` takes `by_priority.get(n-1)`; with only 5 hints (Terminal has no key yet) any n >= 6 gives `None`, so every hint is filtered out. Real pty captures (release build, Esc to clear the keyboard notice): 50x24 and 60x14 show CPU and RAM and then an empty screen, no keys at all; 40x13 (n = 5) shows 5 keys, 40x12 shows 4. Every phone-width or narrow split pane that is tall enough loses the keys the screen exists to show. The tests only use 40x12.
Fix: `let limit = by_priority.get(n.saturating_sub(1)).or(by_priority.last()).copied();` and add a compact test at 50x24 and 60x14 asserting all 5 keys (and 1 via n = 1).
Evidence: qa-artifacts/T17/review/det-50x24-clean.txt, det-60x14-clean.txt, det-40x13-clean.txt.

[minor] crates/vr-ide/src/ui/welcome.rs:632 and :653 - cores beyond the room are dropped without a marker
Scenario: Full draws at most 12 cores (6 rows x 2), Medium cuts the `CORE 0▁ 1▁ ...` row at the last whole entry; a 16 or 32 thread CPU shows C0..C11 (or 0..7) with nothing saying more exist. This box has 8 threads so I could not see it. Cost of a fix: one `+N` cell on the row. The user decides.
Fix: append `+{rest}` (dim) when `cores.len()` is more than shown.

[minor] crates/vr-ide/src/ui/welcome.rs:515-518 - label says `60 s`, graph is 58 columns at 120 wide
Scenario: right column is 58 wide, the graph keeps the last 58 samples, the header says `60 s`. Fix: print `{graph width} s` or widen to 60 (right column cap).

[minor] crates/vr-ide/src/app.rs:85-91 - `RECENT_MAX` was inserted between the `DISMISS_KEY` doc comment and its const
Scenario: the doc of `DISMISS_KEY` now documents `RECENT_MAX` and `DISMISS_KEY` has none. Fix: move `RECENT_MAX` above the "The key that dismisses" comment.

## Code review notes (no finding)
- Parsing: cpu line order user nice system idle iowait irq softirq steal, idle = idle+iowait, guest not double counted; cores keep their slot, hot-plugged core count differences are handled by `zip`; counters going backwards give `None` (no wrap); first sample gives no delta; MemAvailable (not MemFree), SwapTotal 0 gives `—` percent without division by zero (`mem_pct` filters 0; real run showed it); `/proc/self/stat` counted from the last `)` (test has a name with a space and parenthesis); own CPU uses one core's share of the same jiffy window.
- Ring bounded at 60 (test); sampling only from `App::tick` when `docs.is_empty()`, no thread or timer, baseline dropped when hidden; loop polls every 50 ms so the 1 s cadence is real (numbers changed between my 1.6 s and 4.6 s captures at all 3 sizes).
- git HEAD: regular file only, <= 4 KiB, FIFO skipped (tests), worktree `.git` file with relative or absolute gitdir, detached HEAD shows 7 hex (real run: `0123456`), ref-name control characters are dropped by ratatui `set_stringn`. No UI-thread blocking.
- `Theme.ok` is a required field, so every theme sets it (ansi16 Green, light Indexed(28), palette themes green); tracks use the `line` token and carry no information (fill and number do).
- README and bench script match the build (`$target_dir` is defined).

## Design QA
Captures: qa-artifacts/T17/review/{empty,git,det}-WxH-*.png and .txt (release build, pty, truecolor, empty temp dir and a dir with `.git/HEAD`), compared with qa-artifacts/T17/qa/b-*.png (option B, r2).
- 120x40: matches B: left START/RECENT column, `│` divider, SYSTEM right column with one right edge for every value (percent, GiB, LOAD/UP, own figures), thresholds green/orange/red from the real values (C4 72 % orange, C2 100 % red), honest bars (cell count = round(pct x width), `░` track, 2 % shows empty), units on every number (`GiB`, `MiB`, `%`, `UP 2d 11h`). Differences from the mockup are logged deviations (no Terminal row, RECENT only when files were opened, key column 14 not 25). Empty space under the left column is the same as the mockup.
- 80x24: one-row `CORE 0▁ 1▁ ...` glyph row, RAM and SWP bars with `used/total GiB  %`, LOAD/UP/VR-IDE right aligned; nothing clipped or wrapped; matches B-80.
- 40x12: name + branch, root, CPU spark + %, RAM bar + `used/total GiB  %`, 4 keys: matches the B fallback; with the legacy-keyboard notice showing (15 s or until Esc) the notice takes rows and only 1 key remains (existing notice behaviour, not a T17 defect).
- Live numbers: CPU %, per-core bars, RAM, load changed between 1.6 s and 4.6 s at every size; the history graph fills over time (1-3 columns after a few seconds, as expected).
- Only deviation that fails: the compact key bug above (blocker for narrow tall terminals in design terms, filed as major).

## Files read
crates/vr-ide/src/monitor.rs read; crates/vr-ide/src/ui/welcome.rs read; crates/vr-ide/src/ui/tests/welcome_tests.rs skipped - only grepped for the sizes it covers; ui/tests/snapshots and the 2 changed empty-project snapshots skipped - generated; app.rs, app_tests.rs, lib.rs, theme.rs, theme/synthwave.rs, ui/editor_view.rs, ui/mod.rs, README.md, scripts/bench-startup.sh read (diff).

## Rule check T17
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to welcome, monitor, theme token, recent list, README, bench |
| BASE-2 | pass | additive only |
| BASE-3 | pass | red/green lines in 04_dev_T17.md; but the compact path lacks a test for n >= 6 (see finding) |
| BASE-6 | pass | English, why-comments (one misplaced doc comment, minor) |
| BASE-25 | pass | guard output in notes (fmt, clippy, 311 tests) |
| BASE-32 | pass | D187/D192/D193 honoured (no return-to-welcome keys, tabs row empty) |
| new-states | pass | table present; sampling has no thread, FIFO guard tested |
| AC-1 | fail | welcome.rs:337 keys missing on compact 50x24, 60x14 |
| AC-6 | pass | first frame and RSS in notes; sampling is 1 Hz and tiny |

## Round 1 re-check - APPROVED
Fresh release build, pty captures after Esc (qa-artifacts/T17/review/r1-*-clean.txt/.png).
- Major (compact keys) closed: 50x24 and 60x14 now show all 5 keys; 40x12 still shows 4; 120x40 and 80x24 unchanged (START keys, CPU/RAM/SWP, LOAD/UP, VR-IDE all present, no clipping).
- 120x40 history label reads `58 s`, matching the 58-column graph.
- `+N` core marker: 8-core machine, so no marker appears (correct, none cut); the 24-core case rests on the developer's test cut_cores_are_counted.
- `RECENT_MAX` move seen in the diff of the notes only; not a behavior change.
- No new findings.
