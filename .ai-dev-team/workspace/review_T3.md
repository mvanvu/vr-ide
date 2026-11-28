# Review T3 (sonnet) — APPROVED
1. [major, outside T3] event_loop.rs:47 chord_from_key: TUI kitty decode drops SHIFT on ctrl/alt+shift+symbol ("ctrl+!") while the window gives "ctrl+shift+1" → anti-goal. Fix in vr-ide: when ctrl||alt and c is a US shifted symbol, set shift and unshift. → T4 scope.
2. [minor] key.rs:86,93 Windows AltGr typed chars become ctrl+alt chords → T4/T6 track (Windows out of scope).
3. [minor] key.rs:89 shift+space: window drops SHIFT, TUI likely "shift+space" → parity; quick pass after QC.
Rule check: AC-W4 pass, AC-W5 pass, anti-goal fail (vr-ide side, finding 1), deps pass.

# Re-check T3b — CHANGES_REQUESTED
- [major] event_loop.rs:46-54 user keymap chords like ctrl+{ / ctrl+" (documented, tested in vr-config) never match now; normalise at keymap parse.
- [minor] tests for ctrl+alt+shift+symbol with SHIFT set, alt path through the window.
Re-check T3b round 1: APPROVED (normalised in KeyChord::new; USER-5 guard same physical key).
