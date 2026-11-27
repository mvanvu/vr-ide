# Review FR-21 (code + design QA)
Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback. Ran `cargo test -p vr-ide --lib -- explorer kinds_` once via run-heavy.sh: 72 passed.

## Findings
[major] crates/vr-ide/src/theme.rs:~179 and :~253 (light table: Hidden = Color::Gray) — hidden files unreadable on the light theme
Scenario: `light` theme on a white terminal, a dotfile row such as `.bashrc` is drawn in ANSI 7 (Gray, typically #c0c0c0 / #d3d7cf), about 1.5-1.7:1 on white. The "dimmed" kind is nearly invisible, so AC-F5 (readable in every built-in theme) fails. The pre-existing `light.dim` has the same value, but this task adds a new use for it.
Fix: use an indexed grey that holds about 4.5:1 on white, e.g. `Color::Indexed(243)` (#767676), for Hidden in `light`.

[major] crates/vr-ide/src/theme.rs:~173-174 (dark table: Generated and Hidden = Color::DarkGray) — low contrast on common dark terminals
Scenario: Ubuntu default (#300a24, bright black #555753) gives about 2.3:1. Ptyxis and Solarized-style palettes set bright black close to the background. Lock files and dotfiles are then hard to read, and in the Hidden case only the italic separates them, so they look like the same kind.
Fix: use a fixed indexed grey, e.g. `Color::Indexed(246)` (#949494, about 5.4:1 on #300a24), and give Generated a different grey from Hidden (e.g. 244 vs 248), or keep the italic distinction only if you accept the identical colour.

[minor] crates/vr-ide/src/theme.rs:~250 (light: Indexed(166) Archive, Indexed(30) Docs-adjacent Media? see order) — marginal contrast on white
Scenario: Indexed(166) #d75f00 is about 3.8:1 on white (below 4.5); Indexed(30) #008787 is about 4.4:1. Both are borderline for text.
Fix: use Indexed(130) is taken, so use Indexed(124) (#af0000-ish) or Indexed(166)->Indexed(94) (#875f00, about 5.9:1) for Archive; Indexed(23) (#005f5f) for Media.

[minor] theme.rs dracula/synthwave tables: Generated vs Hidden are almost the same colour (dracula #8d9bc8 vs #9aa8d4, synthwave #9d8bd0 vs #b9a6e6)
Scenario: they differ only by italic, which some terminals do not render, so `Cargo.lock` and `.bashrc` look alike. The distinctness test passes because it counts the modifier.
Fix: make the two greys visibly different in hue or lightness, or accept this as intentional (both mean "quiet").

[minor] crates/vr-ide/src/ui/explorer.rs:63-67 — the focused row loses its kind colour
Scenario: the cursor row in a focused explorer shows `list_selected` fg, so the kind is not visible exactly where the user looks. This is documented and the contrast is guaranteed, so it is a design choice for the user, not a defect. Cost of changing: it needs a 4.5:1 check of every kind colour against `list_selected.bg` in every theme.

## Design QA on the preview (explorer-themes.ans, dracula and synthwave)
- Folders (purple) are told apart from files at a glance; source files split into native (pink) and script (green); config orange, docs yellow, media cyan or blue, archives red, dotfiles quiet and italic. Palette matches the Dracula / Synthwave taste in design_taste; no icons added (D311).
- Unfocused cursor and multi-selected rows keep the kind colour on the selection background (3.3-10.5:1 in the table). Contrast table matches the test thresholds (4.5 on bg, 3.0 on selection). No git markers exist in explorer rows, so nothing to collide with.
- dark / light: ANSI choices for the bright hues (LightBlue, LightRed, LightGreen, Yellow, LightCyan, LightMagenta, 208) are reasonable on dark backgrounds (Ubuntu #300a24, Ptyxis dark). The light ones (Blue, Red, 28, 130, Magenta) are reasonable on white. The exceptions are the findings above.

## Files read
| File | Status |
|---|---|
| README.md | read |
| crates/vr-ide/src/explorer_tests.rs | read |
| crates/vr-ide/src/theme.rs | read |
| crates/vr-ide/src/theme/synthwave.rs | read |
| crates/vr-ide/src/theme/explorer.rs (new) | read |
| crates/vr-ide/src/ui/explorer.rs | read |

## Rule check FR-21
| Rule | Result | Evidence |
|---|---|---|
| AC-F5 | fail | dracula/synthwave pass (theme.rs tests, contrast.md); dark/light Hidden and Generated are too faint (findings 1-2) |
| BASE-1 scope | pass | diff limited to theme, explorer UI, tests, README |
| BASE-2 | n-o | no contract touched |
| BASE-6 | pass | English comments, no history comments |
| BASE-16 reuse | pass | reuses `fit_floors`, table pattern of `captures` |
| BASE-20 | n-o | nothing deleted |
| BASE-25 guard | pass | notes show fmt and clippy -D warnings clean; tests re-run by reviewer, 72 pass |
| BASE-32 decisions | pass | D311 (colours only, no icons, theme tokens, 4.5:1) honoured for the true-colour themes; the 4.5:1 part is unmet on dark/light (see AC-F5) |
| BASE-35 user rules | pass | USER-2 (.ans preview), USER-3 (real build, no mockups) |

CHANGES_REQUESTED

## Re-check round 1
Ran `cargo test -p vr-ide --lib -- theme::` once: 27 passed. Read the round's diff of theme.rs, theme/synthwave.rs and theme/explorer.rs. The `.ans` preview was regenerated (it now carries the new Generated grey #a3a3ad).

| Earlier finding | Status | Evidence |
|---|---|---|
| major: light Hidden = Gray | fixed | Hidden is Indexed(243) italic (#767676, about 4.5:1 on white); Generated is Indexed(238) |
| major: dark Generated/Hidden = DarkGray | fixed | Indexed(246) and Indexed(250) italic (about 5.6:1 and higher on #300a24); Folder and Native moved to Indexed(75) and Indexed(203) |
| minor: light Archive and Docs marginal | fixed | Archive Indexed(125), Docs Indexed(23) |
| minor: Generated vs Hidden too close | fixed | dracula Generated #a3a3ad, synthwave #a9a9b6; test `explorer_generated_and_hidden_differ_by_colour_in_every_theme` asserts different colours |
| minor: focused row loses kind colour | open, not a defect | documented design choice; left to the user |

RGB table check: `xterm_rgb` in the new test uses the standard xterm values for the named ANSI colours (Red 205,0,0; Blue 0,0,238; LightBlue 92,92,255; DarkGray 127; Gray 229 and the rest). Indexed colours go through `indexed_rgb` with cube levels 0,95,135,175,215,255 and a grey ramp starting at 8 with step 10, which is the standard xterm-256 table. The new test checks every dark kind on #300a24 and black, and every light kind on white, at 4.5:1. It does not check dark on white or light on black (not the target backgrounds). Terminals that redefine the first 16 colours (Ubuntu, Ptyxis) can still differ a little from xterm for the named ANSI colours; this is minor.

New minor: light Archive Indexed(125) (#af005f) is close in hue to Red (Native) and Magenta (Media); it is told apart mainly by lightness. Optional.

| Rule | Result | Evidence |
|---|---|---|
| AC-F5 | pass | dracula/synthwave tests, dark/light test on the common backgrounds, regenerated preview |
| BASE-1 | pass | only theme files changed this round |
| BASE-2 | n-o | no contract touched |
| BASE-6 | pass | English comments |
| BASE-16 | pass | reuses `contrast`, `to_rgb`, `indexed_rgb` |
| BASE-20 | n-o | nothing deleted |
| BASE-25 | pass | theme:: tests re-run, 27 passed |
| BASE-32 | pass | D311 honoured: colours only, no icons, theme tokens, 4.5:1 |
| BASE-35 | pass | USER-2 (preview), USER-3 |

APPROVE
