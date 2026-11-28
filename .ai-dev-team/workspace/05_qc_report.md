
## QC T1
Engine: python pty driver (`qa-artifacts/T1/qc/drive.py`, `q.py`), real binary `target/debug/vr-ide` (built with `cargo build -p vr-ide`, OK), TERM=xterm-256color, pty 24x80, isolated HOME/XDG dirs, file `proj/a.txt`.
- Start: alternate screen entered (`ESC[?1049h`), bracketed paste on (`?2004h`), title `ESC]0;vr-ide — <root>` written once, initial cursor shape `ESC[2 q`.
- Edit mode (`space i`): cursor `ESC[5 q`; typed XYZ drawn; `esc`: back to `ESC[2 q`.
- ctrl+s: file on disk became `XYZhello` (saved).
- Bracketed paste `ESC[200~PASTED ESC[201~`: PASTED drawn in the editor.
- Terminal panel (ctrl+` as kitty CSI-u): shell opened, `echo QCMARK$((20+22))` output `QCMARK42` seen.
- Resize via TIOCSWINSZ + SIGWINCH (30x100, 20x60): full redraw each time (3.8 KB, 1.9 KB).
- ctrl+q with a dirty buffer: "Quit — 1 unsaved file(s)" dialog shown, process alive; esc dismissed it; second ctrl+q + enter exited, status 0.
- Restore at exit: `ESC[0 q` (cursor shape reset), `?2004l`, mouse modes off, `?1049l` (alternate screen left), cursor shown.
- Not provable here: kitty flag push/pop (the pty does not answer the keyboard-protocol query, so none was pushed; covered only by the developer's unit tests). No regression indicator.
- Repo files not edited by QC; artifacts only under `qa-artifacts/T1/qc/`.

## Rule check T1
| Rule | Result | Evidence |
|---|---|---|
| AC-W1 TUI behaviour unchanged at runtime | PASS | start/edit/save/paste/terminal/resize/dirty-quit/restore all behave as documented |
| BASE-26 runtime proof of main flow | PASS | real binary in a pty, outputs above |
| BASE-50 no repo edits outside qa-artifacts/T1 and 05_qc_report.md | PASS | only those paths written |

## QC T2
Engine: Rust scratch program (not a browser) at qa-artifacts/T2/qc/render (own [workspace], own target dir, path dep on vr-window), frames drawn through `ratatui::Terminal<PixelBackend>` and written to PNG; each PNG viewed. No repo file touched.
Frames (1920x1080 scale 1: grid 192x56, cell 10x19; 960x540 scale 2: grid 50x14, cell 19x38), all in qa-artifacts/T2/qc/render/out/:
- (a) Vietnamese `Tiếng Việt: ệ ở Ặ ữ ỹ Nguyễn Thị Hương`: stacked diacritics correct, in one cell each, no clipping, both scales (`*_text.png`).
- (b) CJK/Hangul/emoji: 2-cell wide, not overlapping neighbours; emoji 😀🚀👍 colour in 2 cells; ❤ monochrome (font), in its cell.
- (c) bold, italic, bold+italic, underline, reversed, dim (visibly dimmer), crossed-out; Rgb fg/bg, Indexed 196/21/244, named Red/LightGreen/Blue, reversed Rgb: all correct.
- (d) cursor block / bar / underline (`*_cursor_*.png`): shapes right, cell restored after change.
- (e) preedit `việ` underlined at col 5,row 6 (`*_preedit.png`); removed cleanly (`*_preedit_removed.png`, blank row).
- (f) `[中中]` redrawn as `[ab]  ` in a second Terminal draw: no stale half glyph, blank cells clean.
Bench (run-heavy, release, once): 1920x1080 s1 full 4.77 ms (cold 16.63), line 0.095 ms; s2 3.18 / 0.100; 4K s1 16.64 / 0.441, s2 13.03 / 0.269. Developer's numbers 4.82 / 0.103 reproduced within noise; gate <= 16 ms passes; one-line change ~50x smaller than full.
Note (minor, non-blocking): `FontSet::new` with the default config resolved to "FreeMono" in my scratch program, while `fc-match monospace` and the dev notes say "DejaVu Sans Mono". Rendering is still a valid monospace grid; T3/T4 may want to confirm the default family choice.

## Rule check T2
| Rule | Status | Evidence |
|---|---|---|
| AC-W2 | pass | frames (a)-(f), both scales, PNGs under qa-artifacts/T2/qc/render/out/ |
| AC-W3 | pass | bench rerun: 4.77 ms full redraw at 1920x1080 s1 (<= 16), line 0.095 ms |
| BASE-26 | pass | runtime evidence: rendered frames viewed and bench run on the real code |
| BASE-50 | pass | only qa-artifacts/T2/qc/ and this report written |

## QC T5
Engine: CLI (target/debug/vr-ide) with XDG_DATA_HOME and HOME pointed at temp dirs under qa-artifacts/T5/qc/. data_home() reads XDG_DATA_HOME (absolute) else HOME/.local/share (desktop.rs:287).
- Install: Exec=`"<exe>" --session` quoted, TryExec, Terminal=false, StartupWMClass=vr-ide, marker X-VrIde-Generated=true, icon svg written. desktop-file-validate exit 0. PASS
- Install twice: byte-identical. PASS
- Foreign vr-ide.desktop (no marker): install exits 1 with message, file byte-identical; uninstall says "nothing to remove", file identical. PASS
- Uninstall removes our .desktop and icon. PASS
- Real ~/.local/share/applications/vr-ide.desktop mtime unchanged (2026-11-27 20:59:18); fakehome stayed empty. PASS
- Migration (migrate_on_window_start): not reachable from the CLI, wired in T4 -> unverified (wired in T4).
- Note: install leaves mimeinfo.cache in the temp home (from update-desktop-database, pre-existing behaviour, harmless).

## Rule check T5
| Rule | Result |
|---|---|
| AC-W7 (install part) | pass at runtime; migration call unverified (wired in T4) |
| BASE-26 | pass (real user dirs untouched, temp homes only) |
| BASE-50 | pass (only qa-artifacts/T5/qc/ and this report written) |

## QC T5 re-check
Rebuilt `cargo build -p vr-ide` (ok). Temp XDG_DATA_HOME/HOME under qa-artifacts/T5/qc/.
- gnome-terminal on PATH: Exec=`gnome-terminal --title=vr-ide -- "<exe>" --session`, Terminal=false, marker present; desktop-file-validate 0; second install identical. PASS
- gnome-terminal absent (PATH=empty dir): Exec=`"<exe>" --session`, Terminal=true, marker present; validate 0. PASS
- Foreign entry: install refuses (rc 1), file byte-identical. PASS
- Uninstall removes ours. Real ~/.local/share/applications/vr-ide.desktop mtime unchanged (2026-11-27 20:59:18). BASE-50 pass.

## QC T2 re-check
Rebuilt the scratch program (qa-artifacts/T2/qc/render) against the fixed vr-window. `FontSet::new` with the default config now resolves to "DejaVu Sans Mono" at scale 1 and 2 (matches `fc-match monospace`), no warning; cell 10x19 at scale 1.
Re-rendered at 1920x1080 scale 1 (qa-artifacts/T2/qc/render/out2/1920x1080_s1_text.png and 1920x1080_s1_preedit.png, both viewed): Vietnamese line correct (pixel dump of the ế cell shows acute and circumflex both inside the cell, not clipped); preedit "việ" underlined at col 5,row 6; other styled/wide/emoji rows unchanged and correct. Only qa-artifacts/T2/qc/ and this report written (BASE-50).
Verdict: QC_PASS T2

## QC T4a
Engine: real `target/debug/vr-ide` (built with `cargo build -p vr-ide`, ok) in a pty driver, `VR_IDE_CONFIG_DIR` pointed at `qa-artifacts/T4a/qc/cfg_*` (real ~/.config untouched). Driver: `qa-artifacts/T4a/qc/run.py`, logs `screen_*.log`.
- AC-W8 settings side, bad values: `font_size = 99` -> status warning "settings.toml: font_size: font_size must be between 6 and 72"; `line_height = 0.5` -> "line_height: line_height must be between 1.0 and 2.0"; two-line `font_family` -> "font_family: font_family must be one line of at most 100 characters" (+2 more queued when all three are bad); `line_height=2.5`+`font_size=5` -> font_size warning, "+1 more". App stays alive and accepts edit input (typed text drawn) in every case. PASS.
- Valid values (14.0 / 1.5 / "DEJAVU Sans Mono") and no settings file: no warning, app works. PASS.
- `/set font_size 99`: not exercised at runtime (my driver could not open the command window reliably; the notice queue swallowed the keys). Static: `/set` uses `Settings::with_value`, which accepts any key in `Settings::names()` (derived from the defaults table, which now holds the three keys) and re-validates through the same `parse` range check, so out-of-range values return an error. Not runtime-proven; no defect found.
- AC-W6 PTY part: not rerun (developer's PTY test).
- Experiments only under qa-artifacts/T4a/qc/; `git status` shows no new repo files from QC.

## Rule check T4a
| Rule | Result | Evidence |
|---|---|---|
| AC-W8 settings warnings at runtime | pass | bad values name the key; good values silent; app keeps working |
| AC-W8 `/set` for font keys | not run (static ok) | see above |
| BASE-26 | pass | warnings name the key and the range; app not crashed |
| BASE-50 | pass | only qa-artifacts/T4a/qc and this report written |

## QC T3
Engine: scratch cargo project `qa-artifacts/T3/qc/parity/` (own `[workspace]`, own target dir, crossterm 0.29.0 from the repo lock). TUI side = real crossterm parser through a pty (`tui` bin: raw mode + `event::read()` + `vr_ide::event_loop::chord_from_key(&k, true)`), kitty CSI-u bytes written as a terminal sends them for DISAMBIGUATE|REPORT_ALTERNATE_KEYS|REPORT_EVENT_TYPES (plain text keys as text, Enter/Tab/Backspace legacy bytes when unmodified, F1 SS3, etc.). Window side = hand-built `KeyInput` the OS would give, `translate_key`, then the same `chord_from_key`. Driver: `qa-artifacts/T3/qc/drive.py`; raw table with crossterm events: `qa-artifacts/T3/qc/table.md`. 49 chords.

Result: 47 equal, 2 differ.

DIFFERENCES
- ctrl+shift+/ : TUI `ctrl+?` (crossterm decodes `47:63;6u` to Char('?') + CONTROL and drops SHIFT), window `ctrl+shift+/`.
- ctrl+shift+1 : TUI `ctrl+!`, window `ctrl+shift+1`.
Cause: crossterm 0.29 replaces the key by the shifted alternate and removes SHIFT for symbols; `chord_from_key` then never reaches `unshift_us`. The window keeps SHIFT (dev notes, deviation 1), which gives the keymap name. So a binding `ctrl+shift+/` or `ctrl+shift+1` fires in the window but not in `--tui`; this breaks the anti-goal "same chords in window and --tui" for shifted-symbol chords. Letters (ctrl+shift+a/p) match. Fix belongs to the leader's choice: either make `chord_from_key` map a ctrl/alt chord on a shifted symbol (no SHIFT flag, symbol in `unshift_us` domain) to `ctrl+shift+<unshifted>` (fixes the TUI, window already right), or make the window drop SHIFT (then neither names the keymap form).

Table (chord | TUI | window | equal?)
| chord | TUI | window | equal? |
|---|---|---|---|
| a | a | a | yes |
| z | z | z | yes |
| 1 | 1 | 1 | yes |
| shift+a | shift+a | shift+a | yes |
| shift+/ (?) | ? | ? | yes |
| shift+1 (!) | ! | ! | yes |
| shift+2 (@) | @ | @ | yes |
| space | space | space | yes |
| ctrl+a | ctrl+a | ctrl+a | yes |
| ctrl+p | ctrl+p | ctrl+p | yes |
| ctrl+shift+a | ctrl+shift+a | ctrl+shift+a | yes |
| ctrl+shift+p | ctrl+shift+p | ctrl+shift+p | yes |
| alt+a | alt+a | alt+a | yes |
| alt+shift+a | shift+alt+a | shift+alt+a | yes |
| ctrl+alt+a | ctrl+alt+a | ctrl+alt+a | yes |
| ctrl+[ | ctrl+[ | ctrl+[ | yes |
| ctrl+h | ctrl+h | ctrl+h | yes |
| ctrl+i | ctrl+i | ctrl+i | yes |
| ctrl+/ | ctrl+/ | ctrl+/ | yes |
| ctrl+shift+/ | ctrl+? | ctrl+shift+/ | **NO** |
| ctrl+1 | ctrl+1 | ctrl+1 | yes |
| ctrl+shift+1 | ctrl+! | ctrl+shift+1 | **NO** |
| ctrl+, | ctrl+, | ctrl+, | yes |
| ctrl+` | ctrl+` | ctrl+` | yes |
| F1 | f1 | f1 | yes |
| F5 | f5 | f5 | yes |
| shift+F5 | shift+f5 | shift+f5 | yes |
| ctrl+F12 | ctrl+f12 | ctrl+f12 | yes |
| Up | up | up | yes |
| shift+Up | shift+up | shift+up | yes |
| ctrl+Left | ctrl+left | ctrl+left | yes |
| alt+Right | alt+right | alt+right | yes |
| ctrl+shift+Down | ctrl+shift+down | ctrl+shift+down | yes |
| Home | home | home | yes |
| End | end | end | yes |
| ctrl+Home | ctrl+home | ctrl+home | yes |
| PageUp | pageup | pageup | yes |
| Delete | delete | delete | yes |
| ctrl+Delete | ctrl+delete | ctrl+delete | yes |
| Insert | insert | insert | yes |
| Enter | enter | enter | yes |
| keypad Enter | enter | enter | yes |
| shift+Enter | shift+enter | shift+enter | yes |
| ctrl+Enter | ctrl+enter | ctrl+enter | yes |
| Esc | escape | escape | yes |
| Tab | tab | tab | yes |
| shift+Tab | shift+tab | shift+tab | yes |
| ctrl+Tab | ctrl+tab | ctrl+tab | yes |
| Backspace | backspace | backspace | yes |
| ctrl+Backspace | ctrl+backspace | ctrl+backspace | yes |
| alt+Backspace | alt+backspace | alt+backspace | yes |

Notes: keypad Enter is `enter` on both (the kitty flags used do not report keypad separately; window carries `KEYPAD` state, ignored by the chord reader). ctrl+[ , ctrl+h, ctrl+i, ctrl+/ stay distinct from Esc/Backspace/Tab on both.

### AC-W5 IME (Telex "việt") via `ImeInput`, output `qa-artifacts/T3/qc/ime.txt`
Enabled -> no events. Preedit v, vi, vie, viê, việ, việt -> no events out each step, preedit held (`Some("việt")`), a stray key event while composing is held back (empty). `Commit("việt")` -> Press events Char('v'), Char('i'), Char('ệ'), Char('t'), no modifiers; preedit cleared afterwards; empty Preedit -> nothing. Matches AC-W5 (nothing sent until commit; one Press per character).

## Rule check T3
| Rule | Status | Evidence |
|---|---|---|
| AC-W4 | fail | 47/49 chords equal; ctrl+shift+/ and ctrl+shift+1 differ (table above) |
| anti-goal same keys window/--tui | fail | same two chords |
| AC-W5 sequences | pass | Telex "việt": no output while composing, one Press per char on commit, preedit cleared (ime.txt) |
| BASE-26 | pass | both sides of the contract compared with real crossterm parser output and the real `chord_from_key` |
| BASE-50 | pass | only `qa-artifacts/T3/qc/` and this file written (git status below) |

## QC T3b
Rebuilt the parity harness on the current tree and replayed the same pty run (real crossterm parser vs window translation, both through chord_from_key): 53 rows, all equal, none differ (raw: qa-artifacts/T3/qc/table2.md). ctrl+shift+/ and ctrl+shift+1 equal on both; new rows shift+space (shift+space), alt+shift+1 (shift+alt+1), ctrl+alt+shift+` (ctrl+shift+alt+`) equal. Plain ? (Char('?'), no mods), space (Char(' '), no mods), a, shift+a (Char('A')+SHIFT) produce identical events on both sides as before.
| Rule | Status | Evidence |
|---|---|---|
| AC-W4 / same keys window vs --tui | pass | all rows equal |
| BASE-50 | pass | only qa-artifacts/T3/qc/ and this file written |

## QC T3b round 1
Tree: keymap-parser normalisation (`KeyChord::new`), sources not newer than my harness/vr-ide binaries (checked with find -newer).
1. Parity replay (same pty driver, real crossterm parser vs window): 54 rows (table3.md), all "yes", none differ. ctrl+shift+/ = ctrl+shift+/ and ctrl+shift+1 = ctrl+shift+1 on both sides; shift+space, alt+shift+1, ctrl+alt+shift+` equal; plain ? (no mods), space, a, shift+a (SHIFT) identical on both.
2. Runtime in the real TUI (target/debug/vr-ide in a pty, VR_IDE_CONFIG_DIR, HOME and XDG dirs all under qa-artifacts/T3/qc/rt/<case>/, never ~/.config). File `    hello` (4 spaces), enter edit mode (space i), send the key, ctrl+s, read the file (rt/run.py, rt.txt):
| case | bytes sent | keymap.toml | file after |
|---|---|---|---|
| control: no binding | ESC[91;6u | none | `    hello` (unchanged) |
| sanity: default shift+tab | ESC[9;2u | none | `hello` |
| SHIFT reported | ESC[91;6u (ctrl+shift+[) | `ctrl+{ = "edit.outdent"` | `hello` (fired) |
| alternate-key form | ESC[91:123;6u | same | `hello` (fired) |
| symbol, no shift flag | ESC[123;5u | same | `hello` (fired) |
Without the binding the same bytes do nothing, so the outdent comes from the user binding.
| Rule | Status | Evidence |
|---|---|---|
| AC-W4 / same keys | pass | all parity rows equal |
| ctrl+{ user binding fires | pass | 3 of 3 forms |
| BASE-50 | pass | only qa-artifacts/T3/qc/ and this file written |

## QC T4 (Window app) — mode QC, side fe
Engines: real run of the debug binary built in the worktree (merged tree, `run-heavy.sh cargo build -p vr-ide` green). Input driven on X11 (Xwayland) with a ctypes XTEST driver (`qa-artifacts/T4/qc/xin.py`; xdotool/xclip/wl-paste/grim not installed, nothing installed). Screenshots via `xwd -id <window>` (root capture is refused by Xwayland). Config/state/data/HOME/XDG all under `qa-artifacts/T4/qc/`.
Tests rerun once after the merge: `cargo test -p vr-ide event_loop` 17 passed; `cargo test -p vr-window` 53 passed.

| Check | Result | Evidence (in qa-artifacts/T4/qc/) |
|---|---|---|
| AC-W1 X11 window | PASS | xwininfo/xprop: WM_CLASS "vr-ide","vr-ide", _NET_WM_NAME "vr-ide — /home/rainy/DEV/vr-ide", 1080x648 (120x36 cells of 9x18); s2_typed.png |
| AC-W1 Wayland window | PASS (no screenshot) | DISPLAY unset + WAYLAND_DISPLAY=wayland-0: no fallback line, process stays up, EXIT_AFTER_FIRST_FRAME exit 0, SIGTERM (clean) exits; no tool to capture GNOME Wayland |
| AC-W1 `--tui` in pty | PASS | tui.typescript: alt screen 1049h, text drawn, runs until timeout |
| AC-W1 no DISPLAY/WAYLAND_DISPLAY | PASS | nd.typescript: TUI (1049h), no fallback line (nod.sh) |
| AC-W1 DISPLAY=:99 unreachable | PASS | bad.typescript: stderr "vr-ide: cannot connect to the display: ...; running in the terminal instead" then TUI |
| AC-W4 observable: typing, mouse click, ctrl+s | PASS | click moved cursor (s3.png), "space i" then "ZZ" typed, ctrl+s wrote "lineZZ one" to proj/a.txt (s5_saved.png) |
| AC-W4 chord the TUI cannot send | PASS | ctrl+/ toggled the comment (s8_slash.png). Note: ctrl+shift+p has no default binding (palette is f1), so ctrl+/ was used |
| AC-W6 embedded terminal | PASS | ctrl+` opened bash (s9_term.png); TIOCGWINSZ = (13 rows, 120 cols, 1080 px, 234 px) = grid x 9x18 cell (s10_pix.png) |
| AC-W6 wheel | PASS | seq 1 200 in terminal, wheel up scrolled the view from 190s to 160s (w01.png) |
| AC-W6 clipboard | PASS | instance A: select line + ctrl+c; separate instance B: ctrl+v inserted "COPYME alpha" (cl_B.png) via arboard, two processes |
| AC-W8 close with running terminal + dirty buffer | PASS | _NET_CLOSE_WINDOW: "Quit: 1 terminal still running" dialog, process alive, Cancel kept window (c1.png, c2_cancel.png); after the shell exited: "1 unsaved file(s) ... Discard keeps a copy in the recovery folder" (c3_unsaved.png) |
| AC-W8 SIGTERM with dirty buffer | PASS | SIGTERM did not kill: Quit dialog shown (t1_sigterm.png), process alive; Discard then exited, stderr named the recovery copy, `state/recovery/...a.txt` holds the content, file on disk intact |
| AC-W8 same for second window (dirty d.txt) | PASS | Discard: recovery count 2, d.txt unchanged |
| AC-W7 `--install-desktop` | PASS | data2: Exec="<exe>" --session, Terminal=false, StartupWMClass=vr-ide, marker, icon |
| AC-W7 migration | PASS | data3 old `gnome-terminal --title=vr-ide -- "<exe>" --session` entry rewritten to the window form on window start; notice "Updated the vr-ide menu entry to open its own window." shown (m1_notice.png); entry for another exe (data4) and one without the marker (data5): byte-identical (cmp) |
| AC-W8 fonts | PASS | font_size=16 + font_family="NoSuchFont": window 1200x684 (larger cells), draws, warning line "font family "NoSuchFont" is not installed; using the default monospace font (Esc)" (f1_font.png) |
| AC-W2 observable | PASS | theme colours, Vietnamese/ASCII text drawn in the grid, 16 pt scaling relays the grid |

Not proven (no tool/hardware, left to the developer's manual list): IME composition (IBus/Fcitx), HiDPI move between scales, Wayland screenshot and CSD title bar, attention hint, cursor shape/blink, AltGr, drag-select. No defect found. The pre-existing `vr-ide` process (pid 1696342, 10 h old) is the user's and was not touched; all processes I started have exited. Repo files untouched (writes only under qa-artifacts/T4/qc and this report).

## Rule check T4
| Rule | Status | Evidence |
|---|---|---|
| AC-W1 | pass | window X11/Wayland, `--tui`, no display, bad display (above) |
| AC-W2 | pass (observable part) | screenshots |
| AC-W4 | pass (observable part) | typing, click, ctrl+/, ctrl+s; IME not provable |
| AC-W6 | pass | PTY pixel size, wheel, clipboard across processes; title seen in xprop |
| AC-W7 | pass | install + migration + untouched foreign entries |
| AC-W8 | pass | close and SIGTERM with dirty buffer and running terminal; font warning |
| BASE-26 | pass | `--tui` real pty run draws on the alt screen as before; event_loop tests 17/17 after the merge |
| BASE-50 | pass | wrote only under qa-artifacts/T4/qc and 05_qc_report.md; no edits in $WT or the main tree; real ~/.config, ~/.local untouched (env redirected) |

## QC T4 re-check (fix round 1)
Rebuilt in the worktree (`cargo build -p vr-ide`, up to date). Temp dirs only under qa-artifacts/T4/qc/.

| Check | Result | Evidence |
|---|---|---|
| (a) X11: dirty buffer, window-manager close | PASS | Quit dialog "1 unsaved file(s)" shown, process alive (r1_close.png) |
| (a) X11: SIGTERM with dirty buffer | PASS | process stays alive, Quit dialog (r2_term.png); Discard exits, stderr names the recovery file, recovery dir 2 -> 3 files, r.txt on disk unchanged |
| (a) Wayland: SIGTERM, clean buffer | PASS | exits. Dirty buffer on native Wayland not provable: XTEST input does not reach a Wayland-native window and no virtual-keyboard tool is installed |
| (b) `DISPLAY=:99`, pty 100x30, COLORTERM unset | PASS | ptyrun.py output: 0 truecolor sequences, 256-colour sequences present, stderr "…running in the terminal instead" present (pty_none.out). Control with COLORTERM=truecolor: truecolor sequences, no 256 (pty_truecolor.out) |
| (c) display lost after start on Wayland, no tty | FAIL | `mutter --headless --wayland --no-x11 --wayland-display=vrqc-wl` (own instance, killed afterwards); vr-ide started on it with stdin /dev/null (up, no stderr, so it was connected); compositor SIGKILLed (mutter confirmed dead). vr-ide was still alive 6 s later and until the 30 s `timeout` killed it (lost.sh, lost.result: "alive 6s after compositor died: yes", exit 124). No exit through the normal path, no recovery path reached. The dirty-buffer variant could not be driven (no input into a headless compositor), but the process does not even exit with a clean buffer. |

Note: an earlier attempt that killed only the `timeout` wrapper left a mutter running; I stopped it by its own pid; no mutter or vr-ide of mine remains.
Verdict: QC_FAIL T4 for case (c).

## QC T4 re-check 2 (fix round 2)
Build current (`cargo build -p vr-ide` up to date).

| Check | Result | Evidence |
|---|---|---|
| Withdraw of re-check case (c) FAIL | WITHDRAWN: harness bug of mine | `qc/run.sh wl` exported `WAYLAND_DISPLAY=wayland-0`, overriding the test display, so vr-ide had attached to the user's session. run.sh now honours `WLD`; lost.sh records `/proc/<vr-ide pid>/environ` |
| Replay (c): headless mutter on `vrqc-wl`, vr-ide with no tty, mutter SIGKILLed | PASS | lost.result: comm=vr-ide, `WAYLAND_DISPLAY=vrqc-wl`; process gone within 6 s, exit code 1, stderr "Error: the connection to the display was lost" (normal error exit, no "running in the terminal instead"). Clean buffer only; no input path into headless mutter |
| New X11 case: Xephyr :7, vr-ide no tty (`DISPLAY=:7` in environ), XTEST edit "EDIT", Xephyr SIGKILLed | PASS | xeph.result: vr-ide gone within 5 s, exit 1; stderr "vr-ide: the connection to the display was lost" + "unsaved buffer written to .../state/recovery/…e.txt"; recovery file content "EDITbase" (recovery dir 3 -> 4); disk file e.txt still "base" |
Processes I started (mutter, Xephyr, vr-ide) all ended; nothing left (the only `ps` hits were the shell command lines themselves).

## Sign-off run 6
Tree: HEAD c945a44 on master, main tree. Throwaway files only in qa-artifacts/signoff/ (test.log, guard.log, release.log, s1-s6.png, run.sh, tui.py, ac.py, xin.py).

### Full suite (`run-heavy.sh cargo test --workspace -j 4`, once, exit 0)
Total 1728 passed, 0 failed, 18 ignored (all ignored are pre-existing `#[ignore]` benches/heavy tests). vr-git `dropping_the_worker_kills_its_running_git` did not flake this run (no rerun needed).
| Crate (summary lines) | passed | failed | ignored |
|---|---|---|---|
| vr-config (lib 17, config 29, languages 8) | 54 | 0 | 0 |
| vr-core | 156 | 0 | 2 |
| vr-fs (lib 44, racing_writer 2) | 46 | 0 | 1 |
| vr-git (lib 11, read 14, security 27, worker 9, bench 0) | 61 | 0 | 1 |
| vr-ide (lib 1157, desktop_cli 3, login_helper 6, statusline_helper 4) | 1170 | 0 | 7 |
| vr-search (lib 43, racing_writer 2) | 45 | 0 | 1 |
| vr-syntax (lib 24, grammars 25, highlight 27, registry 6) | 82 | 0 | 5 |
| vr-term (lib 23, attention 3, env 1, flood 1, leak 1, marker 6, pty 22) | 57 | 0 | 1 |
| vr-window | 57 | 0 | 0 |

### Guard
`cargo fmt --all --check` OK; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo clippy -p vr-ide --no-default-features --all-targets -- -D warnings` clean; `cargo tree -p vr-ide --no-default-features --features window -e normal | grep -ciE "gtk|glib|webkit"` = 0.

### Release build
`run-heavy.sh cargo build --release -p vr-ide`: OK (3m42s), binary 29.4 MB.

### Walk 1: release binary, window on X11 (:0, WAYLAND_DISPLAY unset, temp HOME/XDG/VR_IDE_* dirs)
- Started with `a.txt`: window "vr-ide" (WM_CLASS vr-ide) 1080x648, project root opened (git root), file open, no stderr (s1.png).
- `space i`, typed ZZ, ctrl+s: file on disk "ZZline one" (s2/s3.png).
- ctrl+` opened embedded bash; `echo signoffok` printed `signoffok` (s4.png).
- ctrl+q with terminal running: "Quit - 1 terminal still running" dialog, process alive (s6.png); "Quit anyway": window gone, process exited, stderr empty, no leftover process. (Exit code not observable: the process was not a child of the driver shell.)
- Driver limitation: text() cannot send `$`/parens, so a simple echo was used.

### Walk 2: `--tui` release binary in a pty (30x100, no DISPLAY)
Open b.txt, `space i`, TUIOK typed, ctrl+s: file "TUIOKbase"; ctrl+q exited status 0, alt screen entered and left (tui.out).

### AC coverage
| AC | Proven by | Status |
|---|---|---|
| AC-W1 window / --tui / no display / bad display | QC T1, T4, this sign-off walks | proven |
| AC-W2 cell grid text, fonts, colours | QC T2 frames + re-check, T4 screenshots, s1-s4.png | proven (HiDPI move between scales manual only) |
| AC-W3 redraw <= 16 ms | QC T2 bench 4.77 ms (release) | proven |
| AC-W4 chord parity | QC T3, T3b (54 rows equal), T4 observable, sign-off typing | proven; real-keyboard AltGr manual only |
| AC-W5 IME | QC T3 ImeInput sequences (Telex "viet") | proven at state-machine level; real IBus/Fcitx composition and candidate window position manual only |
| AC-W6 mouse, wheel, clipboard, title, PTY pixels | QC T4 (click, wheel, 2-process clipboard, TIOCGWINSZ px, title); cursor shape/blink, attention hint on real compositor | proven except cursor shape/blink and attention hint (manual only) |
| AC-W7 desktop entry + migration | QC T5, T5 re-check, T4 | proven |
| AC-W8 close/SIGTERM via quit path, font warning | QC T4 + re-checks 1-2 (display lost incl.), T4a, sign-off quit dialog | proven; dirty buffer on native Wayland not provable (no input tool) |
| BASE-26 | full suite + both walks on real release binary | pass |
| BASE-50 | wrote only qa-artifacts/signoff/ and this file; real ~/.config, ~/.local untouched (env redirected) | pass |

Manual-only items (no tool/hardware): IME composition in IBus/Fcitx, HiDPI scale change, Wayland screenshot/CSD title bar, cursor shape/blink, attention hint, AltGr, drag-select.
Verdict: SIGNOFF_PASS

## QC FR-1
Main tree `/home/rainy/DEV/vr-ide`, `cargo build -p vr-ide` green; real runs on X11 (`DISPLAY=:0`, WAYLAND unset), temp dirs under qa-artifacts/T4/qc/ (run.sh with `BIN=`). Only my own pids signalled; pid 1696342 untouched.

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | SIGINT with dirty buffer | PASS | window stays, Quit dialog "1 unsaved file(s)" (fr_int.png); Discard exits, recovery dir 4 -> 5, content "EDITbase", disk file s.txt unchanged "base", stderr names the recovery file |
| 2 | SIGHUP | PASS | process alive, no dialog, buffer still "EDITbase" and dirty (fr_hup.png); ctrl+q then shows the Quit dialog (fr_cq.png) |
| 3 | font_family=NoSuchFont + font_size=16 | PASS | window 1560x936 = 120x36 cells of 13x26 px; font_size=11 gives 1080x648 (9x18), font_size=12 gives 1200x684 (10x19): 16 pt scales as expected (fz11/fz12/fz16.png). Notice reads exactly `settings: font_family "NoSuchFont" is not installed; using the system monospace font at 16 pt  (Esc)` (fz16.png) |
| 4 | resize to a non-multiple of the cell | PASS | XResizeWindow to 1003x607 with the terminal panel open: 111x33 cells + 4 px / 13 px leftover (rz1.png viewed). Bottom leftover rows are the status-bar colour (10,5,18), not the editor background (26,16,40); right leftover column is the panel colour (terminal black) |
| 5 | unfocused window | PASS | _NET_ACTIVE_WINDOW = 0 (nothing focused): hollow outline block on the cursor cell; after XSetInputFocus (active = the window): solid caret (hc_cmp.png: left unfocused, right focused). A second-window variant failed only because Xwayland refuses `GetImage` of a window while another vr-ide window exists, not an app issue |

Not provable here (unit-tested only, no driver): Super chords being ignored, IME composition, the title bar.
No defect found. All processes I started have ended.

## Rule check FR-1
| Rule | Status | Evidence |
|---|---|---|
| FR-1 signals (SIGINT quit path, SIGHUP ignored) | pass | checks 1, 2 |
| FR-1 font fallback at the asked size + notice text | pass | check 3 |
| FR-1 leftover strip fill | pass | check 4 |
| FR-1 hollow unfocused cursor | pass | check 5 |
| BASE-50 | pass | wrote only under qa-artifacts/T4/qc and 05_qc_report.md; no repo edits |

## Sign-off run 6 — after FR-1
HEAD aed370a (FR-1 fixes). Logs in qa-artifacts/signoff/ (test2.log, test3.log, guard2.log).
- Guard: `cargo fmt --all --check` OK; clippy workspace `-D warnings` clean; `clippy -p vr-ide --no-default-features` clean.
- Full suite run 1 (`run-heavy.sh cargo test --workspace -j 4`): FAILED, exit 101: vr-ide lib 1163 passed, 1 failed (`terminal_tests::super_chords_are_not_forwarded_to_the_terminal`, terminal_tests.rs:1994; screen showed "$ AZ" / "vr-42" instead of a line starting "AZ"), cargo stopped there so later crates did not run.
- That test alone: passes (rerun once). Cause is a race in the FR-1 test: it types `echo A` right after opening the terminal without `wait_text("$")` first (the other terminal tests wait for the prompt), so under suite load the keystrokes can reach the shell before it is ready.
- Run 2 (`--no-fail-fast`, whole workspace): see numbers below.
  Run 2: exit 0, 1743 passed, 0 failed, 18 ignored (vr-ide lib 1164 passed, 7 ignored; vr-window 57; other crates as run 1). The flaky test passed this time.
- Flake recorded, not hidden: `super_chords_are_not_forwarded_to_the_terminal` failed once under load, passed alone and in run 2. Suggested fix (developer): add `wait_text(&app, "$")` after `ctrl_backtick` in that test.
Verdict: SIGNOFF_PASS (with the flake above)
