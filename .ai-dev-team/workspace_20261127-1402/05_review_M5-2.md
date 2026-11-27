# Review M5-2 — terminal panel (round 0)

Reviewer tools: Bash, Read, Write, Skill, SendMessage, SubagentHandback.
Scope: `git diff master..HEAD -- crates docs README.md` at 4bb6bad (vr-ide, vr-config, docs; crates/vr-term is M5-1's, reviewed there).

Verdict: **CHANGES_REQUESTED** (3 major, 4 minor)

## Findings

[major] crates/vr-ide/src/commands/terminal.rs:658-682, :282-289; ui/terminal_view.rs:24-29; README.md "Terminal" last bullet — no key hides the panel in a legacy terminal, and the hint and README say there is one
Scenario: GNOME Terminal / xterm / tmux (no kitty protocol) → NUL opens the panel → inside the terminal NUL goes to the shell (D206), the tab bar still says "ctrl+` hide" → alt+q → NUL from the editor *focuses* the shown panel again (state machine "shown, not focused → focus"). The palette cannot help either (F1 / ctrl+shift+p go to the program; from the editor "Toggle Terminal" also only focuses). The only way to hide it is to close every terminal. README says "use alt+q to leave and ctrl+` from the editor to hide", which is false. AC-14a ("shows/focuses/hides … in kitty and legacy terminals") fails for legacy. Seen on the build: `legacy-synthwave-120x40-1-open` / `-5-refocus` (hint "ctrl+` hide" in a legacy session).
Fix (leader / user picks, it changes the spec's state machine): (a) legacy only: a NUL outside the terminal while the panel is shown hides it (focus by click or alt+q-and-NUL is lost — cheap, one branch in `legacy_toggle`); (b) a second default `[terminal]` key that legacy terminals can send (not VS Code, needs a USER-1 exception); (c) minimum: in legacy mode the hint shows only "alt+q editor", README says the panel is hidden from the palette, and the palette's "Toggle Terminal" hides when the panel is shown and was the last focus. Any option needs a test that a legacy session can hide the panel.

[major] crates/vr-ide/src/ui/terminal_view.rs:58-87, commands/terminal.rs:263-275, ui/terminal_view.rs:164-180 — with real shell titles the tab bar hides the active tab and every label reads the same
Scenario: Ubuntu's default `.bashrc` sets the OSC title `user@host: dir`. Four tabs at 80x24 (`tabs-80x24-4tabs.txt`): ` 1 rainy@rainy-Lenovo-IdeaP  2 rainy@rainy-Lenovo-IdeaP  3 rainy@rainy-Lenovo-Id` — the active tab 4 is not drawn, tabs 1-3 are indistinguishable (24-char cut keeps the useless prefix), the hint is dropped, and the status bar drops " 4/4 · 80×9" because the long title leaves no room (`render_status` else-branch). The user cannot tell which terminal has the keys.
Fix: keep the active tab in view (scroll window with a `+N` / `‹` marker as the editor tab bar does); share the width between labels (min ~8 cols) and cut titles from the left keeping the end (`…/m52r-proj`); in the status bar truncate the title so `i/n · cols×rows` always shows. Add a 4-tab long-title snapshot at 80x24 and 40x12.

[major] crates/vr-ide/src/ui/terminal_view.rs:139 — exited banner clipped at 40 columns
Scenario: 40x12, `exit 3` → " [process exited 3] — Enter to restart," — the close key is cut off (`legacy-synthwave-40x12-7-exited.png`). 40x12 is a supported size (snapshot exists, but with a short exit code it is still cut).
Fix: a short form when the long one does not fit, e.g. " exited 3 · Enter restart · ctrl+w close" (39 cols), then " exited 3 · Enter · ctrl+w".

[minor] crates/vr-ide/src/commands/terminal.rs:517-521 — inactive tab title stays stale after the panel was hidden
Scenario: two tabs, hide the panel, tab 2 prints / changes its title → its Notify fires once, `refresh` swaps the flag without a snapshot (panel hidden), so vr-term never re-arms it; after showing the panel tab 2's label keeps the old title until it is activated (the `[N]` marker is live, the title is not).
Fix: on `reveal`, `refresh()` every tab once (re-arms all Notifies).

[minor] crates/vr-ide/src/ui/terminal_view.rs:210-212 — concealed text (SGR 8) visible in dark/light
Scenario: theme dark, `printf '\e[8msecret\e[0m'` → bg is `Reset`, so `fg = bg` sets fg `Reset` and the text shows.
Fix: for `hidden` cells draw `" "` instead of the symbol.

[minor] crates/vr-ide/src/app.rs:929-935 (with status_bar notices) — notices cannot be dismissed while the terminal has the keys
Scenario: legacy terminal → the "does not report these keys" notice (now also listing ctrl+shift+`) takes 3 rows for the whole session; Esc goes to the program, so at 80x24 the grid is 9 rows, at 40x12 4 rows (`legacy-synthwave-40x12-*`). Pre-existing notice, made worse by the panel.
Fix: dismiss a notice on click, or let alt+q's first press in the editor clear it; consider not counting ctrl+shift+` (palette fallback documented).

[minor] crates/vr-ide/src/commands/terminal.rs:779-784 — a program-reported drag that ends outside the panel never gets its release
Scenario: vim with `mouse=a`, drag a visual selection and release over the editor → the Up event goes to the editor; vim stays in drag mode.
Fix: remember "mouse captured by program" on a forwarded press (like `selecting`) and forward Drag/Up clamped to the grid until the release.

Checked, no finding: D8 routing (Terminal context has no global fallback, `vr-config/src/keymap.rs:101-108`; ctrl+q, F1, ctrl+p, ctrl+s, Esc, paste, mouse go to the program — ctrl+q inside the terminal reaching the shell matches the spec, only alt+q / ctrl+` are reserved, D207 adds ctrl+shift+`); release events dropped before routing; kitty toggle show/hide/focus on the build; read mode not applied, alt+q lands in read mode; TERM badge; quit guard on the build (Cancel default, Enter cancels, Quit anyway exits in 0.01 s and `bash` + `sleep 300` are gone); shutdown in `event_loop::exit`, panic path drops `App` while unwinding (vr-term Drop hangs up), SIGKILL of vr-ide closes the master and the kernel hangs up the session (verified: no survivors after every capture); close drops on a helper thread; AC-1 (no spawn until opened); wide chars (raw bytes `日` at col 1, `本` at col 3 — correct); cursor only while focused (hidden after alt+q, hidden under htop); fit on every frame; idle CPU with the panel open 2 ticks / 5 s (0.4 %); welcome.rs and monitor.rs identical to master (T17 round 1 kept).

## Design QA

Real release build (4bb6bad) in a pty, TERM=xterm-256color, COLORTERM=truecolor; artifacts in `.ai-dev-team/workspace/qa-artifacts/M5-2/review/` (`cap_m52.py`, `quit_m52.py`, `tabs_m52.py`, `rawwide.py`; frames `*.txt`/`*.html`, PNGs for 120x40 htop / alt+q, 80x24 ls, 40x12 exited, dracula, dark).
- Layout: panel under the editor column, 40 % → 120×15 grid at 120x40, 80×9 at 80x24, 40×4 at 40x12; welcome falls back to its compact layout above it. Tab bar row doubles as separator (distinct bar colour) — reads clearly in synthwave, dracula and dark.
- Colours: synthwave/dracula paint the panel with the editor background and their ANSI tables (ls colours, htop bars readable); dark keeps the terminal's colours. TERM badge pink (synthwave) / magenta (dark), distinct from READ/EDIT.
- htop 2 s: renders fully, no cursor, F-key bar intact at 120x40 and 80x24; at 40x12 only 4 rows (with the legacy notice).
- Failing: legacy hint "ctrl+` hide" (finding 1), multi-tab bar (finding 2), exited banner at 40 cols (finding 3), notice rows (minor).
- Not captured: scrollback indicator and selection highlight (code-reviewed only; the capture model has no mouse).
- Side effect: the user's bash profile starts an `ssh-agent -s` per interactive shell; my capture shells started some of them (about 04:38-04:42), but QC's parallel shells do the same and their environ is unreadable (setgid), so I could not attribute PIDs and killed none (BASE-15). Leader may want to clean `ssh-agent -s` processes from 04:37-04:43.

## Files read

| File | Status |
|---|---|
| README.md, docs/keymap.md | read |
| crates/vr-config/defaults/keymap.toml, defaults/settings.toml, src/settings.rs, tests/config.rs | read |
| crates/vr-ide/Cargo.toml, src/app.rs, src/lib.rs, src/event_loop.rs | read (diff + handle_key / paste / mouse routing) |
| crates/vr-ide/src/commands/terminal.rs | read |
| crates/vr-ide/src/commands/app.rs, dialog.rs, mod.rs, registry.rs | read |
| crates/vr-ide/src/ui/terminal_view.rs, layout.rs, mod.rs, status_bar.rs | read |
| crates/vr-ide/src/theme.rs, theme/ansi.rs, theme/synthwave.rs | read |
| crates/vr-ide/src/terminal_tests.rs | read (test list, startup / routing / paste tests) |
| crates/vr-ide/src/ui/tests/welcome_tests.rs + welcome / palette / empty-project snapshots | read (diff) |
| 4 new terminal snapshots | skipped — generated; judged on the real build instead |
| crates/vr-ide/src/ui/welcome.rs, monitor.rs | compared with master (identical) |
| crates/vr-term/** | skipped — M5-1's diff; read only Drop, notify, snapshot, status for contracts the panel relies on |
| Cargo.lock | skipped — lockfile |

## Rule check M5-2

| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to vr-ide, vr-config defaults/settings, docs, README; vr-term only via the M5-1 merge |
| BASE-2 | pass | plan names kept (`terminal.*`, `focus.editor`, `Theme::ansi_palette`); additive fields; `DialogAction::QuitTerminals` |
| BASE-3 | pass | 17 tests red on stubs (notes); the 3 green-on-stub ones are guards of properties the stub already had (no spawn at startup, palette NUL no-op, crossterm NUL mapping) — acceptable as regression guards, they fail if the property breaks |
| BASE-6 | pass | English, why-comments (drop_later, refresh, legacy_toggle) |
| BASE-25 | pass | notes: fmt --check and clippy -D warnings clean after the vr-term merge (not rerun by the reviewer) |
| BASE-32 | pass | compared against D8, D10/USER-1, D145, D185, D194-D199, D205-D213: implementation follows them; the legacy hide gap (finding 1) is an AC-14a gap the decisions did not settle |
| New-states | pass | `## New states (round 0)` table present with tests; missing row: "legacy terminal, panel shown" has no hide path (finding 1) |
| AC-14a | fail | legacy terminals cannot hide the panel by key (finding 1) |
| AC-14b, AC-14d, AC-1 | pass | exited banner + Enter restart, quit guard and child kill verified on the build; no spawn at startup |
| AC-14 (panel part) | pass | htop renders and resizes in the panel at 3 sizes; lazygit / vim walk is M5-3 |

## Round 1 re-check

Verdict: **APPROVED**. All 3 majors and all 4 minors are fixed and nothing regressed. One new cosmetic note, optional.

Scope: the uncommitted round-1 diff on 4bb6bad (`git diff` in the worktree): commands/terminal.rs, ui/terminal_view.rs, ui/status_bar.rs, app.rs, vr-config settings, defaults, tests, README, docs/keymap.md, plus 2 new snapshots (skipped, generated). Compared against D214–D219.

Build: release build of the round-1 tree. cargo reported it up to date at 05:01, so the binary is the current tree. Replay in a pty with `/bin/sh` and `ENV=/dev/null`: legacy at 80x24 and 40x12, kitty at 80x24. Script and frames: `qa-artifacts/M5-2/review/r1/` (`cap_r1.py`, `r1-*.txt`/`.html`, 3 PNGs). No ssh-agent was started (none newer than 04:49), and every vr-ide and shell I started has exited.

| Finding | Status | Evidence (build) |
|---|---|---|
| 1 legacy hide (major) | closed | Legacy 80x24 and 40x12: NUL opens the panel focused with the hint "alt+q editor · ctrl+` hide". NUL inside hides the panel and the shell keeps running. alt+q changes the hint to "ctrl+` focus". NUL from the editor focuses the panel, then hides it. Code: `handle_key` looks up a [terminal] ctrl+space binding first, then ctrl+`. `terminal_pass_nul` (default false) restores pass-through, and the hint drops "hide" in that mode (`hide_key_works`). |
| 2 long titles (major) | closed | 80x24, 4 tabs with `user@host: ~/DEV/x` titles: ` 1 …EV/alpha  2 …DEV/beta  3 …EV/gamma  4 …EV/delta  alt+q editor · ctrl+` hide`; status shows `… ~/DEV/delta  4/4 · 80×9`. 40x12: ` 2 …EV/beta  3 …V/gamma  4 …V/delta  +1`; status shows `…5-15IWL: ~/DEV/delta 4/4 · 40×4`. Clicking tab 3 activates it (status 3/4), so drawing and the click hit test share one layout. |
| 3 banner at 40 cols (major) | closed | 40x12: ` exited 3 · Enter restart · ctrl+w close`. 80x24: the long form. Enter restarts. |
| stale inactive titles | closed | `reveal` snapshots every tab when the panel was hidden (code + dev test). |
| SGR 8 | closed | `printf '\033[8mHID%sDEN…' X`: "HIDXDEN" is not drawn. |
| notice dismiss | closed | Legacy: clicking the keyboard notice row while the terminal has the keys removes it, and the TERM focus is kept. |
| mouse capture | closed | Program with `?1000h ?1006h` running `cat -v`: press in the grid gives `^[[<0;5;3M`, release over the welcome area gives `^[[<0;7;1m` (clamped to the grid). No drag report is expected under mode 1000. |

Regression checks on the build:
- Kitty mode: toggle show/hide/focus, 4 tabs via ctrl+shift+`, and the banner all pass.
- Quit anyway exits, and all 4 shells are gone in each run.
- AC-1 path unchanged: no spawn until the panel is opened.
- The dev's test log shows vr-ide 344 passed, vr-config 3+18+8 passed and vr-term green; fmt and clippy are clean.

New note (minor, optional, not blocking): `ui/terminal_view.rs` `tab_bar` always draws `+N` after the visible tabs. When the hidden tabs are on the left (active tab 4 at 40x12: ` 2 … 3 … 4 … +1`), the marker suggests more tabs to the right. Fix: draw `+N ` before the window when `start > 0` (or `‹N` / `N›` per side).

### Files read (round 1)
commands/terminal.rs, ui/terminal_view.rs, ui/status_bar.rs, app.rs, vr-config src/settings.rs, defaults/settings.toml, defaults/keymap.toml: read (diff). terminal_tests.rs: read the test list. README.md, docs/keymap.md: skimmed (pass_nul and banner text). The 2 new snapshots: skipped, generated; judged on the build.

### Rule check M5-2 (round 1)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only the findings' files; vr-term untouched |
| BASE-2 | pass | additive setting `terminal_pass_nul`; `labels()` removed and both of its users moved to `tab_bar` |
| BASE-3 | pass | a red test per finding in the notes; the kitty guard was green on arrival (documented, acceptable) |
| BASE-6 | pass | English, why-comments |
| BASE-25 | pass | notes: FMT-OK, clippy exit 0 (not rerun) |
| BASE-32 | pass | D214–D219 implemented as written; D8 kept: the notice is dismissed by mouse, so no key is taken from programs |
| New-states | pass | round-1 table covers pass_nul, capture, reveal, notice click |
| AC-14a | pass | legacy and kitty can show, focus and hide by key on the build |
