# Review T2 (sonnet) — CHANGES_REQUESTED
1. [major] backend.rs:314-322 relayout blanks the grid on resize/set_font_config/set_padding with unchanged cols×rows; ratatui does not redraw (autoresize only on size change) → blank window. Test missing.
2. [major] backend.rs:271-275 preedit ending on the right half of a wide cell: restore(k+1) repaints the wide lead over the preedit's last grapheme; narrow preedit over a wide lead leaves right-half ink. Test missing.
3. [minor] backend.rs:52 `2 * padding` overflow on huge padding; clamp set_padding.
4. [minor] font.rs:122,258 glyph cache unbounded; cap entries / cluster length / size_pt max.
Rule check: AC-W2 pass, AC-W3 pass, BASE-2 fail, BASE-3 fail, BASE-8/16/32 pass.
Re-check round 1: APPROVED. New minor: font.rs:432-457 only <alias> rules honoured, <match> ignored (fontdb same) → Phase 6 minor.
