# Review T13 - Dracula theme (design QA + code)

Verdict: CHANGES_REQUESTED (1 major, 5 minor)

Tools available to me: Read, Bash, Write, Skill, SendMessage, SubagentHandback (no Edit). Chrome headless was used for screenshots.

## Findings
1. **major (design)** `crates/vr-ide/src/ui/dialog.rs:372,374,381,428` - dialog body text rows use `theme.text`, and line 428 does `theme.popup.patch(*style)`. The text style carries bg `#282a36`, so each text run is painted on the editor background inside the raised `#343746` popup. Result on the Confirm dialog (both sizes): "2 unsaved file(s)..." and "the recovery folder." sit in dark patches on a lighter popup, the padding after the text is not. Every dialog body (confirm, prompts) is affected. dark/light are unaffected (`text` has no bg), which is why no test saw it. Fix: use a text row style that only sets fg, for example `Style::default().fg(fg)` or plain `theme.popup`, in the three `Row::Text` pushes. Add a test that all cells inside the dialog rect have the popup bg.
2. **minor** `crates/vr-ide/src/app.rs:239` (AC-5) - unknown name keeps "using dark" and `Theme::from_name` falls back to dark. This is truthful, and no crash, but the default is now dracula: a typo (`"draculaa"`) silently flips the user from the own-background look to the terminal-colours look. Suggest falling back to the default (dracula) and saying "using dracula", or state in D139/D140 that dark is the deliberate fallback. No test covers the notice text.
3. **minor** `crates/vr-ide/src/theme.rs` 256-colour fallback, no contrast check. I recomputed the nearest-index mapping: gutter `#6272a4` -> idx 61 is 2.35:1 on bg (< 3), dim on list_selected (idx 60) is 2.63:1 (< 3), error red 4.43:1. Tab/status bar (235) vs bg (236) and popup (237) vs bg are nearly the same grey, so surfaces blur together. Not a blocker (fallback only); suggest hand-picked indexes for dim/comment (e.g. 103/146) or a 256 contrast test with a lower floor.
4. **minor** `theme.rs` contrast test gaps - `tab_modified`, `tab_warning`, `button` (dim on popup is covered via dim), `control` are not asserted. `control` red on `selection` is 2.91:1 (< 4.5) when a control glyph is selected. Luminance formula itself is correct (WCAG, 0.03928 vs 0.04045 threshold has no practical effect); body ratios I recomputed match the notes (text/bg 14.0, selection 8.6, find_match 7.7, dim/bar 6.7).
5. **minor** `ColorDepth::detect` reads the env var on every frame (`Theme::dracula()` is built each `render`). Cheap but needless; build the theme once per run or cache the depth. Also TERM=linux (8/16 colours) gets Indexed 16..255 and will look wrong; acceptable, mention in README if wanted.
6. **minor (design)** In dark and light themes `focus` = DarkGray/Gray bold, the same colour as `dim`, so the separator is effectively unchanged; the focus cue exists only in dracula. Fine for T13 scope; note for users of dark/light.

## BASE-3 mutation list (tests written after code)
- `colorterm_decides_the_depth`: always-TrueColor, or dropping the `24bit` arm, fails. Survives: `eq_ignore_ascii_case` -> `==` (no `"TrueColor"` case).
- `dracula_is_named_and_unknown_names_are_not`: removing the arm, or a prefix match, fails.
- `dracula_paints_the_published_palette_in_true_colour`: catches only 5 values (bg, fg, selection, gutter, info); mutating pink/purple/green/orange etc. survives except through contrast.
- `dracula_without_true_colour_uses_only_indexed_colours`: constant or 0..15 index fails; the exact asserts (236, 231, 16, 196) catch grey-vs-cube compare flipped. Survives: a wrong middle cube level (e.g. 135 -> 130).
- `notice_levels_and_roles_are_distinct`: making info/warning/error colours equal fails.
- `dracula_contrast_meets_the_floor`: changing dim to the comment colour, or gutter_active to a dark colour, fails. Survives: changes to tab_modified, tab_warning, control, button.
- `dracula_paints_its_background_on_every_cell`: removing the `set_style(area, theme.text)` fill fails.
- `focused_panel_separator_differs`: explorer separator always dim fails. Survives: `search_panel.rs` sep set to always `dim`, and any change to the `editor_view.rs` `gutter_active` condition (focus / dialog check) - neither is tested.
- `default_theme_is_dracula`: reverting settings.toml fails.

## Design QA
Method: Chrome headless rendered `mockups/T13/dracula.html` (all 12 sections = 6 screens x 120x40, 80x24); dark.html compared by its cell structure (same layout, only colours differ), not viewed screen by screen. The horizontal banding between rows is a line-height artifact of the HTML preview, not the theme. Screenshots: `$TEAM_WS/qa-artifacts/T13/dracula-0.png` .. `dracula-6.png` (full page `dracula-full.png`, `dark-full.png`).
Overall: reads as Dracula (bg #282a36, fg #f8f8f2, purple tab/border, green focus, cyan/orange/red notices); whole frame painted (test also checks 40x12); minimal, nvim-like density, nothing cramped or lost vs dark.

| Screen | 120x40 | 80x24 | Notes |
|---|---|---|---|
| Editor focused, warning notice | pass | pass | dracula-0.png; orange notice distinct; focus cue is only the pink bold current line number (weaker than panel focus, acceptable) |
| Explorer focused | pass | pass | dracula-1/2.png; green separator + purple selected row obvious; unfocused: grey row + blue separator |
| Command palette | pass | pass | dracula-2/3.png; selected row purple with yellow match chars readable; popup edge relies on the purple border (raised vs bg is subtle) |
| Find bar | pass | pass | dracula-3/4.png; olive match vs grey current match distinguishable, count 2/3 readable |
| Search panel | pass | pass | dracula-4/5.png; selected result row purple, matches yellow, green separator |
| Confirm dialog | FAIL | FAIL | dracula-5/6.png; finding 1 (dark text patches inside popup) |

## Files read
- README.md read; crates/vr-config/defaults/settings.toml read; crates/vr-ide/src/theme.rs read; ui/mod.rs read; ui/editor_view.rs read; ui/explorer.rs read; ui/search_panel.rs read; find_tests.rs read; ui/tests/preview.rs read (whole file); app.rs:225-245 read; ui/dialog.rs and palette.rs read at the style lines (callee of `theme.text` / `popup`).
- Previews: mockups/T13/dracula.html (cell colours grepped), dark.html (rendered only).

## Rule check T13
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff has no key or focus logic; `editor_view.rs` and the panels only read `app.focus`; keymap not touched |
| BASE-2 | pass | scope is theme/ui styles/defaults/README; no unrelated edits |
| BASE-3 | partial | tests after code (admitted); mutation gaps listed above (search panel separator, gutter_active condition, 256 contrast) |
| BASE-6 | pass | no dependency change |
| BASE-25 | pass | developer notes show fmt, clippy -D warnings and tests clean; I did not rerun |
| AC-5 | pass with minor | unknown name -> notice, no crash; wording/fallback finding 2 |
| COLORTERM fallback | pass | `from_colorterm` (case-insensitive truecolor/24bit, else 256) + test; findings 3, 5 |
| render_with | pass | `render` delegates; dark/light unchanged because `text` has no bg |
| Preview helper test-only | pass | `mod preview` is inside `#[cfg(test)] mod tests`; writes only when `VR_PREVIEW_DIR` is set, no product path |

## Round 1 re-check
Verdict: APPROVED (no new findings; one nit).

Screenshots: `$TEAM_WS/qa-artifacts/T13/r1-0.png` .. `r1-6.png` (regenerated dracula.html, headless Chrome). Tests not rerun by me (developer: 192 passed, fmt and clippy clean).

### Design QA
| Screen | 120x40 | 80x24 | Notes |
|---|---|---|---|
| Editor focused, warning notice | pass | pass | r1-0.png; unchanged |
| Explorer focused | pass | pass | r1-1/2.png; unchanged |
| Command palette | pass | pass | r1-2/3.png; popup bg uniform, selected row and yellow matches readable |
| Find bar | pass | pass | r1-3/4.png; unchanged |
| Search panel | pass | pass | r1-4/5.png; unchanged |
| Confirm dialog | pass | pass | r1-5/6.png; finding 1 fixed, body text on the popup bg, no dark patches |
Colours: control orange and error/tab_warning #ff7878 (salmon, lighter than #ff5555) still read as Dracula; error stays distinct from the orange warning (hue plus bold). The preview shows no error row, so that judgement is from the values.

### Code
- Finding 1: `dialog.rs` body rows use `Style::default()`; tests assert no editor-bg cell inside confirm, prompt and palette popups at both depths and sizes (`preview.rs`). Honest: reverting to `theme.text` would reintroduce #282a36 cells and fail.
- Finding 2 / D141: `DEFAULT_THEME` const, `from_name` falls back to it, notice interpolates it; `unknown_names_fall_back_to_the_default_theme` would fail with a dark fallback, `preview.rs:388` checks "using dracula".
- Finding 3: hand-picked surface indices (236/234/238/239/241/60/58) keep the greys distinct; `fit` picks the nearest index meeting the floor on the surfaces a colour is drawn on; contrast test runs on both depths.
- Finding 4: contrast test now lists the previously missing pairs (tab_modified, tab_warning, control, button, error on popup/status).
- Finding 5: `OnceLock` caches COLORTERM once per run; `from_colorterm` stays pure and tested, including the `TrueColor` case.
- Finding 6: dark/light focus is Cyan/Blue bold, with a test.
- Developer reports mutation checks for the search separator and gutter_active condition; both tests fail on the mutation.
- Nit: `fit` falls back to the best-contrast index when none meets the floor, silently; fine given the tests assert the floor for the current palette.

### Rule check (re-check)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | round diff is styles, theme, dialog row style, notice text only |
| BASE-2 | pass | no unrelated edits |
| BASE-3 | pass | new tests red first per notes; mutation checked for the two previously untested paths |
| BASE-6 | pass | no dependency change |
| BASE-25 | pass | developer: tests, fmt, clippy -D warnings clean (not rerun) |
| AC-5 / D141 | pass | notice names the real fallback, tested |
| Preview helper test-only | pass | unchanged |
