# Decisions — run 5

Taken by the leader while the user is away (presence: away since 22:24). Recommended option each time.

- D1 (TM C1, 22:34): the claude shared folder follows an inherited `CLAUDE_CONFIG_DIR` of vr-ide's own environment (as codex follows `CODEX_HOME`): `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects`. Why: the shared folder must be the Default account's real history. Tasks: T2. confirm-on-return.
- D2 (TM C2, 22:34): the statusLine command runs through claude's shell; SEC-22 quoting is the control (spec said "no shell"). Task: T3.
- D3 (TM C3, 22:34): the session id must be a canonical lowercase UUID (SEC-24), replacing the plan's `[A-Za-z0-9-]{1,64}`; passed as separate argv items `--resume`, `<id>`. Why: option injection (`--dangerously-skip-permissions`). Tasks: T3, T6, T7.
- D4 (TM C4, 22:34): a switch ends account A's process and waits for its exit before B starts (SEC-28); `TermTab::replace` as-is is not used for the switch. Task: T6.
- D5 (TM C5, 22:34): restart and restore resume by the known session id stored in `session.toml` instead of `continue_args` when an id is known (SEC-31); `session.toml` is in T6's scope. Why: with shared history `--continue` can resume another tab's conversation. confirm-on-return.
- D6 (TM C6, 22:34): the login helper opens only allow-listed claude login start URLs in the vr-ide login window; any other URL it receives (an agent's `xdg-open`, `gh auth login`, MCP sign-in) is handed unchanged to the system opener (`xdg-open` / `open` / `start`), as without vr-ide. Why: keeps today's behaviour for everything that is not the claude login; the login window's own navigation stays allow-listed. Task: T8. confirm-on-return.
- D7 (TM C7, 22:34): persistent per-account web data stays (approved by the user: quick re-login); T-44 recorded as accepted risk, listed in the report. confirm-on-return.
- D8 (TM C8, 22:34): the continue prompt is typed only on a positive "ready" match with no dialog on screen (trust, login, API key, summary choice, permission) — SEC-30. Task: T6.
- D9 (T2): a clash, link or bound found anywhere stops the migration before anything moves (check pass first); a race found in the move pass stops there and leaves each file in exactly one place. Why: an unshared account keeps seeing all of its conversations.
- D10 (T2): the link target is the canonical shared path; an existing link is accepted only when byte-equal to it (SEC-19). If the user later moves `~/.claude` behind a symlink, accounts report "links elsewhere" until relinked by hand. confirm-on-return.
- D11 (T2): the shared folder is computed once at startup (`history::configure`) from vr-ide's own `$HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`; no `$HOME` → nothing shared, no warning.
- D12 (T2): a named account is linked on every session start (spawn_session), not in `accounts::create`; `restart` does not retry.
