
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
