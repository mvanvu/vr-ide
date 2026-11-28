
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
