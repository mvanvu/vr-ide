
## QC T1 — Session store + --session (D1–D3)
Tested tree: worktree .worktrees/T1, HEAD 0470b1be5b39f4d8a6811b109ccdf03067ee308b + uncommitted diff (9 files, +78/-6, plus new session.rs, session_tests.rs; `git diff HEAD | md5sum` cbc30894705cf26e153ddeb0ba7091c9). Real binary `cargo build --release -p vr-ide` (run-heavy, ok), driven in a pty (engine: python pty + vt screen, 120x40), temp HOME/XDG only. Scripts: qa-artifacts/T1/{h,walk2,walk3,e4,e5}.py. Developer tests not rerun.

| Flow | Result |
|---|---|
| AC-1 `--session` first run: only temp `~` root; session.toml roots == [home] | PASS |
| AC-2 /ws add a,b; 3 files; cursor/scroll (Ln45 Col7, top 37); edit mode; explorer expanded; terminal right + resized (45%); quit; relaunch restores roots, tabs, cursor, scroll, expanded, terminal right+shown | PASS |
| AC-2 file edited on disk shows new content; deleted file tab skipped with S6 notice `Session restored; 1 tab skipped, files not found: a/f0.txt` | PASS |
| SIGKILL 3 s after change -> relaunch restores it (Ln 10, root, tab) | PASS |
| AC-3 `vr-ide` and `vr-ide <folder>`: no session.toml/state dir created; existing session.toml byte- and mtime-identical | PASS |
| AC-4 session.toml 0600, dir 0700, .bad 0600 | PASS |
| AC-4 garbage file: one notice `Session file unreadable (line 2); opened ~/, kept it as session.toml.bad`, .bad kept, home opened, no crash | PASS |
| AC-4 second `--session` window: notice "...not saved", did not write; first window still saves | PASS |
| AC-11 spot: `vr-ide <file>`, `vr-ide dir1 dir2`, bad path exit 2, `--session x` usage exit 2 | PASS |

Observation (non-blocking, low): edit mode is lost in the quit-time save when a terminal shell is running. ctrl+q then shows the "still running" dialog, which returns the tab to read mode, and save_on_quit writes `edit = false`, overwriting the debounced `edit = true`. Repro: --session, open file, show terminal (ctrl+`), click editor, `space` `i`, ctrl+q, choose Quit anyway, relaunch -> READ. Without a running terminal edit mode restores (EDIT). Spec says edit mode restored; consider snapshotting before the dialog or skipping the overwrite.
Harness notes: first F1 slash right after startup was sometimes dropped (harness timing, retried). Temp dirs removed; no leftover processes.

## Rule check T1
| Rule | Status | Evidence |
|---|---|---|
| AC-1 | pass | walk2 AC-1 |
| AC-2 | pass (edit-mode caveat above) | walk2, e4 |
| AC-3 | pass | walk3 |
| AC-4 | pass | walk3 |
| AC-11 | pass | e5 |
| S6 notices | pass | exact text observed |
| SEC-6 (perms) | pass | 0600/0700 observed |

## QC T3 — /ssh argument line (D5) — QC_PASS
Tested tree: worktree .worktrees/T3, HEAD 0470b1be5b39f4d8a6811b109ccdf03067ee308b + uncommitted diff (sha1 of `git diff HEAD` 0f4580b0..., plus untracked ssh/args.rs, args_tests.rs). Real binary `cargo build --release -p vr-ide`, driven in a pty, fake ssh shim recording argv/cwd/PATH, temp HOME/state, symlinked workspace launch.
Engine: pty + vt screen model (terminal app, no browser). Script: qa-artifacts/T3/t3_walk.py; result qa-artifacts/T3/t3_walk_result.txt (39/39 PASS in the final run).
- AC-6 PASS: `-i credentials/id_fixecom root@app.fixecom.vn` argv = `-o ControlMaster=no -o PermitLocalCommand=yes -o LocalCommand=... ` + exactly the user words; cwd = canonical root (launched via symlink); -G probe user words only; `-p 2222 user@host`, `-J jump host` (label host), quoted arg with space, backslash escape each one argv word.
- SEC-1 PASS: unclosed ' / " / trailing backslash / >4 KiB -> fixed notice, nothing spawned. Pasted control char (BEL) is turned into a space by paste sanitising, so it never reaches argv (control-char refusal itself covered by unit tests).
- SEC-2 PASS: `-o Foo=bar host -v` -> status row/tab `host`; `-- -evil`, `-evil`, `-- -oProxyCommand=x` refused, nothing spawned.
- SEC-3 PASS: `-M`, `-S`, `-o LocalCommand=`, `-o ControlMaster=` -> prefix dropped, argv = user words. Note: `-o ProxyCommand=` keeps the prefix, which matches the threat model SEC-3 word list (the brief's wording differs).
- SEC-4 PASS: root removed -> refusal, no -G/ssh run; child PATH has no empty/relative entries, trap ./ssh never ran.
- SEC-5 PASS: ProxyCommand recent row -> input filled + hint, nothing spawned; Enter runs in stored folder; file 0600; 0660/0666 file ignored; old `bob@old:2200` listed/run as `-p 2200 bob@old`.
- FR-25 regression PASS: session tab, full screen after login, status `SSH  bob@web1`, exit text, close and reopen from recent.
Observations (not defects): state dir was 0775 in the walk because vr-config creates it before ssh-recent (umask 002); the file itself is 0600. Under heavy host load (load avg ~40) SEC2/AC6 timing flows failed spuriously (F1 palette settle); they passed on re-run at lower load (environment).

## Rule check T3
| Rule | Status | Evidence |
|---|---|---|
| AC-6 | pass | AC6-01..06 |
| SEC-1 | pass | SEC1 flows |
| SEC-2 | pass | SEC2-01..04 |
| SEC-3 | pass | SEC3 flows |
| SEC-4 | pass | AC6-02d, SEC4 |
| SEC-5 | pass | SEC5-01..09 |
| FR-25 regression | pass | FR25-01..04 |
| BASE-14/15/30 | pass | background runs, no kill; temp dirs removed, no processes left |
| COST | pass | one build, one walk script |

### QC T1 re-check (after edit-mode and state-dir fixes)
Tree: HEAD 0470b1be + uncommitted diff (md5 bb9bd824a369c7e7f5f7cfb6c0b1a23e), release rebuilt, temp HOME. Script: qa-artifacts/T1/recheck.py, e6.py.
- (1) edit mode on quit with a running terminal: FAIL. Repro: --session, /ws add, open file, ctrl+` , click editor, space i (EDIT), wait 2.5 s (file has `edit = true`), ctrl+q -> "1 terminal still running" dialog -> Quit anyway -> session.toml has `edit = false`, relaunch shows READ. The unit test passes (it sets app.quit=true with the dialog state) but the real flow differs: probably the dialog closes (editor back in read mode, dialog None) and a tick runs `remember_edit_mode` with edit=false before `app.quit` is set / before `save_on_quit`, overwriting `edit_seen`. The mode seen before the dialog opened must be kept.
- (2) pre-existing state dir 0775: PASS (becomes 0700, session saves, session.toml 0600, no "not saved" notice).
- (3) AC-2 regression: PASS for roots, tabs, skipped-file notice, terminal right restored, cursor Ln 45 Col 7; edit mode of the active tab fails for the same reason as (1) (a terminal is running in that flow).
Verdict: QC_FAIL T1 (edit mode only).

## QC T4 — Palette Tab completion (D6, AC-7)
Tested: worktree /home/vu-dev/DEV/vr-ide/.worktrees/T4, HEAD 0470b1be5b39f4d8a6811b109ccdf03067ee308b + uncommitted diff (8 files modified; completion.rs, completion_tests.rs, snapshots untracked). Release binary built via run-heavy (exit 0). Engine: real binary in a pty at 80x24 and 120x40, temp HOME/XDG/VR_IDE_* dirs outside any git repo; temp data removed.
Harness: qa-artifacts/T4/walk.py; results res_24x80.txt, res_40x120.txt, res_bigd.txt; screens_*.txt. Machine load was 20-40 on 16 cores, so the harness settles on output quiet.
| Flow | 80x24 | 120x40 |
|---|---|---|
| single match completes | PASS | PASS |
| ambiguous: prefix, 2nd Tab list, Tab/Shift+Tab step | PASS | PASS |
| Esc closes list only | PASS | PASS |
| Enter takes candidate, does not run | PASS | PASS |
| /ws add ~/DE path; hidden only after '.' | PASS | PASS |
| 1500 files / 1500 dirs responsive, cap 100 | PASS | PASS |
| /ssh -i cred relative to active root | PASS | PASS |
| /ssh hosts from temp ~/.ssh/config, runs: line | PASS | PASS |
| /ai c -> claude, codex | PASS | PASS |
| regression: palette keys, F1 from terminal, ctrl+f | PASS | PASS |
Note: ws_big_dir FAIL line is the harness wall-time assert only (input correct); ws_big_dirs_responsive PASS (first frame < 3s).
Observation: /ssh git + Tab gives git.example.com with no runs: line (it shows in list state).
Verdict: QC_PASS T4

## Rule check T4
| Rule | Status | Evidence |
|---|---|---|
| AC-7 | pass | flows above |
| D6 | pass | commands, paths, hidden, ssh, ai |
| D12 | pass | screens_*.txt inspected |
| SEC-4 | pass | root-relative, bounded listing |
| BASE-14 | pass | background build/walk |
| BASE-15 | pass | own processes only |
| BASE-17 | n/a | no data committed |
| BASE-19 | pass | per-flow results |
| BASE-30 | pass | temp removed |

## QC T5
Tested commit: c8675ed8faa00313c13755a3b7e48154f6b70f96 (run4-T5). Release build ok. Engine: real binary on a pty (120x40, 80x24, 40x12), temp HOME/XDG, fake `claude`/`codex`/`gemini` shims on a temp PATH (coreutils-only sysbin, no real tools). Scripts: qa-artifacts/T5/{h.py,qc_walk.py,q2.py,q3.py,q4.py}; small-size screens: sz80.txt, sz40.txt.
Verdict: QC_PASS T5.
- AC-8 (all PASS): picker lists shim tools and "Not on PATH: aider, cursor-agent"; 3x claude + 1 codex labels `1 claude`, `2 claude·2`, `3 claude·3`, `4 codex`; panel full height; f12 hides and shim pids stay alive, f12 shows; f7 / shift+f7 cycle; f3 focuses editor; ctrl+f8/f9 change width; terminal(right) drawn under the editor while the panel shows, back at the right when hidden; ctrl+f7 gives the notice; `/ai claude` opens a session in the panel; AI: Rename Session and AI: Close Session (asks while running; Close ends the shim); a shim that exits shows `[exited 3]`, enter restarts with `--continue`.
- AC-13 (PASS): claude `--continue` / `--resume` / none; codex `resume --last` / `resume`; gemini (no resume args) skips the step.
- SEC-8 (PASS): cwd is the canonical root when the file is opened through a symlink; no VR_IDE_* in env; PATH has only absolute entries (the `.` entry of the parent is dropped); project-local `./claude` never ran; `./claude` and `claude; touch PWNED` entries in ai_tools are listed as unavailable, `/ai semi` and `/ai rel` start nothing, no PWNED file.
- Keys (PASS): in a focused session ctrl+w, ctrl+n, f5 reach the shim (bytes 17, 0e, 1b..7e) and open/close nothing; f3, ctrl+` (NUL) and f12 leave it.
- Small sizes 80x24 and 40x12: no crash, two sessions start, panel and tab bar drawn (40x12 shows the narrow `A`/short status).
- Regression spot (PASS): editor typing, terminal bottom, palette.
- Notes (not defects): a cold-start ctrl+f12 sent within about 2 s of launch was once not shown in my harness (passed with a 3.5 s startup wait); the picker remembers the last tool row (harness resets with up keys); the ai close dialog defaults to Cancel (Left then Enter closes).
- Cleanup: temp dirs removed, all my vr-ide/shim processes ended.

## Rule check T5
| Rule | Status | Evidence |
|---|---|---|
| AC-8 | pass | flows above |
| AC-13 | pass | argv per tool/mode |
| SEC-8 | pass | cwd/env/PATH/hostile ai_tools |
| BASE-14 | pass | build in background, no waiting loops |
| BASE-15 | pass | killed only a stray gemini my own harness started (real gemini on /usr/bin from an early harness PATH); fixed with a coreutils-only PATH |
| BASE-17 | n/a | no data committed |
| BASE-19 | pass | per-flow results |
| BASE-30 | pass | temp dirs removed |

### QC T1 re-check 2 (edit mode on the real quit path)
Release rebuilt (cached, up to date), temp HOME. Scripts: qa-artifacts/T1/e6.py, recheck.py, e7.py.
- e6 (running terminal, ctrl+q -> Quit anyway): session.toml `edit = true` after quit: PASS.
- recheck.py: (1) edit on quit with terminal -> relaunch EDIT PASS; (2) state dir 0775 -> 0700, session saves, 0600, no "not saved" PASS; (3) AC-2 regression (roots, tabs, notice, terminal right, EDIT + Ln45 Col7) PASS.
- e7 (unsaved edit "zz", ctrl+q -> Discard): file on disk unchanged, session.toml `edit = true`, relaunch shows EDIT: PASS.
Verdict: QC_PASS T1.

### QC T4 re-check (quick pass 1)
Rebuilt release binary (run-heavy, exit 0), temp HOME. `/ssh git` + Tab: input `/ssh git.example.com`, `runs: ssh git.example.com` shown at 80x24 and 40x12 (PASS). 40x12 `/ws add ../x` + Tab: input ends `../xray/`, the `/` visible (PASS). Results: qa-artifacts/T4/res_recheck.txt.
Verdict: QC_PASS T4

## QC T2
Tree: worktree /home/vu-dev/DEV/vr-ide/.worktrees/T2, HEAD ddd9dc50bf5d05ef943af563af32a5483515e0dd + uncommitted diff (cli.rs, main.rs, untracked crates/vr-ide/tests/); diff md5 92ba9a4d. Release build OK. Engine: shell, desktop-file-validate, GLib DesktopAppInfo, pty via script. Temp HOME/XDG_DATA_HOME only, removed after.
- install: .desktop + svg written; validate 0; Exec gnome-terminal --title=vr-ide -- "<abs exe>" --session; TryExec; X-VrIde-Generated marker. PASS
- exe path with space and $: validate 0; GLib parses argv back exactly. PASS
- pty run of '<exe> --session' with temp HOME: TUI opens, shows temp HOME. PASS
- re-install idempotent (same md5). PASS; uninstall removes marked files only, second run "nothing to remove". PASS
- symlink in place: uninstall leaves it; install replaces the link, targets untouched. PASS
- extra args (any order, also --uninstall-desktop extra): usage, exit 2, nothing written. PASS
- DEFECT (low/medium): an existing unmarked hand-made vr-ide.desktop and vr-ide.svg are silently overwritten by --install-desktop (rc 0, "wrote ..."). Uninstall correctly leaves unmarked files. Expected: refuse or warn.
- Icon render: rsvg-convert/inkscape absent, not rendered; SVG source reviewed only (128px rounded square, prompt chevron, sparkle).

## Rule check T2
| Rule | Result |
|---|---|
| AC-5 launcher entry + icon + gnome-terminal Exec | pass |
| SEC-7 quoting / no symlink follow / marker-only removal | pass |
| SEC-7 no silent overwrite of foreign file | FAIL (unmarked overwritten) |
| Icon sizes 16/32/48 visual | not verified (no renderer) |

## QC T7 — QC_PASS
Tree: worktree .worktrees/T7, HEAD c8675ed8faa00313c13755a3b7e48154f6b70f96 + uncommitted diff sha256 d12f544f4ce57ee9a8544f917b92d9ddfaf07f4c9874b379b48a45abd1eb0f11. Engine: release binary in a pty (qa-artifacts/T7/walk7.py, f3b.py, gitflow.py; outputs out120/out80/out80L.txt), fake claude shims on coreutils-only PATH, temp HOME/XDG.
- 120x40 and 80x24 (+ 80x24 long file path): viewed session -> no mark/notice (BEL, OSC 9, OSC 777); panel hidden -> notice (wide at 120, narrow `◆ claude: Need input f12` at 80) + badge ` ◆ claude  f12 `, status text reflowed (not cut mid-text); other tab active -> ◆ on tab + notice; viewing clears mark, notice, badge.
- 3 sessions, 1 and 2 marked, 3 active: f12 (panel focused) hides with badge `◆ claude +1`; next f12 opens oldest (1/3), then notice for 2 follows (matches ui-spec: f12 shows on oldest marked when hidden).
- OSC 9;4 progress and DCS-with-BEL: no mark; terminal-panel tab bell/OSC 9: no mark.
- SEC-14: ESC[2J inside OSC -> no clear (0 CSI 2J emitted), text `rang the bell`; 10 KiB OSC 9 -> cut with … before f12, one row, label-prefixed; 100 bells -> one notice; OSC 777 -> `Title: Body text`.
- Not black-box checked: one notice per second per session (needs two marks within 1 s; covered by dev unit test). Git branch is not shown in the status bar in this build, so no branch/status interaction to see.
- Harness false FAILs in out80L F5 (my check looked for a.txt); the screens show the correct result.
## Rule check T7
| Rule | Status | Evidence |
|---|---|---|
| AC-10 / D9 | pass | out120/out80/out80L F1-F3, f3b |
| ui-spec S4 forms, badge reflow | pass | 120 wide, 80 narrow, 80 long path |
| SEC-14 | pass | F5 flows; rate limit by unit test only |
| Progress/DCS/terminal no mark | pass | F4 |
| BASE-14/15/30 | pass | one background build/run, own pids only, temp dir removed |

### QC T2 re-check (overwrite fix)
Tree: HEAD ddd9dc50 + uncommitted diff (cli.rs, desktop.rs, desktop_tests.rs, main.rs, tests/). Rebuilt release; temp HOME/XDG only, removed.
- unmarked .desktop + icon: install exit 1 ("exists and was not written by vr-ide; remove it first"), both untouched. PASS
- unmarked .desktop only: exit 1, icon not created. Unmarked icon only: exit 1, no .desktop written, icon untouched. PASS
- symlinks in place: exit 1, links and targets untouched. PASS
- clean install rc 0, validate 0, re-install idempotent (same md5), uninstall removes both. PASS
Verdict: QC_PASS T2 (icon 16/32/48 render still unverified, no renderer).

### QC T5 re-check (D15: restore through --session, exited line)
Tested commit: b5faf214293d7ae626c1701c298ae917dca412dc. Release rebuilt; same harness (coreutils-only PATH, temp HOME), script qa-artifacts/T5/q5.py.
- (1) `vr-ide --session`, 2 claude + 1 codex, session renamed (`mine`, the codex one), width changed with ctrl+f8, Quit anyway, relaunch `--session`: PASS. Panel shown, same separator column, tabs `1 claude`, `2 claude·2`, `3 mine`; shims restarted with argv `--continue` (claude x2) and `resume --last` (codex); no `GOT:` bytes, so no input sent. (The script's "codex in tab name" assertion was my error: the renamed tab is the codex one; the name `mine` was kept.)
- (2) codex shim removed before relaunch: PASS. Only the two claude tabs reopen, one notice "Not reopened: codex in ~, codex not on PATH  (Esc)", codex started once only (original run).
- (3) shim prints 50 lines and exits 4: PASS. `[exited 4] enter: start again (claude --continue)` is drawn below the output, `outline 50` still visible above it; shift+pageup scrolls back (`outline 1 ... ↑ 15/15`).
- (4) console `vr-ide` with no `--session` on the saved state: PASS. No panel, no shim started (count stayed at 6).
Verdict: QC_PASS T5 re-check. Temp dirs removed, my processes ended.

### QC T3 re-check (round 2) — QC_PASS
Tree: .worktrees/T3 HEAD 0470b1be + uncommitted diff (sha1 ee3d23b0...); release binary rebuilt. Scripts: qa-artifacts/T3/t3_r2.py (9/9) and t3_walk.py full rerun (39/39); results t3_r2_result.txt, t3_walk_result.txt.
- (1) PASS: line run with root A active -> cwd A; same line typed by hand with b.txt (root B) active -> cwd B; recent row picked (stored folder A, B active) shows `in <A>` and runs in A.
- (2) PASS: recent `-vI /x.so h1` and `-F cfg h2` only fill the input with the hint (nothing spawned); `-F none h3` runs at once.
- (3) PASS: at 80x24 the long line's `runs:` area wraps and cuts with `…` while keeping `root@app.fixecom.vn`.
- (4) PASS: full regression walk 39/39.
Temp dirs removed, no processes left.

## QC T8 — Accounts per agent session (D11)
Tested commit: 587f4042b8f27a412e3311eebeda0911ed8e0b8b (worktree .worktrees/T8, branch run4-T8). Engine: release binary on a pty (vt screen model), 120x40 and 80x24, fake claude/codex/gemini shims, coreutils-only PATH, temp HOME/VR_IDE_STATE_DIR. Scripts: qa-artifacts/T8/{w8,w9,w10,w11}.py.
Verdict: QC_PASS T8.

| Flow | Result |
|---|---|
| AC-12 picker groups (claude/codex/gemini, default, `+ Add account…`) | pass |
| add `work` (claude): CLAUDE_CONFIG_DIR=<state>/accounts/work, folder 0700, notice "sign in with /login in its tab", tab `claude@work` | pass |
| add `alt`; two claude sessions side by side with own dirs, labels @work/@alt (120 cols full; at 80 cols inactive tab shortens to `@work`) | pass |
| default claude: no CLAUDE_CONFIG_DIR; codex account -> CODEX_HOME only; gemini has no Add row, starts with no vars; terminal tab has none | pass |
| Delete on in-use `alt`: refused, "Not removed: close the session on alt first, tab 2" | pass |
| close tab via palette -> Remove dialog (Cancel default; "for good", sign-in stays valid, /logout); Enter keeps folder; Remove deletes only accounts/alt + index line; work and other state untouched | pass |
| names refused: Work, ../x, default, Default, 33 x a, a/b, "a b", empty, ., ..; nothing created, no session; 32 chars accepted | pass |
| `--session` restore: same accounts (work, alt, codex cxa) | pass |
| alt folder deleted before relaunch: tab not reopened, "Not reopened: claude@alt, account folder missing (no fallback)", no default fallback | pass |
| 0755 account folder -> tightened to 0700 on next use | pass |
| account folder replaced by symlink: not listed in picker; restore refuses ("not a real folder (no fallback)"), target untouched; `accounts/` itself a symlink -> nothing started | pass |

Observations (not defects): index write is atomic via .ai-accounts.*.tmp; a kill -9 landing mid-write leaves the tmp file (seen once, only when the harness killed vr-ide right after Remove; normal run renames it). At 80x24 inactive tab label is `@work`.
Cleanup: temp dirs removed, no processes left, nothing under real ~/.local/state, ~/.claude, ~/.codex touched.

## Rule check T8
| Rule | Status | Evidence |
|---|---|---|
| AC-12 | pass | flows above |
| SEC-15 (name validation) | pass | 10 refused names, 32-char accepted |
| SEC-16 (no reading inside account folders; env only per spawn) | pass | env per tab verified; default/terminal/gemini unset |
| SEC-17 (0700, fd checks, symlink refusal) | pass | 0755 -> 0700; symlinked folder and accounts/ refused, target unchanged |
| SEC-18 (remove only its own folder, confirm) | pass | tree diff = accounts/alt + index only; Cancel default |
| D11/D12/D13/D16 | pass | notices, labels, no fallback, dialog text |

### QC T8 re-check (round 1 changes; release rebuilt, harness qa-artifacts/T8/w12.py, run twice, both all-pass)
| # | Check | Result |
|---|---|---|
| 1 | 80x24 footer with a named account reads whole pairs `enter start  del remove` | pass |
| 2 | Two windows on one state: A runs claude@work; in B, Delete opens the Remove dialog and confirming gives "Not removed: work, in use in another window" (folder intact); after A closes the tab B removes it | pass (refusal comes at confirm, not at the Delete key) |
| 3 | `work` line removed from ai-accounts, folder kept: `--session` restore refuses "Not reopened: claude@work, account not recorded; add it again (no fallback)", nothing started | pass |
| 4 | ai-accounts invalid UTF-8: add refused ("cannot read the account record"), file bytes unchanged, no folder, no session | pass |
| 5 | index mode 0600 | pass |
| 6 | Remove of a folder holding a dir symlink and a file symlink to outside: folder gone, outside dir and files intact | pass |
Notes: an `accounts/.<name>.lock` file now sits beside each account folder; the index write can lag the session start by a moment (a harness write made too early was overwritten once, not an app defect).
Verdict: QC_PASS T8 (re-check).
