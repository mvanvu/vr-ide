
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
