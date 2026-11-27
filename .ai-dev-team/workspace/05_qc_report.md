
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

## QC T6 — 2026-11-27
Tested: worktree .worktrees/T6, HEAD c35e6fa2cd21 + uncommitted fix round 1 (`git diff HEAD | sha256sum` 98cabd15e63a90374057843d821a4f8a6ce5f5d282991c5af371eb227d035d29). Release binary built once via run-heavy (ok). Engine: real binary in a pty with a vt screen model (terminal UI, no browser), sizes 120x40 and 80x24, themes dracula and synthwave (there is no "homebrew" theme; Homebrew is the terminal-panel colour in every theme). Fake `claude` agent shim (shell, blocks on a fifo and runs scripts that write/create/delete/swap files), coreutils-only PATH, temp HOME / XDG_DATA_HOME (trash) / VR_IDE_STATE_DIR / VR_IDE_CONFIG_DIR. Developer tests not rerun. One flow script: qa-artifacts/T6/qc/flows.py (lib.py, h.py, shim/, sysbin/); last full run `final.txt` (68 lines, 67 PASS, 1 harness-check FAIL explained below); screens in `screens.txt`.

| Flow | Result |
|---|---|
| AC-9 list (header `Changes · claude · 3 files`, M/A/D rows, counts, D8 statement, ±3 badge) at 120x40 and 80x24, dracula and synthwave | pass |
| AC-9 diff modified (`@@ -1,3 +1,4 @@`, `+changed`), created (all +), deleted (all -), four size/theme combos | pass |
| AC-9 Revert dialog texts: deleted ("Restores del.txt (2 lines) that claude deleted"), created ("claude created this file. Revert moves it to the trash") | pass |
| AC-9 Enter on dialog = Cancel, file untouched | pass |
| AC-9 Revert modified: bytes exactly the pre-agent bytes, notice `Reverted a.txt` | pass |
| AC-9 Keep: row leaves, file keeps agent content, notice `Kept b.txt` | pass |
| AC-9 Revert created: file in `$XDG_DATA_HOME/Trash/files/new.txt`, gone from folder | pass |
| AC-9 Revert deleted: restored byte-exact; empty list shows `No changes since it started` | pass |
| SEC-9 agent resets mtime to 2000 after rewrite: Revert gives true bytes | pass |
| SEC-9 file vr-ide never read (ignored dir): row/diff "no earlier version kept", `revert is off`, shift+f5 -> `No earlier version of x.txt kept; Keep only`, nothing written | pass |
| SEC-9 write right after own save (4 trials, `>>` and `>`): Revert never leaves agent bytes; it restored the pre-save file (see observation) | pass |
| SEC-10 store base 0700, store dir 0700, index/lock/blobs 0600 | pass |
| SEC-10 9 MiB file: row `too large`, no big blob | pass |
| SEC-10 store cleared on session close (blobs empty) and store folder removed on quit | pass |
| SEC-10 after Keep/Revert of all: harness FAIL line only — remaining 5 blobs (5,21,24,6,4 bytes) are baselines of current files (the 4-byte one is `CHG`, b.txt as Kept); they go on session close/quit | pass (by design) |
| SEC-11 symlink to outside file and to outside folder, FIFO pre-existing and agent-made, file replaced by link: UI responsive, no stored data holds outside / link-target content, files below a linked folder not listed, Revert did not write through, outside files untouched | pass |
| SEC-12 Keep after change since shown -> `Not kept: a.txt changed since shown; diff updated`, diff reloaded | pass |
| SEC-12 Revert after change since dialog -> `Not reverted: a.txt changed since shown; diff updated`, file keeps agent's newer bytes | pass |
| SEC-12 created file changed after shown: stays, trash empty (`Not reverted: new.txt changed since shown`) | pass |
| SEC-12 folder swapped in at deleted file's path: listed as deleted, Revert -> `Not reverted: a folder is now at del.txt`, folder and inner file intact | pass |
| SEC-12 parent folder swapped for symlink to outside after dialog: `Not reverted: the folder of c.txt is no longer inside the session's folder`, outside untouched | pass |
| SEC-12 Revert while a writer rewrites a.txt (60 appends): `Not reverted ... changed since shown`, no torn lines, UI responsive | pass |
| SEC-13 150 005 files / 3 000 dirs: banner `! List may be incomplete (still scanning the folder)` while walking, gone after the scan | pass |
| SEC-13 12 005 dirs (over the 10 000 watch cap): banner `List may be incomplete (watch limit at HH:MM) · enter hides` | pass |
| USER-2 (real binary driven, screens in terminal text) / USER-5 (f5, shift+f5, f6 only, no fallbacks; keys legacy-terminal safe) | pass |

Observations (not defects):
- Warning notices (`Not kept`, `Not reverted`) stay until clicked and queue (`+N more · click`); a newer refusal can sit behind an older one until the user clicks. Harness clicks them away between steps.
- Own save then immediate agent write: Revert restores the file as it was before the user's own save (the user's saved edit is attributed to the agent, as the dialog's "edits by other programs count as claude's" says); the agent's bytes are never written.
- Early harness runs: ctrl+q while the AI panel has focus goes to the agent; the harness now sends f3 first. Early "store not removed after quit" and "scan banner" FAILs were harness bugs (quit path, missing grep in sysbin, picker needing three Enters), fixed in the script.
- Cleanup: no vr-ide / shim processes left, ctl fifo and big fixture removed.

## Rule check T6
| Rule | Result | Evidence |
|---|---|---|
| AC-9 | pass | list/diff/Keep/Revert/created-to-trash/deleted-restore/refusals, final.txt |
| SEC-9 | pass | mtime reset, never-read "no earlier", own-save trials |
| SEC-10 | pass | modes 0700/0600, too large, cleared on close and quit |
| SEC-11 | pass | links, FIFOs, linked folder |
| SEC-12 | pass | stale Keep/Revert, folder swap, parent swap, writer race, trash |
| SEC-13 | pass | still scanning and watch limit banners |
| USER-2 | pass | terminal render |
| USER-5 | pass | f5 / shift+f5 / f6 per key table |
| BASE-14/15 | pass | long runs in background, only own processes |
| BASE-17 | n/a | no data committed |
| BASE-19 | pass | per-flow lines in final.txt |

QC_PASS T6

## QC T10 — 2026-11-27
Engine: real release binary (worktree .worktrees/T10, built once) in two PTY windows, temp HOME/state/config, fake agent shim only. Script `qa-artifacts/T10/qc/flows.py`, result `qa-artifacts/T10/qc/result.txt` (17/17 PASS).

| Flow | Result |
|---|---|
| AC-3 second `--session` window while first runs (agent tab running) | PASS, notice "Another vr-ide --session window keeps the session; this one is not saved"; still refused after that second window is killed |
| AC-1 first window quits with agent running: session saved | PASS (session.toml 0600) |
| AC-4 new `--session` window right after quit | PASS, no notice; AC-2 restore reopens the agent tab |
| AC-4 after first window SIGKILLed | PASS, allowed |
| AC-4 5x quit-with-agent then immediate restart | PASS, 0 refusals |
| SEC-6 state dir 0700, session.toml 0600, session.lock 0600 | PASS |
| D17 remove account while window A has a tab on it | PASS, "Not removed: work, in use in another window", folder kept |
| D17 remove after A quits | PASS, account folder gone |
| Flake gone: load loop `loop2.sh` x3 parallel x10 on a fresh test binary of the fixed tree (30 runs of session/ai_tests/accounts) | 1/30 failed: `ai_tests::a_focused_session_gives_other_keys_to_the_agent` (PTY wait timeout, ~10 s; developer notes it flakes on HEAD too, not a lock). Target panel test and account test: 0 failures. Logs `qc/loop-qc-{1,2,3}.log` |

## Rule check T10
| Rule | Status | Evidence |
|---|---|---|
| AC-1..4 | pass | flows above |
| SEC-6 | pass | modes 0700/0600/0600 observed |
| D17 | pass | refused while running, allowed after quit |
| Flake fixed | pass | 0 lock-related failures in 30 loaded runs |
| Out-of-scope flake | note | a_focused_session_gives_other_keys_to_the_agent, 1/30, separate task suggested |
| BASE-14/15/17/19 | pass | background tasks, no kills by name, no DB probes, results written as checked |

## QC T6 re-check r2 — 2026-11-27

Build: `.worktrees/T6r2` release (`run-heavy.sh cargo build --release -p vr-ide`, exit 0, 4m08). Harness: `qa-artifacts/T6/qc/flows_r2.py` (extends flows.py; `h.py` BIN via `VRBIN`, default T6r2; shim `claude` ends after a script whose first line is `#EXIT`). Engine: pty + VT model, fake agent shim only. Output files in `qa-artifacts/T6/qc/`: `screens.txt`, `r2_gap2.txt`, `r2_regress.txt`. Sizes 80x24 and 120x40, themes dracula and synthwave.

| Flow | Result | Evidence |
|---|---|---|
| a. exit before the walk reaches the folder (SEC-13) | pass, 4/4 combos | `! Scan stopped early: changes may be missing · enter hides`; Enter hides. At 80x24 the text is whole over two rows (`! Scan stopped early: changes` / `may be missing · enter hides`), screens.txt lines 350-351, 376-377. The harness substring check FAILs there only because the neighbouring SYSTEM column is interleaved in the flattened text (2 lines of r2 output: 80x24 dracula and synthwave); eyeballed from the screen dump |
| b. deletion during the scan, folder walked late (SEC-13) | pass, 4/4 combos | `! Deletions during the scan may be missing · enter hides` at 120x40 (machine-checked, kept after the walk ended, Enter hides). 80x24: two rows `! Deletions during the scan` / `may be missing · enter hides`, whole; the same interleaving makes the substring check FAIL (3 lines in r2_gap2.txt), Enter-hides passes |
| c. Revert modified: trash (SEC-12 / D18 / D20) | pass, 4/4 combos, 16 checks | dialog `Today's text, +1 -0, goes to the trash.` whole at both sizes; file = earlier version; Trash/files holds exactly one entry with the current text (named `a.txt.vr-ide-reverted-PID-N`, content = text before Revert); no stray file in the workspace; notice `Reverted a.txt` |
| d. own save, then agent writes at once (SEC-9 / D21) | pass, 6/6 trials (strict check) | every trial: row `no earlier version kept`, Shift+F5 gives `No earlier version of a.txt kept; Keep only`; file never became the pre-save text (round 1 trial criterion was weaker; this one requires after != original and no-earlier row or user's save) |
| e. two refusals in a row | pass | stale Keep shows `Not kept: a.txt changed since shown…`; then stale Revert on b.txt shows only `Not reverted: b.txt changed since shown…`, `Not kept` gone |
| f. regression, round-1 flow set on the new binary | 67 / 68 pass | the 1 FAIL is the same SEC-10 harness line as in round 1 (5 baseline blobs of current files, 4-byte one is Kept b.txt; judged pass by design in round 1) |

Observation (not a defect): the trash entry carries the name `<file>.vr-ide-reverted-PID-N`, not `<file>`, so the user sees that name in the Trash (D18 design). The dialog no longer says "is lost" (also dev note 3 of the round is resolved: text now matches D18).

## Rule check T6
| Rule | Verdict | Evidence |
|---|---|---|
| SEC-13 incomplete list marked | pass | flows a, b |
| SEC-12 / D18 / D20 nothing lost on Revert | pass | flow c (current text in Trash/files, no stray file); flow e |
| SEC-9 / D21 never undo own save | pass | flow d, 6/6 |
| AC-9 no regression | pass | flow f, 67/68 with the one known by-design harness line |

QC_PASS

## QC T10 re-check r1 — 2026-11-27
Fresh release build of the uncommitted .worktrees/T10 tree (one run-heavy build); same PTY harness; no load loop. Script `qa-artifacts/T10/qc/flows.py`, result `qa-artifacts/T10/qc/result-r1.txt`: 17/17 PASS.

| Flow | Result |
|---|---|
| AC-3 second `--session` window refused while first runs (agent tab) | PASS, notice shown; still refused after refused window killed |
| AC-1 quit with agent running saves session (0600) | PASS |
| AC-4 new window right after quit: no notice; AC-2 agent tab restored | PASS |
| AC-4 after first window SIGKILLed | PASS |
| AC-4 5 cycles quit-with-agent then immediate restart | PASS, 0 refusals |
| SEC-6 state dir 0700, session.toml 0600, session.lock 0600 | PASS |
| D17 remove refused while window A has a tab on the account; allowed after A quits | PASS (folder kept, then gone) |
| Quit time, ctrl+q to process exit with an agent tab running (`qtime.py`, 5 runs each, `qtime.txt`) | before (binary of the first fix) 0.02 s median; after r1 0.02 s median (max 0.04). No added ~1 s. |

## Rule check T10 re-check r1
| Rule | Status | Evidence |
|---|---|---|
| AC-1..4 | pass | flows above |
| SEC-6 | pass | 0700/0600/0600 |
| D17 | pass | refused then allowed |
| Quit latency | pass | 0.02 s vs 0.02 s |
| BASE-14/15/19 | pass | build in background, no name kills, results written as checked |
Note: the earlier "10 s"/"5 s" timing readings were a harness mistake (ctrl+q sent while the agent panel had focus; F3 needed first), not product behaviour.

## QC T6 re-check r3 — 2026-11-27

Build: `.worktrees/T6r3` release (run-heavy, exit 0). Harness: `qa-artifacts/T6/qc/flows_r3.py` (+ flows_r2.py, flows.py, h.py via `VRBIN`); banner checks read the Changes panel column only (right of the border before the `Changes ·` header), so the 80x24 wrap no longer gives false FAILs. Output: `r3_all.txt` (122/129 pass), debug probes `dbg1..5.py`. Fake agent only; 80x24 / 120x40; dracula / synthwave.

| Flow | Result |
|---|---|
| 1. queued folders removed + moved before the walk reaches them | banner `! Incomplete: files deleted during scan · enter hides` shown, whole, no leading `·`, last panel cell free, kept after the walk: pass 4/4 combos. **Enter does not hide it: FAIL 4/4 (defect, below)** |
| 2. exit mid-walk | `! Incomplete: scan stopped early · enter hides` whole at 80x24 (`… scan stopped` / `early · enter hides`) and 120x40; later folder removals do not replace it; Enter hides: pass 4/4 (the late-walk part is weakly exercised: after the exit I could not prove the walk produced a late note) |
| 3. save, agent writes, second quick ctrl+s (ChangedOnDisk dialog seen in trials 0, 3) | pass 6/6: row `no earlier version`, Revert answers `No earlier version of a.txt kept; Keep only`, file never the pre-save text |
| 4. banner rows | whole + `enter hides` + no leading `·` + last cell free: pass for flow 1 and 2 notices at both sizes. Scanning notice (no hint by design): rows `! List may be incomplete (still scanning the` / `  folder)` whole; the 2 FAILs are my check also reading the two rows below the banner (harness artifact) |
| 5. `notes.vr-ide-reverted-x` created by the agent | pass: listed as `A notes.vr-ide-reverted-x` |
| 6. regression | r2 flows c, d, e: pass (revc 16 checks, owns2 6/6, refuse2 2/2); round-1 set 67/68, the one FAIL is the SEC-10 baseline-blob line (same as r1/r2: 5 baselines of current files, by design) |

### Defect D-r3-1 (SEC-13): the `files deleted during scan` banner cannot be dismissed in one scenario
Steps (fake agent, big tree of 3000 folders x 50 files; harness `f1`, also `dbg1.py`): open the AI panel (agent running, walk in progress), the agent runs `rm -rf big/d29[0-9][0-9]; mv big/d28[0-9][0-9] <dir outside the workspace>/` (100 folders removed, 100 moved out), press F6, wait until the banner appears, wait 25 s to 120 s (CPU idle, walk over), press Enter repeatedly.
Expected: banner hides (Enter hides).
Seen: banner stays after every Enter, for 120 s: `! Incomplete: files deleted` / `during scan · enter hides` (80x24), `! Incomplete: files deleted during` / `scan · enter hides` (120x40); 4/4 size and theme combinations.
Contrast (same build, fresh state, Enter at 25 s): only `rm -rf` of 100/200 folders, only `mv` of 100 / 10 / 1 folders, 10 folders rm: Enter hides at once (dbg2, dbg4, dbg5). One earlier mv-only run (dbg3, first run) also failed, but its target folder still held earlier moved dirs, so that mv partly failed; not reproduced on reruns. I did not find the cause (instructed); trigger so far is rm + mv of many queued folders together. Possibly the walk keeps re-noting for each vanished folder, but CPU is idle so it looks like a stuck state, not a running walk.

### Rule check T6
| Rule | Verdict | Evidence |
|---|---|---|
| SEC-13 incomplete list marked | pass for marking (flows 1, 2), **fail for dismissing (Enter hides) in the rm+mv scenario** | D-r3-1 |
| SEC-12 / D18 / D20 | pass | revc 16/16 |
| SEC-9 / D21 incl. second quick save | pass | flow 3 6/6, owns2 6/6 |
| AC-9 regression | pass | 67/68, the one known by-design line |
| D22 banner layout | pass | flow 4 |

QC_FAIL: D-r3-1 (SEC-13 banner `files deleted during scan` not dismissable by Enter after rm + mv of many queued folders; repro above).

### r3 follow-up: D-r3-1 withdrawn (D26)
Replay of flow 1 once on the existing T6r3 release build (no rebuild), `flows_r3.py f1_80 f1_120` (dracula), output `qa-artifacts/T6/qc/r3_f1_rerun.txt`. Same steps (rm d29xx + mv d28xx), then F6, Up until the banner row is selected, Enter: banner gone at once and still gone 10 s later, at 80x24 and 120x40 (10/10 checks pass; banner rows before: `! Incomplete: files deleted` / `during scan · enter hides` at 80x24, `! Incomplete: files deleted during` / `scan · enter hides` at 120x40). The earlier failure was a harness error: my Enter landed on a file row (a d28xx folder walked before `mv` listed its files) and opened that file's diff, per ui-spec S3 (Enter on the banner row hides it). D-r3-1 withdrawn; no defect. The "Scanning notice seen" FAILs stay harness artifacts and the SEC-10 baseline line stays by design. Final r3 rule check: SEC-13, SEC-12/D18/D20, SEC-9/D21, AC-9, D22 all pass.

QC_PASS

## Sign-off run 4 — 2026-11-27

Tree: master ca60805 (includes T10 f47e194, T11 04af6ff); uncommitted changes only in `.ai-dev-team/`. Logs and harness: `qa-artifacts/signoff-run4/` (suite.log, fmt.log, clippy.log, build.log, walk.txt, qc/walk.py, qc/screens_*.txt).

### Full suite (`run-heavy.sh cargo test --workspace`, once, exit 0, 0 failed anywhere)
| Crate | Passed | Ignored |
|---|---|---|
| vr-config | 17 + 26 + 8 | 0 |
| vr-core | 156 | 2 |
| vr-fs | 44 + 2 (racing_writer) | 1 |
| vr-git | 11 + 14 + 27 + 9 (+ bench 0, 1 ignored) | 1 |
| vr-ide | 925 lib + 3 (desktop_cli) | 7 |
| vr-search | 43 + 2 | 1 |
| vr-syntax | 24 + 25 + 27 + 6 | 5 |
| vr-term | 21 + 3 + 1 + 1 (+ flood 1 ignored) + 1 + 6 + 21 | 1 |

Guard: `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` exit 0.
Release build (one, run-heavy): exit 0.

### AC coverage (task-level QC pass in this report)
| AC | QC task(s) | Result |
|---|---|---|
| AC-1, AC-2, AC-3, AC-4 | T1 (re-check), T6/T10 session lock flows | pass |
| AC-5 | T2 | pass (icon 16/32/48 render unverified, no renderer; noted at T2) |
| AC-6 | T3 (+ round 2) | pass |
| AC-7 | T4 | pass |
| AC-8, AC-13 | T5 (+ re-check) | pass |
| AC-9 | T6 (r1, r2, r3; D-r3-1 withdrawn by D26) | pass |
| AC-10 | T7 | pass |
| AC-11 | suite green above + spot checks T1 | pass |
| AC-12 | T8 (+ re-check), T10 | pass |
No AC without a task-level QC pass. T11 (test-only race fix) had review only by design (light tier); its test passed in the full run.

### Main-flow walk on the release binary (pty, fake claude shim, temp HOME/state/config, 14 checks per size)
| Flow | 120x40 | 80x24 |
|---|---|---|
| 1 `--session` quit with AI tab; session.toml 0600; relaunch restores AI tab | pass | pass |
| 2 `/ssh -i cred`+Tab completes `credentials/`; `/ssh myh`+Tab completes `myhost.example` | pass | pass |
| 3 AI changes list (`Changes · claude · 2 files`, M/A rows) -> diff modified/created -> created dialog mentions trash -> Revert created (file in `$XDG_DATA_HOME/Trash/files`, gone from folder) -> Revert modified (pre-agent bytes exact) | pass | pass |
| 4 Notify: bell while panel hidden marks tab (◆); viewing clears | pass | pass |
| 5 Account picker lists claude and `+ Add account…` | pass | pass |
Totals 14/14 and 14/14. Note: with `--session` the first run's only root is the temp `~` (AC-1), so the walk's fixtures live there; my first walk attempt failed 2a/3a for that harness reason only, fixed in the script, rerun green.

### Flaky list
No test went red; no rerun needed. Known flaky (event_loop exit_restores…, ai_account_tests remove_account…, ai_tests focused-keys fixed by T11) did not fail in this run.

SIGNOFF_PASS

## QC FR-1 — 2026-11-27
Engine: PTY harness (python pty + VT model) driving the release build of `.worktrees/FR1` (one `run-heavy.sh cargo build --release`, exit 0); fake-agent shim logs every start with its args to `starts.log`. Temp HOME/state/config under `qa-artifacts/FR-1/qc/`. No real AI tool. Scripts: `flows.py`, `probe2.py`; results `result.txt`, `probe2.txt`, `screen-B-*.txt`.
Note: `--session` takes no folder; the root is the saved/home folder, so fixtures live in the temp HOME. Two harness iterations before the valid run (wrong args, no file opened); the product was unchanged.

| Flow (80x24 and 120x40) | Result | Evidence |
|---|---|---|
| A: file tab + agent tab, quit, session file saved | PASS | result.txt |
| AC-8 relaunch A: agent resumed, shim started with `--continue` exactly once (count 0 to 1) | PASS | starts: `<pid> ` then `<pid> --continue` |
| B while A runs: 0 agents started (starts count unchanged 2 to 2) | PASS | result.txt |
| B notice "Another vr-ide --session window keeps the session; this one is not saved" whole | PASS | screen-B-*.txt |
| B notice "AI sessions not reopened: they run in the other vr-ide --session window" whole | PASS (shown second) | the toast queue shows one at a time ("+1 more"); after Esc the second appears whole at both sizes (probe2.txt). The first is whole at 80x24 except its "(+1 more · Esc)" hint wraps, existing toast behaviour |
| B still restores file tab | PASS | tab `aaa-note.txt x` |
| Quit B: session file byte-identical | PASS | |
| Quit A, start C: agent resumed with `--continue`, no notices | PASS | starts show a second `--continue` |

Harness line `B notice 2 whole` reported FAIL only because it checked the first screen; probe2 shows it on the next toast. Not a product defect.

## Rule check FR-1
| Rule | Status | Evidence |
|---|---|---|
| AC-1..4 / D1-D3 | pass | B notice text unchanged, B does not save, file unchanged after quit |
| AC-8 / D7 | pass | A and C resume with `--continue` |
| FR-1 (second window no agents) | pass | 0 shim starts in B at both sizes |
| BASE-14/15/17/19 | pass | one background build, no polling, no kill of others, no data probes |
| COST-21 | pass | 1 full run + 2 probes |
| No product edits | pass | only qa-artifacts/FR-1 |

QC_PASS

## QC FR-3 — 2026-11-27
Engine: PTY harness (terminal UI, no browser), release build of /home/rainy/DEV/vr-ide/.worktrees/FR3 (run-heavy, 4m17s, ok). Fake `ssh` shim (exit 0) on a temp PATH; temp HOME/state/config. Artifacts: $TEAM_WS/qa-artifacts/FR-3/qc/ (flows.py, sec16.py, sec16b.py, p_*.txt, s7/).
| Flow | Result | Evidence |
|---|---|---|
| D28 `/ssh myhost mysql -uroot -pS3cret -e 'select 1'` (exit 0) | PASS | `state/ssh-recent` = `<ws>\tmyhost` only; no `S3cret` in file or in the palette recent list |
| D28 `/ssh myhost ls -la` kept whole | PASS | file line `<ws>\tmyhost ls -la`; palette shows `myhost ls -la  recent` |
| SEC-5 `ssh-recent` mode | PASS | 0600 |
| SEC-16 `/ssh -i <state>/accounts/work/` and `.../accounts/` + Tab | PASS (weak) | no candidates; note Tab with no candidates resets the palette input to `/ssh` (looks pre-existing, not judged) ; control `/ssh -i ../sib/` completes to `../sib/alpha_sib/` |
| SEC-16 `/ws add <state>/accounts/` + Tab | **FAIL** | palette lists `../state/accounts/other/` and `../state/accounts/work/` (p_ws_accounts.txt, also absolute path form in sec16_b.txt). `/ws add .../accounts/work/` lists nothing (fixture folder had no subfolders, so uninformative) |
| SEC-16 sibling still completes | PASS | `/ws add ../sib/` -> `../sib/alpha_sib/`; `/ws add ../` lists siblings |
| SEC-7 `--install-desktop`, `.` first in PATH, fake in cwd | PASS | fake not run (no FAKE_RAN_CWD); with an absolute fake on PATH it ran (REAL_ABS_RAN); `.`-only PATH ran nothing |
Defect: SEC-16 incomplete. Repro: state dir with `accounts/work`, `accounts/other`; run vr-ide with a workspace, F1, type `/ws add <state>/accounts/` (relative `../state/accounts/` works too), Tab. Expected: no candidates. Actual: both account folder names listed. Likely cause: `/ws add` completion goes through the slash.rs listing path, not the guarded `path_cands`; the developer test `sec16_...` passes because `/ws add` only shows folders and it only probed `accounts/work/` containing files, so it never lists `accounts/` itself for `/ws add`. `/ssh -i` and `/open` paths are guarded.

## Rule check FR-3
| Rule | Status | Evidence |
|---|---|---|
| D28 | pass | file and UI hold no S3cret; plain line whole |
| SEC-5 | pass | 0600 |
| SEC-16 | FAIL | `/ws add <state>/accounts/` lists account names |
| SEC-7 | pass | fake in cwd not run, resolved absolute path run |
| COST-21 | pass | 1 build, 1 flow script + 2 probes |
Counts: pass 4 / fail 1.

QC_FAIL T FR-3

## QC FR-2 — 2026-11-27
Engine: terminal pty harness (copy of T6 harness, `qa-artifacts/FR-2/qc/fr2.py`), release build of worktree FR2, fake-agent shim, temp HOME as root and temp state dir. Sizes 80x24 and 120x40. Run logs: `qa-artifacts/FR-2/run1.txt`, `run2.txt` (run 1 failures were harness errors: wrong row name `proj/app.toml` (list shows basename), notices not dismissed; fixed in the script, no product change); screens in `qa-artifacts/FR-2/qc/screens.txt`.
- D27 baseline: root HOME with `.ssh/id_ed25519`, `.aws/credentials`, `.env`, `proj/app.toml` (unique markers). After the walk the store holds 2 blobs; no secret marker anywhere under the state dir; the app.toml marker is in a blob. PASS at both sizes.
- Agent edits to .env, app.toml, id_ed25519, credentials: all four listed (M); `.env` and secrets show "no earlier version". `.env` Revert refused, file untouched, notice "No earlier version of .env kept; Keep only"; Keep works ("Kept .env", row leaves). `app.toml` Revert restores exact bytes. PASS at both sizes.
- SEC-10: store dirs 0700, files 0600. PASS.
- Row counts after a refused Revert (second writer appended 3 lines between showing and confirming; notice "Not reverted: a.txt changed since shown; diff updated"): diff header `+5 -0`, list row `+5 -0`, header `@@ -1,3 +1,8 @@`. Equal. The script's final assertion printed FAIL at both sizes only because its regex took the first row of the filtered lines, the panel header; the evidence rows show the counts match. PASS by inspection of the printed rows.
- Not run at runtime (per brief): create_store/sweep race (unit tests, dev notes 7/7 green). A file the agent created in a secret place keeps Revert = trash (held S2 question, noted by dev).

## Rule check FR-2
| rule | status | evidence |
|---|---|---|
| D27 | pass | no secret bytes in store, app.toml kept, edits listed, .env refused/Keep, app.toml exact restore |
| SEC-10 | pass | 0700/0600 at runtime; race unit-tested |
| SEC-13 | pass | secret edits listed, not missing |
| SEC-16 | not run at runtime | unit test (dev notes) |
| row counts | pass | diff +5 -0 = row +5 -0 after refused Revert |
| BASE-17 | pass | temp dirs only, no external data |

QC_PASS FR-2

## QC FR-3 re-check r1 — 2026-11-27
Rebuilt release in .worktrees/FR3 (1m11s), replayed with qa-artifacts/FR-3/qc/r1.py (PTY, fake ssh shim, temp state; screens r1_*.txt).
| Flow | Result | Evidence |
|---|---|---|
| `/ws add ../state/accounts/` and absolute form + Tab | PASS | no candidates (only the command hint); defect fixed |
| `/ssh -i ../state/accounts/` and `.../work/` + Tab | PASS | no names |
| `/ws add ../sib/` | PASS | completes to `../sib/alpha_sib/` |
| `/ws add ../state/` | PASS | completes to `accounts/` only |
| Tab with no candidates on `/ssh -i /nonexistent/` | PASS | typed line kept |
| D28 `/ssh myhost -p 2222` | PASS | stored `myhost -p 2222` |
| D28 `myhost -p 2222 mysql -pX` | PASS | stored `myhost -p 2222`, no `mysql` |
| D28 quoted `"mysql -pS3cret"` / `'mysql -pS3cret'` | PASS | stored `myhost2` / `myhost3`, no S3cret in file |
| D28 `-i ~/.ssh/k myhost4` | PASS | stored `-i ~/.ssh/k myhost4` (tilde kept) |
| SEC-5 | PASS | 0600 |
SEC-7 passed in the first round; the fix did not touch desktop.rs, so not rerun.
## Rule check FR-3 (r1)
| D28 | pass | SEC-5 | pass | SEC-16 | pass | SEC-7 | pass |
Counts: pass 4 / fail 0.

QC_PASS

## QC FR-2 re-check r1 — 2026-11-27
Engine: same pty harness, release rebuild of FR2 after fix round 1 (`qa-artifacts/FR-2/build2.txt`), script `qa-artifacts/FR-2/qc/fr2.py` (row-count regex fixed to read the `M a.txt` row; fixtures added: `proj/src/credentials/provider.py`, `proj/src/id_map.rs`, `proj/deploy/id_ed25519`, `dotfiles/.config/gh/hosts.yml`). Log `qa-artifacts/FR-2/run3.txt`, screens `qc/screens.txt`. Sizes 80x24 and 120x40.
- No secret marker (ssh, aws, env, deploy id_ed25519, gh hosts.yml) anywhere under the state dir; 4 blobs = app.toml, provider.py, id_map.rs, a.txt. PASS both sizes.
- `provider.py` and `id_map.rs`: copied; agent edit, Revert restores exact bytes. PASS both sizes.
- `proj/deploy/id_ed25519`, `dotfiles/.config/gh/hosts.yml`: listed, "no earlier version", Revert refused with file untouched, Keep works. PASS both sizes.
- `.env` refused/Keep, `app.toml` exact restore: PASS both sizes.
- Row counts after a refused Revert: diff `+5 -0` = row `+5 -0`. PASS both sizes.
- SEC-10 store 0700/0600: PASS both sizes.
- One FAIL line at 80x24, "agent edits listed": the list shows 7 of 8 rows in the 24-row viewport (the `credentials` row is below the fold); all 8 rows are listed at 120x40 and the Changes header says "8 files". Viewport limit of the harness assertion, not a defect (not rerun, COST-21).

## Rule check FR-2 r1
| rule | status | evidence |
|---|---|---|
| D27 | pass | secrets and other-device not copied; edits listed |
| D29 | pass | source files with secret-like folder names copied and revertable; id_* / gh hosts.yml not copied |
| SEC-10 | pass | 0700/0600 |
| SEC-13 | pass | all secret edits listed |
| row counts | pass | +5 -0 both |

QC_PASS FR-2

## Re-verification after Phase 5 fixes — 2026-11-27
Master db788d0 (FR-1..FR-4 merged). Engine: PTY harness (terminal UI, no browser), one release build (run-heavy, ok), fake agent shim and fake ssh, temp HOME/state/config. Artifacts: qa-artifacts/final-run4/ (f1 FR-1, f2 FR-2, f3 FR-3, f4 FR-4/main flow; out*.txt, test.log, clippy.log, build.log).

| Check | Result | Evidence |
|---|---|---|
| `cargo test -p vr-ide` (once) | PASS | 946 passed, 0 failed, 7 ignored (lib) + 3 passed (other target) |
| Guard: `cargo fmt --all --check` + clippy `--workspace --all-targets -D warnings` | PASS | FMT_OK, clippy exit 0 |
| (a) FR-1 `--session` A + B at 80x24 and 120x40 | PASS | B started 0 agents (2->2), session file unchanged after B quit, A/C resume with `--continue`, file tab restored; B shows both notices (toast queue, 2nd after Esc; harness line "notice 2 whole" is the known first-screen check, same as FR-1 section, 26/28 raw) |
| (b) FR-2 secrets store, 80x24 and 120x40 | PASS | no QCMARK secret bytes under store/state; .env/id_ed25519/hosts.yml Revert refused, Keep works; app.toml, provider.py, id_map.rs Revert exact bytes; store 0700/0600; list counts == diff header after refused Revert. One raw FAIL at 80x24 ("edits listed") is a harness text check that expects "credentials" in a path column hidden at 80 cols; the 8-row list (.env, provider.py, id_map.rs, id_ed25519, hosts.yml, app.toml, ...) is present in the same dump |
| (c) FR-3 `/ssh myhost -p 2222 mysql -pX` | PASS | recent file stores `myhost -p 2222`, no `mysql`, quoted secrets not stored, `-i ~/.ssh/k myhost4` kept, 0600 |
| (c) FR-3 `/ws add <state>/accounts/` + Tab (relative, absolute), `/ssh -i .../accounts/` | PASS | no names listed; sibling `../sib/` still completes; no-candidate Tab keeps typed line (r1.py replay, 12/12) |
| (d) FR-4 picker title at 80x24 | PASS | long folder shows `Start in …ry-long-folder-name` (folder name kept, `…` prefix); short folder untouched; 120x40 fits with `…` shorter |
| (d) FR-4 diff header right margin | PASS | 120x40 header `f5 keep  shift+f5 revert │` keeps one free cell before the border; 80x24 header fits without keys |
| Main-flow walk (sign-off walk.py) | PASS | 14/14 at 80x24 and 14/14 at 120x40 (session restore, /ssh completion, changes list, diff, revert to Trash/exact bytes, bell mark, picker) |

Product unchanged, no edits outside qa-artifacts/final-run4. COST-21: one full run per flow script, 2 probes for FR-4 (picker long folder), no flaky reruns.

QC_PASS
