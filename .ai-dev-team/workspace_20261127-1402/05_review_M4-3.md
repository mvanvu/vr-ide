# Review M4-3: editor wiring of syntax highlighting (code review, opus)

Verdict: **CHANGES_REQUESTED** (2 major, 2 minor)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings

[major] crates/vr-ide/src/syntax.rs:195-199, 276-298: a disabled highlighter still diffs the whole buffer on every change (AC-13c / AC-18)
Scenario: open a 100 MB `.json` (or `.rs`, `.sql`) file. The highlighter is created and turns itself `Disabled::TooLarge`, but `d.hl.is_some()` is still true. Each keystroke then runs `follow` → `Edit::diff_in`, which walks the rope chunks from both ends up to the change (about 106k chunks), and then `hl.edit` drops the result (`worker` is `None`). The prefix/suffix walk copied verbatim from edit.rs, run on a 100 MB rope with one char changed at mid-file, took median 7.6 ms and worst 13.7 ms per keystroke (qa-artifacts/M4-3/review-diffcost, release). The 5 ms keystroke-to-frame budget is spent before the frame is drawn. No test covers this case: `long_line_keystroke_to_frame_fast` and `render_large_file_fast` use untitled (plain) documents. The same file pays for `restart` → `has_lone_cr` over all 100 MB on the UI thread on every reload, because `restart` does not have the `<= max_bytes` guard that `resolve` has.
Fix: in `sync` / `resolve`, when `hl.status()` is `Disabled` and no reset is pending, only set `seen = rope.clone(); seen_version = version` (no diff, no edit). Gate `restart`'s `has_lone_cr` like `resolve` does. Add a perf case: a highlighted-extension file over `max_highlight_bytes` (100 MB `.json`), keystroke+frame median < 5 ms, in `highlighted_keystroke_to_frame_fast`.

[major] crates/vr-ide/src/theme.rs:194-209 (dark_syntax), 84-85: in the `dark` theme, syntax colours under the selection and on find matches drop to 1.7–2.6:1
Scenario: `theme = "dark"`, select a comment or search for a keyword. The selection and find-match backgrounds are fixed 256-colour indexes (239 = #4e4e4e, 58 = #5f5f00), not palette-defined colours. With xterm's default palette (the one the developer's own test uses), the contrast is:
- comment `DarkGray`: 2.08 on the selection, 1.68 on a find match;
- number/constant/tag `LightRed`: 2.08 on the selection, 1.68 on a find match;
- keyword `LightMagenta`: 2.65 on the selection, 2.14 on a find match.

The other themes have a 3:1 floor here (qa-artifacts/M4-3/review_contrast.py). Before M4-3 these cells used the terminal's default fg, which reads. D173 leaves out the 3:1 check for dark/light because "terminal colours have no RGB", but the surfaces are fixed indexes and the foregrounds are checked against xterm already. `light` passes (min 3.12). This is a visible defect, so the user decides. Fix options and cost:
- (a) Darker dark-theme surfaces: selection 237/238, find match a darker index. A few lines, plus a re-check of the T13 focus tests.
- (b) Change comment/number to Gray/Yellow tones. These collide with punctuation/type.
- (c) Keep and record the waiver in D173 as user-accepted.

Whichever is chosen, extend `terminal_colour_themes_use_ansi_syntax_colours` to check 3:1 on `selection.bg` / `find_match.bg`.

[minor] crates/vr-ide/src/syntax_tests.rs:100-110: `the_first_frame_does_not_wait_for_the_parse` does not time `open_path`, which now creates the highlighter (app.rs:441)
Scenario: if `Highlighter::new` ever parsed on the calling thread, `open_path` would block for seconds and the test would still pass. This was one of the three tests that passed vacuously on the stub (BASE-3), and it still does not guard AC-13d's "first frame".
Fix: start the timer before `app.open_path` (build the file first, then open it inside the timed block).

[minor] crates/vr-ide/src/syntax_tests.rs:260-271: `read_mode_and_edit_mode_draw_the_same_colours` passes when neither mode has colours
Scenario: if a regression drops spans in both modes, the test stays green. `rust_file_is_drawn_in_the_theme_colours` covers read mode only by accident (it never enters edit mode).
Fix: assert that `read` has the keyword colour at (0,0) before the equality loop.

## Verification notes (no finding)
- **Sync.** `sync` runs once per frame on the active document. When nothing changed, it does one `u64` compare plus a `PathBuf` compare. When one char changed at 10 MB, the diff walk takes about 0.23 ms (probe) and the edit points are O(log n) with the flag.
- **Versions.** `Buffer::version` increments on every commit, undo and redo, and nothing else mutates the text (text.rs:148/171/184). Docs are only pushed or removed, so `version != seen_version` never misses a change. Two or more changes between frames become one exact covering edit.
- **expect_changes validation.** It needs `at == seen_version && version == at + 1` and checks that the rebuilt text equals the buffer. Any mismatch falls back to the diff, so it cannot desync.
- **Lone CR.** ropey 1.6.1 guarantees that CRLF is never split across chunks (iter.rs:1268, rope.rs `assert_crlf_seams`), and the workspace enables `cr_lines`. Without that feature, `has_lone_cr` would only give a false positive (the slow path is still correct). `lone_cr_in` covers inserted bytes, the byte before them and the byte after them. The flag only goes false→true and is rescanned on reset. `inserted_ranges` shift order is correct for last-change-first edits.
- **Wake.** It is swapped before the draw, and the worker sets it after publishing, so no wake is lost. Redraws happen only on publish, at most 50 ms late (D172), with no busy loop.
- **editor_view.** Spans are fetched for visible rows only. The byte counter follows `c.symbol` (a slice of the rope) through horizontal scroll, tabs and wide chars. Selection and find match patch only the background. Control chars are unchanged.
- **README.** Matches the code (10 MiB default, 10 s parse timeout, files/globs before the longest extension, field-by-field override).
- **BASE-1.** edit.rs/lib.rs are additive (old fns delegate with `Unknown`).

## Files read
| File | Status |
|---|---|
| Cargo.toml, crates/vr-ide/Cargo.toml | read |
| Cargo.lock | skipped — lockfile (one workspace edge, checked) |
| README.md | read |
| crates/vr-ide/src/app.rs | read (diff) |
| crates/vr-ide/src/commands/file.rs, find.rs, search.rs | read (diff) |
| crates/vr-ide/src/event_loop.rs | read (diff + run loop) |
| crates/vr-ide/src/lib.rs, ui/mod.rs | read |
| crates/vr-ide/src/syntax.rs (new) | read |
| crates/vr-ide/src/syntax_tests.rs (new) | read |
| crates/vr-ide/src/theme.rs, theme/synthwave.rs | read (diff) |
| crates/vr-ide/src/ui/editor_view.rs | read (diff + render loop) |
| crates/vr-ide/src/ui/tests/preview.rs | read (diff) |
| crates/vr-syntax/src/edit.rs, lib.rs | read (diff + common_prefix/suffix) |
| callees: vr-core text.rs (version), document.rs reload, vr-syntax highlighter.rs new/edit/reset/spans, ropey iter/rope CRLF docs | read |

## Rule check M4-3
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | files in scope; vr-syntax edit.rs/lib.rs additive only; highlighter.rs untouched |
| BASE-2 | pass | `Theme::syntax(Capture) -> Style` as planned; additions listed |
| BASE-3 | pass | red logs per group; 3 stub-vacuous tests judged: unknown-ext is a valid negative guard, the read-mode and first-frame tests are weak (2 minors above) |
| BASE-6 | pass | English, comments say why |
| BASE-8 | pass | only the workspace crate vr-syntax added |
| BASE-25 | pass | fmt + clippy output in notes |
| BASE-32 | pass | D171–D175 recorded; D173 waiver challenged in finding 2 (user decision) |
| New-states rule | pass | table present; disabled-highlighter edit path missing its cost (finding 1) |
| AC-13 | pass | rust_file_is_drawn_in_the_theme_colours, unknown_extension… |
| AC-13b | pass | typing/undo/redo/replace-all tests; perf 0.5–2.1 ms medians |
| AC-13c | fail | opens plain with notice, but per-keystroke cost on a 100 MB file with a known extension measured over budget (finding 1) |
| AC-13d | pass | behaviour holds (Highlighter::new never parses on caller); test interval weak (minor) |
| AC-13e | pass | a_language_without_a_compiled_grammar_is_a_startup_notice |
| AC-18 | n-o | existing perf tests green but use plain documents; the highlighted-extension 100 MB case is untested (finding 1) |

## Design QA round 1 re-check
Verdict: APPROVE (earlier findings 1-4 fixed; 2 new nits, optional).
Screenshots: qa-artifacts/M4-3/design-r1-synthwave.png, design-r1-dracula.png (120x40 Rust + Vue scenes from the regenerated previews; trim2.py / trim-r1-*.html beside them).

Re-check
- Comments recede (synthwave): `9884c9` italic, luminance 0.27 vs punctuation 0.43; clearly dimmer than code and still readable (5.6:1). Pass.
- Strings vs warning orange (synthwave): `ffb454` is a lighter amber than the warning/EDIT orange in true colour. Pass (256-colour shares index 215, accepted and tested as true-colour only).
- Property colour: Rust fields `cents`/`name`, Vue `item.title`/`picked.value`, CSS `padding` now carry the property colour (synthwave `82aaff`, dracula `8be9fd` upright); the match over `name` keeps it (4.3:1 synthwave, 5.9 dracula, above the 3.0 floor). Pass.
- Not a rainbow: the added colour touches only member names; code still reads as a few families with plain identifiers and punctuation. Pass.
- Dracula Constant: `LIMIT` is now purple upright, no longer the same as Type. Pass.
- Label now "line 5"; READ/EDIT badges, tab titles, find bar unaffected.
- Dark theme (by value): selection 236 / find 52 give min syntax fg 3.30 / 3.53 (xterm), above the 3.0 floor; light unchanged (3.12 / 4.51). Pass.

New nits (minor, optional)
1. Synthwave String `ffb454` (amber, ~35 degrees) now sits close to Type/Constant `ffe66d` (yellow); distinct in the render (`"total..."` vs `LIMIT`) but the closest pair in the palette. No change needed.
2. Dracula Property `8be9fd` upright and Type `8be9fd` italic share one hue; only italic separates them, which matches the published Dracula treatment of cyan italic types. No change needed.

## Round 1 re-check

Verdict: **CHANGES_REQUESTED** (major A closed; major B closed as stated but the fix opens a new major)

### Earlier findings
- **Major A (disabled highlighter diffed): closed.**
  - `DocSyntax::step` (syntax.rs) skips the diff/edit once `hl.status()` is `Disabled` and only records `seen` / `seen_version`. This is safe because a disabled highlighter takes no edits until `reset`, and `restart` rescans.
  - `scan` gates `has_lone_cr` at `max_bytes` in both `resolve` and `restart`.
  - When a file grows past the limit, one last diff runs (the highlighter is not disabled yet), then the path is skipped. That is acceptable.
  - Test `a_disabled_document_is_neither_diffed_nor_scanned` covers typing, undo and reload, and the reload back under the limit. Perf "100 MB json (disabled)" has a 0.71 ms median, and the other perf cases are unchanged.
- **Major B (dark syntax colours under the selection / find match below 3:1): closed as stated.** The extended `terminal_colour_themes_use_ansi_syntax_colours` checks 3:1 on `selection.bg` and `find_match.bg` for dark and light.
- **Minors: both closed.** `the_first_frame_does_not_wait_for_the_parse` now times `open_path` + the first draw. `read_mode_and_edit_mode_draw_the_same_colours` asserts the keyword colour first.
- **Design-QA colour changes (D178): no regression found.** Synthwave comment 9884c9 is 3.08 on find match, and the floors test covers both depths.

### New finding
[major] crates/vr-ide/src/theme.rs (Theme::dark, selection `Indexed(236)`, find_match `Indexed(52)`): in `dark`, the selection and find matches almost disappear on common terminal backgrounds
Scenario: `dark` paints no background; the terminal's own background shows. Contrast of the surface against that background (qa-artifacts/M4-3/review_r1_surfaces.py), old → new:

| Terminal background | Selection 239 → 236 | Find match 58 → 52 |
|---|---|---|
| black | 2.52 → 1.59 | 3.12 → 1.49 |
| #1e1e1e (VS Code-like) | 2.00 → 1.26 | 2.48 → 1.18 |
| #282c34 (One Dark) | 1.68 → 1.06 | 2.08 → 1.01 |
| #002b36 (Solarized) | 1.80 → 1.14 | 2.23 → 1.06 |
| #300a24 (Ubuntu) | 2.11 → 1.33 | 2.61 → 1.24 |

On a One Dark or Solarized terminal, the user selects text or searches and sees close to nothing marking it, including on whitespace and empty-line selections where no foreground can help. Fixing syntax readability on these surfaces made the surfaces themselves unreadable. That is worse for the user than finding B was.

Answer to the leader's question: yes, 236 equal to the status bar / popup grey is visible, but it is the smaller problem. A selection on the last editor row merges into the status bar row below it. Popups have a cyan border, so they stay apart.

Fix options (the user decides, D176 is confirm-on-return):
- (a) **Recommended.** Restore 239 / 58. In the palette themes (`dark`, `light`), cells under the selection / a find match keep the plain text foreground; syntax colour is dropped there. That was the pre-M4 look, and every ANSI colour problem goes away. Cost: one theme flag (e.g. `syntax_on_highlight: bool`, true for synthwave/dracula) read in editor_view's style match, plus one test per theme. About 20 lines.
- (b) Restore 239 / 58 and drop only the failing classes (DarkGray comment, LightRed number, LightMagenta keyword) to plain under the highlight. Smaller visual change, more rules.
- (c) Keep 236 / 52 and record the near-invisible selection on non-black terminals as user-accepted.

### Rule check M4-3 (round 1)
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | only the findings' files and the D178 palette |
| BASE-2 | pass | `Stats::scans` additive |
| BASE-3 | pass | r1-red-A.log, r1-red-theme.log; test-only minors |
| BASE-6 | pass | |
| BASE-25 | pass | fmt + clippy in notes |
| BASE-32 | pass | D176–D178 recorded; D177 trade-off challenged above |
| AC-13c | pass | disabled path skipped, 0.71 ms |
| AC-18 | pass | perf table r1 |
| T13 dark selection / find match visibility | fail | new major above |

## Round 2 re-check

Verdict: **APPROVED** (1 minor)

### Round 1 major (dark selection / find match nearly invisible): closed
- `Theme::dark` is back to selection `Indexed(239)` / find match `Indexed(58)` (theme.rs:88-89). On black these are 2.52 / 3.12, the same as before M4. `light` is unchanged (153 / 229).
- editor_view.rs:319 drops the capture for selected or found cells when `!theme.syntax_on_highlight`. Those cells then use `plain` (`theme.selection` / `text.patch(find_match)`), which is exactly the pre-M4 style. Cells outside the selection keep their syntax colour. Control chars are unchanged.
- The flag is false for dark/light and true for synthwave/dracula (theme.rs:128/178/459, synthwave.rs:117). Tests assert this per theme.
- `terminal_colour_themes_draw_plain_text_under_the_selection_and_find_matches` was red first (r2-red.log). It checks plain fg + selection bg on "fn", Function fg outside the selection, and plain fg on the "let" match, for both themes.
- `dark_highlight_surfaces_stand_out_from_a_black_terminal` would have failed on 236 / 52 (1.59 / 1.49). That is enough of a guard even though nobody saw it red.
- No regression: synthwave/dracula paths and their floors test are unchanged. D179 is recorded and supersedes D176/D177.

### New finding
[minor] README.md:279-280: the README is now wrong for `dark` and `light`
Scenario: the Syntax highlighting section says syntax colours "stay under the selection and find matches", but since D179 the `dark` and `light` themes draw plain text there. A user on `dark` reads the README and sees different behaviour. Docs only; behaviour is as designed.
Fix: "…and they stay under the selection and find matches in `synthwave` and `dracula` (`dark` and `light` show plain text there, as the terminal's palette is unknown)."

### Rule check M4-3 (round 2)
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | theme.rs, synthwave.rs, editor_view.rs, syntax_tests.rs only |
| BASE-2 | pass | additive `pub syntax_on_highlight` |
| BASE-3 | pass | r2-red.log; guard test not seen red, stated, judged adequate |
| BASE-6 | pass | |
| BASE-25 | pass | fmt + clippy in notes |
| BASE-32 | pass | D179 applied |
| T13 dark selection / find match visibility | pass | 239 / 58 restored, guarded |
| README matches code | fail (minor) | finding above |
