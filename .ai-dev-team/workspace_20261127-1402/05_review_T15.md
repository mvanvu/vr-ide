# Design QA T15 - three theme palettes (A Midnight, B Neon, C Synthwave)

Verdict: PASS (all three showable); two minor preview findings. Ranking: C, B, A.
Method: previews rendered with headless google-chrome (file: URLs); crops in qa-artifacts/T15/{compare,dracula,a-midnight,b-neon,c-synthwave}-p0..p5.png (+ -full.png). True colour only; the 256 fallback is judged by the tests, not visually. Tests read: theme.rs `pairs()` + `candidate_contrast_meets_the_floor_in_both_depths` + `candidate_surfaces_and_roles_stay_distinguishable_in_both_depths` loop NAMES x BOTH depths (body 4.5, dim/gutter 3, surfaces >= 1.1, selection != list_selected, info/warning/error and focus/dim distinct). They cover every candidate at both depths. Gutter is only checked on the editor bg (floor 3.0, as in T13). I spot-checked 15 token ratios by hand and they agree with the notes.

## Per-candidate per-screen (120x40 and 80x24 both painted, no clipping, nothing cramped)
| Screen | A Midnight | B Neon | C Synthwave |
|---|---|---|---|
| Editor + explorer + warning notice | ok; pink active tab, cyan current line number, orange notice distinct from the pink | ok; cyan tab, magenta line number; bar is #010203, near pure black | ok; pink tab, orange line number, notice orange |
| Explorer focused | ok; green separator, row 5a36a0 clearly selected | ok; green separator, teal row | ok; cyan separator, purple row |
| Command palette | ok; purple frame, row 5a36a0 readable, popup uniform | ok; magenta frame, teal row | ok; cyan frame, purple row (input and selected row are two similar purples) |
| Find bar | ok; match amber, input band blue-grey | ok; match dark gold, input band | ok; input band is a heavy purple stripe, match amber still reads |
| Search panel | ok | ok | ok |
| Confirm dialog | ok; popup bg uniform, [Save all] pink block | ok; popup only 1.2x over bg but the magenta frame carries it | ok; popup clearly lifted, cyan frame |
Focus is visible in all three: the separator colour changes and the row highlight differs between editor-focused and explorer-focused, so it is not colour alone. Notices: warning, error and info are different hues in each one.

## Direction and "cooler than current dracula"
- A: reads as a darker, punchier Dracula (near-black bg, pink active, purple frame). It is the smallest jump; next to the current one it is "deeper", not different.
- B: reads clearly as neon on blue-black (cyan and magenta). Most unlike Dracula in hue. Surfaces are flat (popup 1.2x, bar black), so the screens feel sparser.
- C: reads clearly as synthwave (purple bg, hot pink, cyan frame, orange accents). Most distinctive and has the most visible depth between surfaces.

## Findings
1. minor, preview (not the palette): compare.html is not side by side at normal widths. The grid has `minmax(min(100%,980px),1fr)` and each 120x40 block is about 1010px, so below about 2050px of window width the four candidates stack. The dracula (current) block and the later ones are then roughly 1000px of scroll apart, and the token table sits under each block. Fix: scale the blocks (CSS `zoom` or a smaller font) so 2 columns fit in 1500px, or cut the compare to 2x2 at 80x24. A user can still judge from it, but the A/B comparison needs scrolling. (compare-p0.png)
2. minor, preview artifact: faint horizontal hairlines in empty editor rows of every preview (including dracula). These come from the line-height of the `<pre><span>` rows, not from the theme. Tell the user that real terminal cells have no gaps. Fix: `line-height:1` with `display:block` rows.
3. minor, observation: B has the lowest surface lift (popup 1.2, bar 1.1 and pure black); C's find/palette input stripe is heavy. Both pass the tests; for C the selected row vs input (4d3078 / 6a2f96) is only about 1.2 apart, so selected and input rows are told apart mostly by position.
4. minor: dim text on selection surfaces reaches 3.0 only, in line with the T13 floor. Gutter digits on a selected line were not covered by any pair; none appear on a selected line in the previews.

## Ranking
1. C Synthwave: the clearest "cooler/edgier" change, best surface depth, readable at both sizes, cyan frame/focus coherent. Risk: the purple is saturated; the default for a minimal nvim-like taste may tire over long sessions.
2. B Neon: a clear departure with strong frame/tab accents, but flat surfaces and a black bar feel stark.
3. A Midnight: safe and closest to the stated design_taste (Dracula family), but least of an upgrade; recommend it only if the user wants a mild change.

## Implement design QA
Verdict: CHANGES (1 major). Screenshots: qa-artifacts/T15/impl-synthwave-p0..p5.png, impl-compare-wide.png, impl-compare-p*.png. Diff read: theme.rs, theme/synthwave.rs, settings.toml, README.md; preview.rs and the deleted candidates.rs were not read line by line (only the rendered output). I did not run the tests.

### Per screen (synthwave.html, 120x40 and 80x24, READ/EDIT badges)
| Screen | Result |
|---|---|
| Editor + explorer + notice | ok; READ badge cyan on the bar, orange notice distinct |
| Edit mode | ok; EDIT badge orange, clearly unlike READ |
| Explorer focused | ok; cyan separator, purple row |
| Command palette | ok; frame cyan, selected row 6a2f96 clearly separate from the recessed input (2.3:1, minor 3 fixed) |
| Find bar | FAIL (major 1): the Find and Replace fields are not visible as fields |
| Search panel | FAIL (major 1): the "> total" input is not visible as a field; the selected result row is fine |
| Confirm dialog | ok; popup bg uniform, Save all pink; no regression against round 0 |

### Earlier minors
1. compare.html: now 2-up (synthwave | dracula) at 2200px, scaled so both blocks and token tables fit; acceptable. Below about 1500px it will stack again, which is fine for two blocks.
2. Hairlines: gone, but `line-height:1` now clips descenders in the preview (g, y, p, comma tails cut, for example ".gitignore", "settings:", "ctrl+pageup"). Preview-only, minor. Use about 1.15 with `display:block` rows.
3. Palette input vs selected row: fixed (120a1e vs 6a2f96 is 2.26:1).

### Findings
1. major, `crates/vr-ide/src/theme/synthwave.rs` input `(0x120a1e, 233)`: the input bg is 1.06:1 against the editor bg 1a1028 (index 233 vs 234 in 256 mode), and the find bar, replace field and search input sit directly on the editor bg. In round 0 the field was a visible purple band (4d3078); now the field has no visible extent, and only the popup (1.27:1 to its bg) works. The test only checks input vs popup (>=1.1) and input vs picked (>=1.6), not input vs editor bg. Fix: pick an input bg that is >=1.3:1 from both bg 1a1028 and popup 2f1c49 and still >=1.6 from picked 6a2f96, for example a lift such as 2a1a40-ish does not work against popup 2f1c49, so use separate surfaces: either keep one input colour in the 1.3-1.5:1 window over the editor bg (about 241a3c) and accept a low contrast on the popup, or give the palette input the recessed value and the find/search inputs a raised one. Add a test for input vs editor bg at both depths.
2. minor, preview: descender clipping above.
3. minor, observation: the READ/EDIT badge touches the file path with no gap ("READsrc/..." visually adjacent; a blank cell would read better). Not in this diff; optional.

### Badges
READ cyan 2de2e6 on bg 1a1028 is 11.4:1; EDIT orange ff9d3c is 8.8:1; both bold and in `pairs()` at 4.5. They differ in hue and in the word, so they are not colour alone.

### Code
- `named("synthwave")` present; `DEFAULT_THEME = "synthwave"`; `from_name` falls back to the default per D141 (test updated); settings.toml and README match.
- dracula, dark and light code is unchanged (diff touches no existing palette); badge tests were extended to synthwave at both depths and a new dark/light badge-collision test checks bg against status bg, status fg and tab bar. Honest, except the missing input-vs-editor test above.
- 256 fallback: `synthwave_without_true_colour_uses_only_indexed_colours` covers every style.

### Rule check
| Rule | Result | Evidence |
|---|---|---|
| Contrast floors, both depths | pass | `synthwave_contrast_meets_the_floor_in_both_depths` over `pairs()` incl. badges |
| Distinct surfaces and roles | fail | input vs editor bg not tested; actual 1.06:1 |
| Focus not colour alone | pass | impl-synthwave-p1 |
| Dialogs uniform popup bg | pass | impl-synthwave-p4 |
| Other themes unchanged | pass | diff |
| Default and fallback (D141) | pass | theme.rs |

## Implement round 1 re-check
Verdict: APPROVE. Screenshots: qa-artifacts/T15/impl-r1-p0..p5.png (-full). Read: the round-1 palette values and the new `>= 1.3` input-vs-bg test (theme.rs:651, 657) in the diff; tests not run.
- Major 1 (inputs invisible) fixed: input 3a2760 is 1.43:1 on the editor bg; the find and replace fields and the search "> total" row read as bands again in both sizes (impl-r1-p3.png). Text on input 11.3:1, dim 5.9:1.
- Selected row 7433a6: 1.69:1 to the input, text 6.7:1, matched yellow 6.1:1, dim 3.5:1 (floor 3). Palette selected row is clearly separate from the input row (impl-r1-p2.png). The palette input on its popup is only 1.18:1; it stays readable by the `>` prompt and position, which is acceptable (was 1.27).
- Minor 2 (descender clipping) fixed with line-height 1.15; remaining faint lines in the crops are Chrome tile seams, not in the output.
- Minor 3 (badge gap): the badge is padded inside its own colour block and the cyan/orange edge separates it from the path; no extra blank cell needed.
- No regressions: explorer focus, confirm dialog and notices unchanged; READ and EDIT still distinct.
