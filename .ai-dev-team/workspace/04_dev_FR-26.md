# FR-26 notes — terminal panel background distinct from the editor's (D319)
Progress: red (compile error, no `terminal_bg`/`terminal_fg`) -> tokens + view -> related tests green -> guard green.

## Files
- crates/vr-ide/src/theme.rs: `Theme.terminal_fg` / `terminal_bg` (dark, light, dracula); test `terminal_background_differs_from_the_editor_and_text_reads_on_it`; `terminal_palette_follows_the_theme` now compares to the tokens.
- crates/vr-ide/src/theme/synthwave.rs: `Palette.terminal` (0x132047 / idx 17), tokens, ANSI palette fg/bg = tokens.
- crates/vr-ide/src/ui/terminal_view.rs: `theme_cell_style` (new, used by `render_grid` for the area fill and every cell) over a private `style_with`; `cell_style` keeps its signature (FR-25 can call either; the SSH view should call `theme_cell_style` to share the bg). The hidden "fg = bg" guard and the inverse swap work on the fixed bg; for dark/light inverse now swaps the fixed pair instead of REVERSED.
- crates/vr-ide/src/terminal_tests.rs: `panel_default_background_is_the_terminal_token_in_every_theme` (TestBackend: text cell, right margin, rows below, far corner).
- Snapshots: none changed (text-only), nothing regenerated.

## Colours
| theme | editor bg | terminal bg | bg vs bg | terminal fg | fg on terminal bg |
|---|---|---|---|---|---|
| synthwave | #1a1028 (256: 234) | #132047 navy (256: 17) | 1.15 (hue shift purple -> navy) | #f6eefc | 14.0 |
| dracula | #282a36 (236) | #16171d (233) | 1.26 | #f8f8f2 | 16.8 |
| dark | terminal's own | idx 234 #1c1c1c | 1.23 vs black | idx 252 #d0d0d0 | 11.0 |
| light | terminal's own | idx 253 #dadada | 1.40 vs white | idx 235 #262626 | 10.8 |
Test also requires RGB distance >= 24 from the editor bg for fixed-colour themes, fg/bg >= 7:1 (all depths).
dark/light: both colours fixed so text reads on any user terminal. dark on a dark terminal: a slightly darker-than-typical-grey panel; on a light terminal: a dark panel. light on a white terminal: a light-grey panel; on a dark terminal: a light panel with dark text.
Synthwave contrast is hue-driven (purple vs navy); the luminance ratio cannot exceed ~1.2 for a darker panel without clashing with ANSI black (#2f1c49).
Program colours unchanged (indexed/RGB cells pass through; ANSI table untouched).

## Preview (USER-2)
$TEAM_WS/qa-artifacts/FR-26/preview.ans (editor + panel per theme, from make_preview.py in the same folder; approximates dark/light editor as black/white).

## Commands
- `cargo test -p vr-ide --lib -- terminal theme:: ui::`: 177 passed, 0 failed, 2 ignored (pre-existing perf ignores).
- `cargo test -p vr-config --test config` (related-tests listing): 23 passed.
- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Nothing left running.

## Reuse
| need | reused |
|---|---|
| ANSI palette / fit machinery | theme/ansi.rs `palette`, `fit` extended with a terminal surface |

## Rule compliance
BASE-1 pass; BASE-3 pass (compile-red first); BASE-4 pass; BASE-16 pass (theme tokens, reuse table); BASE-25 pass; BASE-32 pass; BASE-35 pass; COST-8 pass (Edit/perl on own files); COST-15 pass (no snapshot changes).
n-a: all other BASE-1..48 and COST-1..20 IDs not named above — no server, no deps, no commit, no new agents, no full suite, nothing persistent started (BASE-14/15/17/18/19 and COST-9/13 observed: no background task, no polling, no tool writes).

## Fix round 1
Red first: new floor tests failed (synthwave 1.15, dracula 1.26, dark/light vs common backgrounds, selection) before the colours changed.
1. synthwave panel #20407a (256: idx 20): 1.81 vs editor (256: >=1.6), text 8.9:1, vs ANSI black 1.50 true colour; 256-colour cube has no blue clearing both ANSI red (>=3:1) and black by more, so black floor is 1.2 there.
2. dracula panel #31437a (256: idx 238): 1.50 vs editor (256: 1.36, floor 1.3), text 8.9:1, ANSI black 1.7. Window is tight: lighter breaks the existing "ANSI red >= 3:1 on the panel" test.
3. dark idx 237 (#3a3a3a, text idx 252 7.4:1): >= 1.23 vs #000000 / #1c1c1c / #300a24 / #282c34. light idx 250 (#bcbcbc, text idx 235 8.0:1): >= 1.36 vs #ffffff / #dadada / #e4e4e4 / #fdf6e3. Limit (README line added): blends on a terminal whose own background is exactly that grey.
4. `cell_style` is now `#[cfg(test)] pub(crate)`; the only drawing path is `theme_cell_style`, so FR-25's SSH view cannot compile against the editor defaults.
Extra (needed by the lifted panels): new token `terminal_selection` used for selected text in the panel, because the editor selection colour was ~1.0-1.2 from the new panel bg (test: >= 1.3 vs panel, text on it >= 4.5). Dracula #5b6aa6 (61), synthwave #8a4fc7 (91), dark idx 24, light idx 38.
Tests: `cargo test -p vr-ide --lib -- terminal theme:: ui::` 179 passed, 0 failed, 2 ignored. fmt + clippy clean. preview.ans regenerated. No snapshot changes. Nothing left running.
