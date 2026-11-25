# Review T24 — code + design QA (+ security pass on the config watcher)

Reviewer tools: Bash, Read, Write, Skill, SendMessage, SubagentHandback.

Verdict: **CHANGES_REQUESTED** (2 major, 4 minor). Security: **SECURE** (no finding).

Evidence: fresh release build (`cargo build --release -p vr-ide` via run-heavy: up to date with the tree);
dev walk re-run 31/31 PASS (`qa-artifacts/T24/review/dev-walk/`); reviewer walks
`qa-artifacts/T24/review/rwalk.py` (`rwalk_result.txt`, screens `r_*.txt`) and `rwalk2.py`
(`rwalk2_result.txt`, screens `r2_*.txt`), legacy bytes + SGR mouse. Walk FAILs that are script
artefacts, not product defects: r_01..r_03 (superseded by rwalk2 S1-S5), r_09..r_11 (ctrl+a in the
find field does not select; redone as rwalk2 R1-R4, all PASS), r2_10 R5 (ctrl+z sent in read mode
after Esc closed the box). r_07 (ctrl+` ignored while the command window is open) is pre-existing
modal behaviour, not a T24 change.

## Findings

[major] crates/vr-ide/src/config_reload.rs:39-45, 68-75 (+ app.rs:688-690, 739-741) — live reload is lost for a symlinked config file opened by its real path
Scenario: `~/.config/vr-ide/settings.toml` is a symlink to `~/dotfiles/vr/settings.toml` (common dotfiles setup); the user opens the dotfiles copy in vr-ide (workspace = dotfiles repo, ctrl+o, explorer). `Watcher` keys files by canonical path, so the tab's `watch(real path)` replaces the config watch's given path (`Scope.files` HashMap insert). Then (rwalk2, all reproduced on the release binary): S3 an external write is reloaded into the tab but never applied; S4 ctrl+s in vr-ide applies nothing (save hook compares lexical paths, `is_config_file(real)` is false); S5 after closing that tab the close guard does not fire (lexical compare again), `unwatch(real)` drops the only entry and every later external edit is silently never applied until restart. Control S1 (symlink, no tab on the target) applies fine.
Fix: compare by identity, not by lexical path: in `is_config_file` / `config_file_changed` resolve both sides (canonicalize, falling back to canonical parent + name like `vr_core::watch::resolve`), which makes the save hook, the fs-event hook and both close/rewatch guards work for either path; add a test with a symlinked settings.toml opened by its target path (save, external write, close tab then external write).

[major] crates/vr-ide/src/commands/terminal.rs:333-348 (close_tab) / 1106 — one click on a tab's 'x' hangs up a running program with no question
Scenario: a build, `npm run dev`, or vim with unsaved changes runs in tab 1; a click that lands one cell right of the title (the 'x' is a single plain cell inside the label) SIGHUPs the process group at once. Quit asks for the same situation ("1 terminal still running", Cancel default, `quit_allowed_for`); the 'x' does not. This follows the D280 leader reading "same guard as terminal.close" (which had none, but was a deliberate two-key chord, not a 1-cell mouse target) — the user decides. Options and cost: (a) keep as is (0); (b) ask "Close terminal? <title> is still running" Close / Cancel, Cancel default, only when `!shell_in_foreground()` (a `DialogAction::CloseTerminal(i)` + one test, ~30 lines; idle shell and exited tabs still close at once); (c) always ask while running (same cost, more prompts).

[minor] crates/vr-ide/src/config_reload.rs:83-128 — an empty settings.toml mid-write applies every default
Scenario: a writer that truncates then writes more than 200 ms later (slow script, network FS): the debounced event reads an empty file, valid TOML → every file key resets to its default and is applied ("settings.toml applied: tab_width, theme", r_22 — the theme flashes back to the default, session values of those keys are replaced), then the next event applies the content again. A partial write that is cut mid-line gives a red "not applied" error first instead. Self-healing, cosmetic.
Fix (cost ~10 lines): skip a watcher-triggered reload of a zero-length file (`saved == false && len == 0`), or re-read once after a short settle when the parse fails; say it in the D294 f note either way.

[minor] crates/vr-ide/src/ui/terminal_view.rs:111-123 (tab_label) — the close 'x' looks like title text
Scenario: titles "tmux", "box", "1 sh x" → " 1 tmux x " reads as one word; the 'x' has the label's own style, so nothing marks it as a button (screens r_14, r_17, snapshots). Options: dim/patch the 'x' cell with `theme.dim` or `tab_warning`-like style (render only, ~5 lines, snapshots change), or a different glyph (the user's call on glyphs).

[minor] crates/vr-ide/src/ui/find_bar.rs:23 — stale doc comment
"Enter replaces the current match, ctrl+a every match" — now Enter asks One / All / Cancel and ctrl+a does nothing. Fix: reword.

[minor] crates/vr-config/tests/config.rs:137-138 — `KNOWN` still lists terminal.new / terminal.close
The registry no longer has them; a test keymap naming them would be accepted by this list. Fix: drop the two names.

## Checked, no finding
- Keys: keymap.toml has exactly f4/shift+f4 (find.next/prev), f1 in [terminal]; f3, shift+f3, ctrl+f12, ctrl+f4 unbound (REMOVED_V2 test, walk 04/15); no other key changed. No f3 / ctrl+f12 / ctrl+f4 / terminal.new|close / /term new|close / /wrap left in README, docs, hints, welcome, palette (grep; only the D279 explanatory comment and tests).
- Replace dialog: Enter → [One] [All] [Cancel] with count; One default; Esc/Cancel replace nothing; edit mode back after Esc and the field keeps typing (R1-R3); All replaces all (R4); no match → notice, no dialog (B5).
- Live reload: external write applied, bad TOML / unknown theme refused with old values kept (D2), theme change applied (D3), /keymap save applies, 200 ms debounce in vr_core, removed/renamed-away file not read, config watch survives closing a tab opened by the config path, a user keymap binding terminal.new warns at startup (D1). No key sequences exist (D250/D261), so a keymap swap cannot strand a pending chord; `space_pending` is built in.
- ctrl+`: show/hide from editor, find box, explorer, focused terminal (B1-B3, dev walk 16-21).
- '+'/'x' hit testing: 40 cols bottom (C1-C3), right panel 120 cols (C4-C6), dev walk 22-25; hit test and render share `tab_bar`.
- Welcome "1 file" (dev walk 31, unit test).
- Security: reads only `<config_dir>/{settings,keymap}.toml`, writes nothing, follows symlinks as the start-up load already did; no new trust boundary (same-uid file, same values as at start). SECURE.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/config_reload.rs | read |
| crates/vr-ide/src/config_reload_tests.rs | read (test names/asserts) |
| crates/vr-ide/src/app.rs | read (diff) |
| crates/vr-ide/src/commands/{dialog,file,find,palette,search,slash,terminal}.rs | read (diff) |
| crates/vr-ide/src/ui/{find_bar,terminal_view,welcome}.rs | read (diff) |
| crates/vr-ide/src/lib.rs | read |
| crates/vr-config/defaults/keymap.toml, crates/vr-config/tests/config.rs | read |
| README.md, docs/commands.md, docs/keymap.md | read |
| snapshots (12 changed + 2 new) | read (diffs) |
| crates/vr-ide/src/{app,find,mode,slash,terminal,terminal_right,workspace_ui}_tests.rs, ui/tests/welcome_tests.rs | read (new test names), not line by line |
| .ai-dev-team/capabilities.md, decisions.md | skipped — team files |

## Rule check T24
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 scope | pass | keymap diff limited to the listed keys; no other key changed |
| BASE-10 tests + guards | pass | notes: vr-ide 531 / vr-config pass, clippy, fmt, full suite log; but no test for the symlinked config (finding 1) |
| BASE-14 no sleep/poll | n-o | self-attested |
| BASE-15 own processes | pass | no vr-ide left after the walks (both walks kill by pid) |
| BASE-16 reuse | pass | apply_settings, ConfirmDialog, Watcher::watch, tab_bar shared by render and hit test |
| BASE-17 / BASE-18 | n-o | self-attested |
| BASE-19 notes | pass | 04_dev_T24.md with progress, evidence, rule compliance |
| COST-8 / COST-9 | n-o | self-attested |
| SWEEP | n-a | terminal UI; snapshots + pty screens judged instead |
| Secure by default | pass | reload reads only the two config files, writes nothing |

## Round 1 re-check
Verdict: **APPROVE** (major 2 stays with the user, D295; not a blocker here). No new finding.

Evidence: `cargo test -p vr-ide config_reload` 9/9 pass (incl. symlink and both empty-file tests); `rwalk2.py` re-run on `target/release/vr-ide` (newer than config_reload.rs and terminal_view.rs) 10/10 PASS, S3-S5 included.

- Major 1 fixed. `identity()` = canonicalize (resolves symlink chains), else canonical parent + file name (the missing-file case, same as `vr_core::watch::resolve`), else lexical. `config_kind()` compares identities, and `is_config_file` and `config_file_changed` both go through it, so the save hook (file.rs:80, 471, 619), the fs-event hook (app.rs:1465) and the `rewatch` (app.rs:689) and `close_doc` (app.rs:741) guards all match a symlink or its target. Residual, not a defect: a dangling symlink falls to parent + link name, as the watcher does.
- Minor 3 (empty file): ignored only when `saved == false && len == 0` (config_reload.rs:96-99); a save of ours and a parse error still apply or show red. Cost: a file emptied on purpose by another program (`: > settings.toml` to reset to defaults) is not applied by the event; it applies on the next non-empty write or a vr-ide save, and a restart reads it normally. Acceptable (rare, no data loss, and the opposite choice flashes every default on a slow writer). Suggest one sentence in README/docs only if the user cares.
- Minor 4: the 'x' cell takes `theme.dim` fg over the label background (terminal_view.rs:270-275); no-op if the theme has no dim fg; text and snapshots unchanged.
- Minor 5: `REPLACE_HELP` comment reworded. Minor 6: `terminal.new` / `terminal.close` no longer in `KNOWN` (grep).

## Rule check T24 (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 scope | pass | only findings 1, 3-6 touched; no key change; major 2 untouched |
| BASE-10 tests + guards | pass | symlink + empty-file tests added, config_reload 9/9 re-run; notes: vr-ide 535, clippy, fmt, full suite 939 |
| BASE-14 no sleep/poll | n-o | self-attested |
| BASE-15 own processes | pass | walk ended by its own pids |
| BASE-16 reuse | pass | identity mirrors watch::resolve; theme.dim reused |
| BASE-17 / BASE-18 | n-o | self-attested |
| BASE-19 notes | pass | 04_dev_T24.md Round 1 |
| COST-8 / COST-9 | n-o | self-attested |
| SWEEP | n-a | terminal UI |
| Secure by default | pass | canonicalize reads metadata of the two config paths only |
