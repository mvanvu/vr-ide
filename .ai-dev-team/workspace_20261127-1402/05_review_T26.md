# Review T26 (D296: terminal tab 'x' asks while a program runs)

Verdict: APPROVED (2 minor notes, no change required).

## Files read
README.md read; docs/keymap.md read; crates/vr-term/src/terminal.rs read; crates/vr-ide/src/commands/dialog.rs read; crates/vr-ide/src/commands/terminal.rs read; crates/vr-ide/src/terminal_tests.rs read; snapshot render_close_terminal_dialog_80x24.snap read (via test); .ai-dev-team/* docs skipped (process files).

## Findings
- minor, terminal.rs `close_tab_asking`/`shell_in_foreground`: `exec prog` in the shell makes prog the session leader, so pgid == pid and the 'x' closes at once without asking (live: `exec sleep 300` then x -> closed, no dialog). Inherited T20 semantics, indistinguishable by tcgetpgrp; accept and optionally note in docs.
- minor, terminal.rs:134-139 `foreground_name`: /proc comm is kernel-bounded (<=15 bytes, one short read), but only trimmed. A program can set comm (prctl) with an interior newline; ratatui drops control chars at buffer level (no escape injection), but the message may mis-wrap. Cosmetic; optional `.chars().filter(|c| !c.is_control())`.
- info: `answer_close` matches by `t.pid() == pid`; None never occurs for a real tab (pid is kept after exit), a fake tab with None could match the first fake tab only in tests.

## Checks
- foreground_pgid: additive (new method, shell_in_foreground untouched), tcgetpgrp via master, None for unknown/<=0: correct.
- Close by pid: tab reordered/closed while open resolves by pid; gone = no-op. Live: cat killed while dialog open then Close -> panel hidden, no stray sh; shell killed (tab exited) while dialog open then Close -> clean, vr-ide alive. 4/4 (review/walk.py). Dev walk 6/6 reported, not re-run.
- Dialog: "Close terminal?", "<name> is running; closing the terminal ends it.", [Close] [Cancel], Cancel selected, Esc = Cancel (live + test).
- No key changes (mouse-only x, D291). README + keymap updated.
- cargo test -p vr-ide terminal_tests: 44 passed. cargo test -p vr-term: all ok.

## Rule check T26
| Rule | Result | Evidence |
| --- | --- | --- |
| D296 ask only when a non-shell program holds foreground | pass | terminal.rs close_tab_asking; test x_asks..., x_closes_an_idle...; live walk |
| Idle shell / exited closes at once | pass | test + dev walk_06 |
| Unknown -> ask | pass | snapshot test (fake running tab) |
| Cancel default, Esc cancels | pass | test + live |
| Close by pid, safe if tab gone/reordered | pass | answer_close; live edge walks |
| foreground_pgid additive/correct | pass | vr-term diff |
| /proc comm bounded, sanitized for display | pass (minor) | bounded; control chars filtered by ratatui; interior newline not stripped |
| No key changes | pass | diff |
| Docs (README, keymap) | pass | diff |
| Tests / design QA | pass | 44 + vr-term ok; screens in qa-artifacts/T26/review/ |
Counts: pass 10, fail 0.
