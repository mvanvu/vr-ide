# Phase 5 — ui dimension (opus) raw findings
U1 high window_frontend.rs:291 SIGHUP/SIGINT kill the window with no recovery (old gnome-terminal entry first start, `&` then closing the shell, ctrl+c).
U2 major font.rs:176 missing family at start drops font_size/line_height (falls back to 12pt/1.2); reload keeps them.
U3 major app.rs:272 CSD title bar follows system light theme, not app theme.
U4 major backend.rs:353 leftover pixels filled with editor background → strip under status bar / beside panels.
U5 major backend.rs:245 IME caret stays at preedit start; PreeditText::cursor_columns unused.
U6 minor app.rs:458 unfocused cursor stays solid (terminals show hollow).
U7 minor desktop.rs:257 start notices worded inconsistently; 'warning:' prefix string matching.
U8 minor README:847 line_height documented as multiple of font height; code uses font size.

# Phase 5 — correctness dimension (opus) raw findings
C1 high window_frontend.rs:295 SIGINT/SIGHUP kill the window, no quit path/recovery (same as U1 — found independently by both dimensions → confirmed without a separate verifier).
C2 major input/key.rs:101 super/cmd chords reach the app as the plain key (cmd+c types 'c').
C3 major app.rs:356 IME composing state not cleared on focus loss (uncertain, IME-dependent) → keys swallowed.
C4 minor desktop.rs:75 --install-desktop in a window build writes Terminal=false without checking a window can open.

# Verification
U2 REAL (major): font.rs:176-185 default on FamilyNotFound; QC T4 pass on f1_font.png was wrong (cells 10x19 = 12 pt).
U4 NOT REAL as defect (spec 01_spec.md:127 asks for background padding) → minor polish (extend edge colours), rides along in FR-1.
U3 REAL, re-graded minor (user session color-scheme default → light title bar over synthwave; spec silent on colours) → FR-1 minor, taken (visible on the user machine).
U5 REAL, re-graded minor (caret at preedit start; spec only asks preedit at cursor) → FR-1 minor.
C2 REAL (major): super+letter not grabbed by GNOME (b c e f g i j k r t u w x y z…) types the letter in editor/dialog/terminal.
C3 NOT REAL as described (winit self-heals on X11; Wayland sends Disabled) → minor: clear stale preedit on focus loss.
