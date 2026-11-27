# FR-32 Terminal panel: black background, Homebrew-style text (D329)
Skills: ai-dev-team:implementation (ui-ux-design not loaded; contrast checked by WCAG tests).

## Changes
- crates/vr-ide/src/theme/ansi.rs: `panel_colors(depth)` (single source of panel fg/bg/selection), `NEUTRAL_ANSI` table, module doc updated; synthwave/dracula ANSI black lifted (0x5a4a82 / 0x44475a) so it shows on black.
- crates/vr-ide/src/theme.rs: dark/light get the panel colours and a neutral ANSI palette (`neutral_terminal_palette`); dracula uses `panel_colors`; field docs updated; tests reworked.
- crates/vr-ide/src/theme/synthwave.rs: palette `terminal`/`terminal_selection` fields removed, uses `panel_colors`.
- README.md line 428, crates/vr-config/defaults/settings.toml line 8: theme text.

## Colour values
| depth | bg | fg | selection |
|---|---|---|---|
| truecolor (synthwave, dracula) | Rgb 000000 | Rgb 28fe14 | Rgb 005f00 |
| 256 (all, and dark/light always) | Indexed(16) | nearest index passing 7:1 (fit) | Indexed(22) |

dark/light know no depth, so they use the 256 values (draw identically on truecolor terminals).

## Tests
- Replaced the three FR-26 terminal tests with `theme::tests::terminal_panel_is_black_with_homebrew_green_text` (all themes, both depths: bg RGB = 0,0,0; fg>=7:1 and green; truecolor fg exactly 28fe14; selection vs bg >=1.3, fg on selection >=4.5; ANSI black >=1.3, bright black >=3 on bg); `terminal_palette_follows_the_theme` now covers dark/light too.
- `cargo test -p vr-ide`: 710 passed; 0 failed; 7 ignored. terminal_tests panel-colour tests pass unchanged.
- Snapshots are text-only (no colour), none changed, nothing to regenerate.
- Guard: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` -> Finished, no warnings.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-H1 | pass | test above; `ansi.rs::panel_colors` |
| BASE-11 | pass | no checkout/restore/stash/reset used |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt run only on the 3 changed .rs files |
| COST-15/17 | pass | snapshots are text-only; none differ, so no regeneration was needed |
| others | n-a | not exercised by this task |

## Quick pass 1
Comment-only fixes: `ui/terminal_view.rs` concealed-text comment reworded; `terminal_tests.rs:1505` now "No palette: Reset defaults and raw ANSI indices."; `theme/ansi.rs` `ansi_palette()` doc says every built-in theme has a palette, `None` (tests only) leaves colours to the host. Checked terminal_view.rs ~419 (accurate) and ~462 (describes the no-palette Reset path, accurate): unchanged.
rustfmt on the 3 changed files; `cargo fmt --all --check` clean; clippy `-D warnings` Finished; `cargo test -p vr-ide theme`: 40 passed; `cargo test -p vr-ide terminal`: 96 passed; 0 failed.
