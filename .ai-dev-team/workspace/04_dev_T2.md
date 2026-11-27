# T2 vr-window renderer (PixelBackend, fonts, glyph cache) + bench gate

## Progress
- read brief, spec (Window and drawing), plan (T2, review focus 1-2), profile, capabilities
- deps: cosmic-text 0.19.0, sys-locale 0.3.2 added; rangemap pinned 1.8.0 (1.9.0 is < 2 weeks old)
- code: palette.rs, font.rs, canvas.rs, backend.rs + backend/tests.rs; cargo test -p vr-window 20 passed
- bench (release, run-heavy): gate passed, 4.82 ms full redraw at 1920x1080 scale 1
- visual check of the bench frame: qa-artifacts/T2/crop.png
- clippy -p vr-window clean; red shown by mutation (3 mutations, undone from a copy); guard green

## Files changed
- `Cargo.toml` — member `crates/vr-window`; workspace deps `vr-window` (path), `cosmic-text = "=0.19.0"`, `sys-locale = "=0.3.2"`
- `Cargo.lock` — new crates below; `rangemap` held at 1.8.0
- `crates/vr-window/Cargo.toml` (new) — deps ratatui, cosmic-text, unicode-width, unicode-segmentation, thiserror, sys-locale (all cross-platform; no Unix-only API used, no vr-ide dependency)
- `crates/vr-window/src/lib.rs`, `palette.rs`, `font.rs`, `canvas.rs`, `backend.rs`, `backend/tests.rs` (new)
- `crates/vr-window/examples/bench.rs` (new)
- `.ai-dev-team/capabilities.md` — `## vr-window` section appended

## API for T3 / T4
```rust
// fonts
let (fonts, warn) = FontSet::new(&FontConfig { family: Option<String>, size_pt: 12.0, line_height: 1.2, scale });
// warn: Option<FontError> (FamilyNotFound / InvalidSize / InvalidLineHeight / InvalidScale) -> show as warning; default applied
// FontSet::new loads system fonts (~0.1-1 s): call once. `system_font_system()` / `FontSet::with_font_system(fs, &cfg)`.
// backend
let mut b = PixelBackend::new(fonts, Palette::default(), width_px, height_px);
let mut term = ratatui::Terminal::new(b)?;        // Backend::Error = std::io::Error
term.backend_mut().resize(w, h, scale)?;           // re-lays the grid, blank canvas, full damage; then term.autoresize()/resize
term.backend_mut().set_font_config(&cfg)?;         // settings reload; Err keeps the old font
term.backend_mut().set_palette(p);                 // theme: indexed[256], foreground, background, cursor: Option<Rgb>
term.backend_mut().set_padding(logical_px);        // padding * scale around the grid; leftover pixels = background
term.backend_mut().set_cursor_shape(CursorShape::Bar);  // Block | Bar | Underline
term.backend_mut().set_cursor_blink_on(phase);     // blink phase; hide/show_cursor from ratatui still apply
term.backend_mut().set_preedit(Some(Preedit { text, col, row })); // IME composing text, underlined; None removes
term.backend_mut().render_overlays();              // repaint cursor/preedit now (flush() does it after each draw)
let rects: Vec<PixelRect> = term.backend_mut().take_damage(); // per grid row; whole canvas after resize/clear
term.backend().canvas().copy_rect_to(rect, &mut softbuffer_frame); // 0RGB, same width x height
let layout = term.backend().layout();              // GridLayout { cols, rows, origin_x, origin_y, cell: CellMetrics }
layout.cell_at(px, py) -> Option<(col, row)>;      // mouse
layout.cell_rect(col, row, 1) -> PixelRect;        // IME cursor area
```
Notes for T4: overlays are drawn in `flush`, so a cursor/preedit change without a ratatui draw needs `render_overlays()` (or `flush()`) before `take_damage()`. The canvas persists between frames, so copying only damaged rects is enough when softbuffer's buffer age says the frame holds the previous content; otherwise copy the whole canvas.

## Design
- Grid copy (`Vec<Slot>`) of every cell the backend got, so the cell under a moved cursor / removed preedit is repainted exactly; covered right halves of wide cells point to their lead cell.
- Wide cells (width 2, or `CellDiffOption::ForcedWidth`) paint background + glyph over two cells; a narrow cell over a former wide one also repaints the stale right half (ratatui does not resend an unchanged blank).
- Every glyph is clipped to its cell span, so changed-cells-only drawing can never leave fragments in neighbours.
- Glyph cache: (grapheme cluster, bold/italic) → images placed relative to the cell; ASCII in a flat table, others in a `HashMap<Box<str>, [_; 4]>`; emptied on every size/scale change, so size/scale are part of the key by construction. Rasterising via cosmic-text shaping (font fallback, combining marks) + `SwashCache::get_image_uncached` (mask and colour emoji).
- Cell size: advance of "M" in the resolved family (rounded), height = round(px × line_height); px = pt × 96/72 × scale. Baseline from cosmic-text's centred line of the main font (fallback fonts share it, so CJK/emoji never shift the row).
- Default family: fontdb's `monospace` (fontconfig alias on Linux: "DejaVu Sans Mono" here), then a list of common monospace names, then any monospaced face. cosmic-text's `FontSystem::new()` overwrites the monospace alias with "Noto Sans Mono", so `system_font_system()` builds the database itself.
- Modifiers: BOLD/ITALIC (font style), DIM (fg 55 % towards bg), REVERSED, HIDDEN, UNDERLINED (underline_color or fg), CROSSED_OUT; blink modifiers are drawn steady.

## Tests added (crates/vr-window)
`backend::tests::` grid_size_comes_from_the_pixel_size_and_the_cell_size, padding_moves_the_grid_and_leftover_pixels_keep_the_background, a_cell_with_a_known_colour_sets_pixels_inside_that_cell_only, named_and_indexed_colours_use_the_palette, a_wide_symbol_covers_two_cells, combining_sequences_are_one_glyph_in_one_cell (also ệ ở Ặ), emoji_stay_inside_their_two_cells, drawing_changed_cells_leaves_other_pixels_untouched (+ damage rect), modifiers_draw_lines_and_swap_colours, cursor_shapes_are_drawn_and_moving_the_cursor_restores_the_cell, a_redrawn_cell_under_the_cursor_gets_the_cursor_back, preedit_is_drawn_underlined_and_removed_cleanly, resize_relays_the_grid_and_scale_grows_the_cells, a_missing_font_family_is_an_error_and_the_default_is_used, a_named_family_resolves_case_insensitively, ratatui_terminal_draws_through_the_backend; `palette::tests::` (2), `canvas::tests::` (2).

Tests depend on installed system fonts (a monospace font, a CJK fallback); they assert placement properties, not exact glyph pixels.

## Commands run
```
$ cargo test -p vr-window
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
$ cargo clippy -p vr-window --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.32s
$ rustfmt --edition 2024 crates/vr-window/src/*.rs crates/vr-window/src/backend/tests.rs crates/vr-window/examples/bench.rs
$ $TEAM_BIN/run-heavy.sh cargo run -p vr-window --example bench --release -- --dump …   (log: qa-artifacts/T2/bench.log)
size        scale  grid      cold-full-ms  full-ms(median)  line-ms(median)
1920x1080  1      192x56         27.98             4.82            0.103
1920x1080  2      101x28         15.80             3.57            0.112
3840x2160  1      384x113        32.64            21.51            0.355
3840x2160  2      202x56         33.47            20.65            0.322
exit=0
$ $TEAM_BIN/run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'   (log: qa-artifacts/T2/guard.log)
warning: unused workspace dependency `vr-window`     <- cargo manifest warning; T4 adds the first user (vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.12s
exit=0
```
Mutation check of the tests (backend.rs copied to qa-artifacts, mutated, restored from the copy, copy removed; final run 20 passed):
- no right-half repaint after wide→narrow → `a_wide_symbol_covers_two_cells` FAILED
- overlay cells not restored → `cursor_shapes_…` and `preedit_…` FAILED
- every cell width 1 → `a_wide_symbol_covers_two_cells` FAILED

Bench: medians of 41 runs, alternating two frames that differ in every cell (mixed ASCII / Vietnamese / CJK / Hangul, per-cell RGB + indexed colours, some bold); "cold" = first frame including rasterising every glyph. Gate (full redraw ≤ 16 ms at 1920×1080 scale 1): **pass, 4.82 ms**. 4K at scale 1 is 21 ms (best effort, outside the gate; the cost is pixel fill + blending of 4× the pixels). Ran under run-heavy (low priority), so numbers are upper bounds.

## Crate versions (D74: published ≥ 14 days before 2026-11-28; dates from the crates.io index `pubtime`)
| crate | version | published |
|---|---|---|
| cosmic-text | 0.19.0 | 2026-04-22 |
| sys-locale | 0.3.2 | 2024-11-01 |
| swash | 0.2.10 | 2026-07-17 |
| fontdb | 0.23.0 | 2024-10-09 |
| harfrust | 0.5.2 | 2026-03-04 |
| skrifa | 0.40.0 / 0.44.0 | 2026-01-07 / 2026-07-04 |
| read-fonts | 0.37.0 / 0.41.0 | 2026-01-07 / 2026-07-03 |
| font-types | 0.11.3 / 0.12.7 | 2026-04-13 / 2026-10-15 |
| rangemap | 1.8.0 (pinned; 1.9.0 is from 2026-11-24) | 2026-08-14 |
| bytemuck_derive | 1.12.1 | 2026-09-11 |
| core_maths | 0.1.1 | 2025-02-01 |
| fontconfig-parser | 0.5.8 | 2025-05-12 |
| linebender_resource_handle | 0.1.1 | 2025-09-16 |
| roxmltree | 0.20.0 | 2024-05-23 |
| self_cell | 1.3.0 | 2026-07-16 |
| slotmap | 1.1.1 | 2025-12-06 |
| smol_str | 0.3.7 | 2026-10-20 |
| tinyvec | 1.13.3 | 2026-09-13 |
| ttf-parser | 0.25.1 | 2024-11-29 |
| unicode-bidi | 0.3.18 | 2024-12-16 |
| unicode-linebreak | 0.1.5 | 2023-07-25 |
| unicode-script | 0.5.9 | 2026-11-02 |
| yazi | 0.2.2 | 2026-11-01 |
| zeno | 0.3.3 | 2025-05-08 |
Reason: cosmic-text is the spec's chosen shaper (fallback, combining marks, emoji, CJK); sys-locale (already a cosmic-text dependency) gives fallback ordering the system locale, since the font system is built by hand to keep fontconfig's monospace alias.

## Deviations from the plan
- The plan says "RGBA `Canvas`"; the brief asks for softbuffer's `u32` 0RGB — the canvas is 0RGB.
- Tests were written right after the code rather than before it; red was shown afterwards by mutation (above).

## Out-of-scope issues noticed
- `cargo` prints `unused workspace dependency vr-window` until T4 makes vr-ide depend on it (warning only; guard passes).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| shaping / rasterising / fallback | capabilities.md, spec | none in repo; cosmic-text (spec choice) |
| grapheme splitting, widths | workspace deps | reused `unicode-segmentation`, `unicode-width` |
| colour palette / theme colours | capabilities.md, `grep -rn "Indexed" crates/vr-ide/src/theme*` (not imported: vr-window must not depend on vr-ide) | new `Palette`; T4 fills it from the theme |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff: Cargo.toml, Cargo.lock, crates/vr-window/** (+ capabilities.md, notes); `git status` shows vr-ide changes are T1's |
| BASE-2 | pass | `Backend` impl matches ratatui-core 0.1.2 trait (no `scrolling-regions` feature enabled in the workspace); API listed above |
| BASE-3 | pass | tests written right after the code (deviation noted); red shown by 3 mutations, each failing a named test (Commands run) |
| BASE-4 | pass | trimmed outputs above; logs qa-artifacts/T2/bench.log, guard.log |
| BASE-5 | pass | release bench and guard ran via run-heavy.sh; nothing left running |
| BASE-6 | pass | English code/comments; comments explain why |
| BASE-7 | pass | no commit |
| BASE-8 | pass | version/date table above; rangemap pinned to 1.8.0 to meet 14 days |
| BASE-10 | pass | only `cargo test -p vr-window`, `clippy -p vr-window`, guard once |
| BASE-11 | pass | no checkout/restore/stash/switch |
| BASE-13 | pass | only removed my own copy qa-artifacts/T2/backend.rs.orig and frame-1080.ppm (created in this task) |
| BASE-14 | pass | bench ran as a background task, turn ended, resumed on notification; no polling |
| BASE-15 | pass | no process stopped; the single background task finished by itself |
| BASE-16 | pass | Reuse table; capabilities.md `## vr-window` added |
| BASE-17 | pass | no toolchain/system package installed; crates via cargo only |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created before the first change; Progress section |
| BASE-21 | pass | no secrets in output |
| BASE-22 | pass | crate/dep names follow workspace style (`vr-*`, `name.workspace = true`, `=` pins) |
| BASE-24 | pass | no git index/history command |
| BASE-25 | pass | guard exactly as profile: exit 0 (guard.log) |
| BASE-32 | pass | D3: vr-window has no App knowledge, no vr-ide dependency (crates/vr-window/Cargo.toml) |
| BASE-38 | pass | rustfmt only on the changed files |
| BASE-49 | pass | APIs read from ~/.cargo/registry sources (ratatui-core backend.rs, cosmic-text, fontdb); no program run for usage |
| BASE-51 | pass | mutation experiments made on a copy of backend.rs and restored from it; copy removed; final `cargo test -p vr-window` 20 passed |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no sleep/pgrep |
| COST-8 | pass | new files written whole, edits batched |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set = crate's unit tests (no other test names these files) |
| COST-13 | pass | scratch files only under qa-artifacts/T2 |
| COST-16 | pass | bench/guard output to logs, only summaries read |
| AC-W2 | pass | font config (family/size pt/line height/scale) `FontConfig`; wide, combining, Vietnamese, emoji tests; palette tests (Rgb as is, 16 named + 256 indexed, overridable); visual: qa-artifacts/T2/crop.png |
| AC-W3 | pass | bench: full redraw 4.82 ms at 1920x1080 scale 1 (≤ 16 ms); changed-cells-only test `drawing_changed_cells_leaves_other_pixels_untouched` |

n-a: BASE-9 (no input boundary, auth or queries: an in-process renderer) · BASE-12, BASE-29, BASE-30, BASE-31, BASE-33, BASE-40, BASE-41, BASE-48 (no other repo, system, container, browser or outward write) · BASE-20 (nothing removed) · BASE-23 (one entry path: the Backend trait; also exercised via `ratatui::Terminal` test) · BASE-26 (not a rewrite) · BASE-27, BASE-44 (no shared data, no fakes) · BASE-28 (no third-party code changed) · BASE-34 (no blocked step) · BASE-35 (no USER-n lines for this area) · BASE-36 (no app scope change) · BASE-37, BASE-47 (not a defect fix) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-43 (no access rules) · BASE-45 (AC-W2/W3 used as written in the spec) · BASE-46 (no runtime qc walk; bench ran on the final renderer code) · BASE-50 (no text for it in rule-packs.md §3 of plugin 1.0.45, which ends at BASE-49) · COST-2, COST-4, COST-5, COST-6, COST-7, COST-11, COST-12, COST-14, COST-15, COST-17..21 (no browser, e2e, audit, CI, goldens or qc role in this task)

## Fix round 1
- read findings 1-5; reading backend.rs relayout/render_overlays, font.rs cache, fontdb fontconfig alias handling
- tests written first, stubs added so they compile; red run (qa-artifacts/T2/round1-red.log): 9 failed, incl. `the_default_family_is_the_fontconfig_monospace_match` left "FreeMono" right "DejaVu Sans Mono" (QC observation reproduced), padding "attempt to multiply with overflow" at backend.rs:52
- fixes 1-5 implemented; green run (round1-green.log): 31 passed
- red shown on copies for the two tests whose first red was not meaningful (overlong cluster: stub limit; ratatui relayout: test bug fixed): both FAILED with the fix removed, then restored from the copy, 31 passed
- clippy -p vr-window clean; guard exit 0 (guard-r1.log)

### Findings
1. fixed — `backend.rs` `relayout`: when cols×rows stay the same the grid is kept and `repaint_grid()` paints it at the new metrics/origin (full damage); only a changed grid size blanks it (ratatui then redraws). Tests: `relayout_that_keeps_the_grid_size_keeps_the_cells` (same-size resize, same-grid bigger window, padding change, font reload), `relayout_through_ratatui_keeps_the_screen`.
2. fixed — `backend.rs` `render_overlays` + new `preedit_cells()`: the preedit is laid out first; a wide grid cell cut at either end of the preedit gets a blank in its background colour for the cut half (no half glyph, no repaint of the wide cell over the preedit); the cursor on a preedit cell follows the preedit's cells instead of mapping to the grid's wide lead. Removing the preedit restores the wide cell (pixel-exact). Tests: `preedit_ending_on_a_wide_cell_keeps_its_last_grapheme_and_no_stale_half`, `preedit_starting_on_a_wide_cells_right_half_leaves_no_left_half`, `preedit_across_a_wide_cell_mid_text_is_fully_drawn`.
3. fixed — `GridLayout::new` uses `padding.saturating_mul(2)`; `set_padding` clamps to `MAX_PADDING` = 1000 logical px. Test: `huge_padding_does_not_overflow`.
4. fixed — `font.rs`: `CACHE_LIMIT` = 4096 non-ASCII clusters (the map is emptied when full and refills with what is on screen; ASCII table is fixed 128×4); clusters cut at `MAX_CLUSTER_BYTES` = 64 (char boundary) before shaping and caching; `MAX_SIZE_PT` = 200 (was 400). Tests: `the_glyph_cache_is_bounded`, `overlong_clusters_are_cut_before_shaping_and_caching`, `font_sizes_above_the_cap_are_refused`.
5. fixed — root cause: fontdb sets `monospace` to the first `<prefer>` of the *last* `monospace` alias in the config, installed or not ("FreeMono" here). New `system_database()` (used by `FontSet::new` via `system_font_system()`, and by the tests) re-reads the same fontconfig files with `fontconfig-parser` and picks like fontconfig: every `<prefer>` in rule order, then `<accept>` (later rules first), then `<default>`, first installed family wins. Linux/BSD only (cfg as fontdb's). Tests: `the_monospace_alias_takes_the_first_installed_preferred_family_in_config_order` (pure), `the_default_family_is_the_fontconfig_monospace_match` (machine check: with DejaVu Sans Mono installed and the system config containing 60-latin.conf, the default family is "DejaVu Sans Mono" = QC's `fc-match monospace`; it failed with "FreeMono" before the fix).

### Files changed (round 1)
- `crates/vr-window/src/backend.rs`, `crates/vr-window/src/backend/tests.rs`, `crates/vr-window/src/font.rs`
- `crates/vr-window/Cargo.toml` — target dependency `fontconfig-parser` (cfg(all(unix, not(macos/android))))
- `Cargo.toml` — `fontconfig-parser = "=0.5.8"` (published 2025-05-12; already in the lock through fontdb, no new crate in Cargo.lock)

### New states (round 1)
- Kept grid on relayout: a second relayout repaints again (idempotent); a relayout while a preedit/cursor is shown resets the overlays, which `flush` repaints; a grid-size change still blanks (ratatui redraws).
- Glyph cache emptied when full: next frame re-rasterises what is visible (cost bounded by the screen); no data at risk.
- Preedit edge blanks: restored with the overlays when the preedit changes or goes away (tests compare with the plain canvas).

### Commands (round 1)
```
$ cargo test -p vr-window            (round1-red.log, before the fixes, stubs only)
test result: FAILED. 22 passed; 9 failed
$ cargo test -p vr-window            (round1-green.log)
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.16s
$ cargo clippy -p vr-window --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.23s
$ $TEAM_BIN/run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'   (guard-r1.log)
warning: unused workspace dependency `vr-window`   (manifest warning, unchanged)
exit=0
```
Bench not rerun: the draw hot path changed only by a char-boundary cut and a map-size check on a cache miss.

### Compliance rows changed (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | new tests written before the fixes and seen failing (round1-red.log, 9 FAILED; the two others shown red on copies) |
| BASE-4 | pass | outputs above; round1-red.log, round1-green.log, guard-r1.log |
| BASE-8 | pass | fontconfig-parser 0.5.8 (2025-05-12), reason: fontconfig's monospace order; no new crate in the lock |
| BASE-25 | pass | guard exit 0 (guard-r1.log) |
| BASE-51 | pass | mutations on copies of backend.rs / font.rs, restored, copies removed; final 31 passed |
| AC-W2 | pass | default family = fontconfig monospace match (`the_default_family_is_the_fontconfig_monospace_match`); preedit over wide cells tests |
