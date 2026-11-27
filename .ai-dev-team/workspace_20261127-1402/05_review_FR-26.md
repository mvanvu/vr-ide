# Review FR-26 — CHANGES_REQUESTED

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback. Test run (once): 31 passed, 0 failed.

## Findings
[major] crates/vr-ide/src/theme/synthwave.rs:184 — synthwave panel is not clearly distinct from the editor (user's complaint not resolved)
Scenario: editor #1a1028 vs panel #132047 has a luminance ratio of 1.15 (measured). Both are very dark, and a navy next to a purple of equal darkness blends at a glance; the user's complaint ("same colour as the IDE") is likely to repeat. The test's RGB distance >= 24 is a floor, not a visibility measure.
Fix: lift the panel instead of staying equal-dark. The 7:1 rule allows a panel up to about L 0.08 for fg #f6eefc. Measured options: #1c3a78 (256: 24 or 25) gives 1.68 vs editor, 9.6:1 text, 1.39 vs ANSI black #2f1c49; #20407a gives 1.81 vs editor, 8.9:1 text, 1.50 vs ANSI black. Both are a clearly bluer, lighter band and ANSI black stays visible on them. Re-run make_preview.py for the user to confirm.

[minor] crates/vr-ide/src/theme.rs:453 — dracula panel #16171d is 1.26 vs the editor
Scenario: a neutral darker step, noticeable but subtle, and it is darker than the editor's own bar #21222c only by a small step, so it may not read as a separate area on every monitor.
Fix: deepen to #0b0c10 (1.37 vs #282a36; text >= 7:1 still holds), or use a lighter step (#3a3d4f, 1.33, text 10:1) if a lifted panel suits the look better. The user decides; show both in the preview.

[minor] crates/vr-ide/src/theme.rs:185 — dark: panel idx 234 (#1c1c1c) is not told apart on a user terminal whose own background is about #1c1c1c or lighter-dark grey
Scenario: dark leaves the editor on the terminal's colours, so the contrast is unknown; the test cannot cover it (editor = Reset). On a #1c1c1c terminal the panel and the editor are identical. Symmetric for light on a #dadada terminal.
Fix: acceptable as a stated limit; record it in the notes and the README theme line, or pick idx 233 / 254 for a larger step on the common black/white terminals.

[minor] crates/vr-ide/src/ui/terminal_view.rs:422 — public `cell_style` still resolves defaults to the old editor colours and the FR-25 SSH view is not on this branch
Scenario: AC-T1 requires the SSH full-screen view to share the terminal background. FR-25 (worktree in progress) that calls `cell_style` or builds its own fill gets the editor colours. grep shows no SSH code here, so it is not verifiable in this diff.
Fix: leader to require FR-25 to call `theme_cell_style` and the same whole-area fill when merging (both touch `render_grid`; expect a conflict), or make `cell_style` private so the compiler enforces it. Re-check at the FR-25 review.

Verified: hidden "fg = bg" guard runs after the inverse swap on resolved colours (fixed bg for all themes, so no Reset hidden-text gap remains); inverse swaps the fixed pair (no REVERSED) for dark/light, correct; the whole-area fill uses `theme_cell_style(&blank)` so margin and unused rows share the panel bg (test checks margin, rows below, far corner); Indexed and Rgb program colours bypass the defaults unchanged; 16 ANSI colours still go through the palette; 256-colour fallback: synthwave terminal idx 17 (#00005f) vs true #132047 is a visible shift toward a purer blue but text is fitted through `fit(depth,..)` at 7:1; dark/light use indexed values only. Note synthwave ANSI black #2f1c49 vs the panel is 1.04 (ratio), so ANSI-black-on-default blocks vanish; moving to the lighter option above (1.39 to 1.50) also fixes this.

## Design QA notes
Judged from preview.ans (RGB values and measured ratios, not a rendered screenshot). dark 1.23 and light 1.40 are fine against the approximated black/white editor. Text readability passes everywhere (10.8 to 16.8:1). Synthwave fails the "two areas at a glance" test (see major). Dracula borderline. Density and the minimal nvim-like look are untouched (no borders added; only a background step).

## Files read
| file | status |
|---|---|
| crates/vr-ide/src/theme.rs | read |
| crates/vr-ide/src/theme/synthwave.rs | read |
| crates/vr-ide/src/ui/terminal_view.rs | read (diff plus the style function) |
| crates/vr-ide/src/terminal_tests.rs | read (diff) |
| .ai-dev-team/workspace/04_dev_FR-26.md, qa-artifacts/FR-26/preview.ans | read |

## Rule check FR-26
| rule | result | note |
|---|---|---|
| BASE-1 | pass | does what the spec says except the synthwave/dracula distinctness finding |
| BASE-3 | pass | red-first noted; new tests exist |
| BASE-6 | pass | no scope creep seen |
| BASE-16 | pass | reuses ansi palette and fit; tokens on Theme |
| BASE-25 | pass | evidence in notes, tests re-run green |
| BASE-32 | fail | AC-T1 "visibly different" is not met for synthwave, the user's own theme, shown by a 1.15 ratio; the developer left it as a hue-only trade-off, which the user decides, not the developer |
| BASE-35 | pass | notes carry a Rule compliance section |

CHANGES_REQUESTED

## Re-check round 1
Test run once: 33 passed, 0 failed.
- major synthwave distinctness: fixed. #20407a vs #1a1028 is 1.81 with a hue shift as well (dark purple against a clearly lighter medium blue), 8.9:1 text, 1.50 vs ANSI black; new test pins >= 1.6 at both depths. Design QA: from the regenerated preview.ans values, yes, it reads as two areas at a glance.
- dracula: fixed. #31437a is 1.50 (256-colour 1.36 vs floor 1.3), 8.9:1 text; the 256-colour limit is stated in the test.
- dark/light: fixed with idx 237 / 250, a test against four common terminal backgrounds each (>= 1.2) and a README line stating the residual limit.
- `cell_style` is `#[cfg(test)]`; only `theme_cell_style` draws, so FR-25 must use it (compile-enforced). Still to be re-checked at FR-25 merge.
- Extra `terminal_selection` token: in scope. The editor selection colour became too close to the lifted panel, a direct consequence of the fix, and it is small (3 theme values, one `.bg()` call in `render_grid`) with a test (>= 1.3 against the panel, >= 4.5:1 text on it). No new finding.
Verdict: no open blocker or major; BASE-32 now pass.

APPROVE
