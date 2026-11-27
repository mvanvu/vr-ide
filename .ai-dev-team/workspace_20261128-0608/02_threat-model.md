# Threat model: run 5 (shared history, statusLine helper, auto-switch restart, login web view)

Task TM. Inputs: `01_spec.md` (AC-1..AC-9, "Login inside vr-ide"), `02_plan.md` (Tasks 1-8, Global
Constraints), the run-4 model `../workspace_20261127-2224/02_threat-model.md` (T-1..T-23, SEC-1..SEC-18),
code: `crates/vr-ide/src/ai/{accounts,spawn,mod,attention}.rs`, `commands/terminal.rs`
(`TermTab::replace`, `close_later`), `cli.rs`, `main.rs`, `vr-config/src/paths.rs`. Installed: Claude Code
2.1.332, codex-cli 0.144.1. Numbering continues the run-4 model (threats T-24.., requirements SEC-19..) so
the amended run-4 items keep their IDs.

Setting: local single-user TUI; no listener of vr-ide's own; the login helper opens an embedded web view
to Anthropic / IdP sites. Automated abuse (bots, crawlers, AI agents calling an API): not applicable — no
public flow. The agent itself (claude/codex, possibly steered by prompt injection from repo content) is
treated as an untrusted same-uid program (A2).

Attackers (run-4 A1-A5 kept; added):
- **A6 agent-controlled output**: screen text and status JSON the tool prints, plus anything the agent's
  shell commands print or write (it runs as the user, sees its own env, `--settings` path in its argv, the
  report path in the settings file, and `BROWSER`).
- **A7 web content**: pages loaded in the login window (claude.ai, IdPs, anything they redirect to, a
  malicious URL handed to `$BROWSER` by any child of claude).
- **A8 inherited environment**: vr-ide started from a shell that already carries `CLAUDE_CONFIG_DIR`,
  `CODEX_HOME` or `BROWSER` (e.g. launched from inside an agent tab).

## Assets
| ID | Asset |
|----|-------|
| AS-7 (run 4, widened) | Account folders: tokens per account, and which account a session runs on. |
| AS-8 | The user's conversation history: `~/.claude/projects`, `$CODEX_HOME/sessions` (now the shared pool) and each account's pre-migration `projects/`/`sessions/` — loss or mis-move is permanent data loss. |
| AS-9 | Agent permission model: the flags the agent starts with (an injected `--dangerously-skip-permissions` is privilege escalation inside the agent). |
| AS-10 | Web login state: claude.ai / Google / Apple session cookies per account in `<state>/ai-login/…`, and OAuth authorization codes passing through the window. |
| AS-11 | In-flight agent work and the transcript file of the running conversation (a restart kills the process; two writers corrupt the transcript). |
| AS-12 | Truthful UI and input integrity: usage/limit notices, which account a tab runs on, and that vr-ide only types where the user would have. |
| AS-1 (run 4) | Code execution as the user (helper command lines run by claude's shell, `BROWSER`). |
| AS-6 (run 4) | Disk / responsiveness (report polling, restart loops, migration walk, helper processes). |

## Trust boundaries & data flows
| ID | Flow |
|----|------|
| F9 | `history::ensure`: account folder `<state>/accounts/<n>/projects|sessions` ↔ shared folder (`~/.claude/projects`, `$CODEX_HOME/sessions`): lstat, create link, one-time recursive merge (rename) of an existing real folder. |
| F10 | vr-ide → `<state>/ai-usage/<id>.json` settings file → `claude --settings <file>` → claude runs the `statusLine.command` **through its shell** → `vr-ide --ai-statusline <report>` reads claude's JSON on stdin → report file → vr-ide refresh reads it (session id, usage). |
| F11 | Session screen text (last lines) → limit / login / ready patterns → per-account state file `<state>/ai-account-state` → notices. |
| F12 | Limit event → `switch::on_limit`: pick account B, locks, end A, start B with `--resume <id>`, type the continue prompt. Also restart (Enter on exited) / restore (`again_mode` = `--continue`). |
| F13 | Codex id/usage channel (only if a reliable one exists, Task 7). |
| F14 | claude (or any of its children) runs `$BROWSER <url>` = `vr-ide --login-window claude <account> <url>` → tao/wry window with data dir `<state>/ai-login/…` → navigations → `http://localhost:<port>/callback` of claude. |
| F15 | Account removal → account folder (now holding a link to the shared folder) + web data folder. |

## Threats
| # | Flow | Threat | Cat. | L | I | Sev. | SEC |
|---|------|--------|------|---|---|------|-----|
| T-24 | F9 | Migration TOCTOU: while vr-ide walks `accounts/<n>/projects/sub`, a same-uid program (the agent on that account, A6) swaps `sub` (or a shared-side dir) for a symlink to `~/.ssh` / `~/` → path-based `rename` moves the user's real files into the shared history or moves history into `~/…`; files "disappear" from where the user keeps them. | T | L | H | high | SEC-19, SEC-20 |
| T-25 | F9 | Clash check then `rename` → a shared-side file created meanwhile (claude on Default writing the same id) is silently **overwritten** (rename replaces); copy-then-delete across filesystems (EXDEV) deletes the source after a partial copy; a crash mid-merge leaves files nowhere. | T | L | H | high | SEC-20 |
| T-26 | F9 | Link target wrong: shared dir computed from an inherited `CLAUDE_CONFIG_DIR`/`CODEX_HOME` that points into `<state>/accounts/x` (A8) → account linked to another account's or its own entry (loop), or history of account A lands inside account B's 0700 folder; `~/.claude/projects` group/world-readable (or owned by someone else) → named accounts' conversations, private before, become readable by others; replacing a symlink `projects → elsewhere` deletes/loses that target. | I/T | L | M | medium | SEC-21, SEC-38 |
| T-27 | F9 | Migration while the account's claude runs (this or another window): new files appear after the walk; `remove_dir_all`-style cleanup or a non-atomic "empty then replace" deletes them. Unbounded tree (A2 floods `projects/` with 10⁶ entries) freezes the UI thread. | T/D | L | M | medium | SEC-20 |
| T-28 | F10 | statusLine command is run by claude's shell: an exe or report path with `'`, `"`, `$`, `` ` ``, `;`, space or newline → wrong program or command injection. `current_exe()` ending in ` (deleted)` after an upgrade. | E | L | H | medium | SEC-22 |
| T-29 | F10 | Settings/report files: predictable names pre-planted as symlinks (write through to `~/.bashrc`), world-readable modes, written non-atomically (torn read), never cleaned (one per session forever), swept while another window's session still uses them. The helper given an arbitrary path (`vr-ide --ai-statusline ~/.bashrc`, any caller) replaces that file. | T/I/D | L | M | medium | SEC-22, SEC-23 |
| T-30 | F10 | Crafted status JSON / forged report (A6: the agent writes the report file or runs the helper itself): session id `--dangerously-skip-permissions` (passes the plan's `[A-Za-z0-9-]{1,64}`) lands in argv as `--resume <id>` → claude/codex parse it as an option → agent restarts with permissions bypassed; NaN/1e308 percentages, `resets_at` in year 9999, 10 MiB JSON, control/escape chars in `model.display_name` echoed into claude's status line and vr-ide's label. | E/T/D | M | H | high | SEC-23, SEC-24, SEC-25 |
| T-31 | F10 | A forged but well-formed id (another conversation of the same user) → the switch resumes a different conversation than the one that ran. | T | L | M | low | SEC-24 (accepted remainder) |
| T-32 | F10 | Account `statusLine` or a repo's `.claude/settings*.json` overriding vr-ide's (if precedence is not as assumed) → no reports, or a project-defined command runs; `--settings` content broader than `statusLine` changes the agent's permissions. | E/T | L | M | medium | SEC-22 |
| T-33 | F11 | Spoofed limit (A6/A1): the agent `cat`s a README, prints a test log, or the user pastes text containing "usage limit reached … resets 3pm" in the last 12 lines → healthy session killed mid-turn (work lost), account marked limited (until a crafted far-future reset = permanent DoS of that account), auto-switch storm through all accounts. | S/D/T | M | M | medium | SEC-26, SEC-27, SEC-29 |
| T-34 | F11 | State file `ai-account-state`: two windows read-modify-write → lost updates; tampered keys (`../x`, unknown tool) or huge file; `works` set by a forged report. | T/D | L | L | low | SEC-27 |
| T-35 | F12 | Restart overlap: `TermTab::replace` (commands/terminal.rs:312) starts B **before** A is ended (`drop_later(old)` runs off-thread), so A and B both write the same transcript `<id>.jsonl` in the shared folder → corrupted or forked conversation; A still running its last tool call while B re-runs it. | T | H | H | high | SEC-28 |
| T-36 | F12 | Locks during a switch: B removed by another window between choice and start (hold refused after A is already ended → tab dead); A's lock released before A's process is gone; B started without `ready()` (folder now a symlink / other tool / not shared). | T | L | M | medium | SEC-28 |
| T-37 | F12 | Continue prompt typed into the wrong place: B shows the folder **trust dialog** (trust is per config dir, so a "works" account may never have trusted this folder), the API-key approval, "resume from summary", onboarding/theme picker, a permission prompt or a login screen → the typed text + Enter accepts the default choice (trusting an untrusted repo, approving a tool call); written with `send()` (writes the **active** tab) after the user changed tabs → text typed into another agent or a shell; typed while the user is typing. | E/T | M | H | high | SEC-30 |
| T-38 | F12 | Restart loops / resource use: limit → B → limit (spoofed or real) → C …; a session that crashes on start re-triggers; restarts of exited sessions; each restart spawns processes and helper processes. | D | M | M | medium | SEC-29, SEC-39 |
| T-39 | F12 (amends T-23) | With one shared history, `--continue` (restart via `again_mode`, restore from `session.toml`, codex `resume --last`) picks the most recent conversation **of any account and any tab** in that folder: two restored tabs (work, personal) in one folder continue the same conversation (two writers, T-35) or the wrong one. | T | M | M | medium | SEC-31 |
| T-40 | F13 | Codex id from a heuristic (`resume --last`, newest rollout file, screen text) → wrong conversation; reading codex's files contradicts SEC-16; `-c notify=` program string handed to a shell. | T/I | M | M | medium | SEC-32 |
| T-41 | F14 | `BROWSER` value split/evaluated by consumers (xdg-open word-splits, treats `:` as a list, substitutes `%s`; some tools run it via `sh -c`): exe path with space, `:`, `%`, `$`, quotes → wrong program or injection. `BROWSER` inherited by every child of claude (agent shells, `gh auth login`, `npm login`, MCP OAuth, `xdg-open http://localhost:3000`) — their URLs reach the helper. | E/T | L | M | medium | SEC-33, SEC-34 |
| T-42 | F14 | URL allow-list bypass: lookalike `claude.ai.evil.com`, `evilanthropic.com`, `x.anthropic.com.evil.com`, userinfo `https://claude.ai@evil.com/`, `https://claude.ai%2F@evil`, backslash `https://claude.ai\@evil.com`, IDN/homoglyph `сlaude.ai`, trailing dot `claude.ai.`, upper case, non-default port `:8443`, `http://claude.ai`, open redirect on an allowed host to `evil.com`, `javascript:`/`data:`/`file:`/`blob:`/`about:`/`view-source:` URLs, `window.open` popups, form posts, downloads; checking the string vr-ide received instead of the URL the engine actually navigates (parser differential). | S/E/I | M | H | high | SEC-35, SEC-36 |
| T-43 | F14 | Localhost confusion: callback allowed on any port / any `localhost.*` host / `0.0.0.0` / `[::1]` / `127.1` → an auth code or session-bearing redirect delivered to another local service; port taken from a URL other than claude's start URL. | S/I | L | H | medium | SEC-35 |
| T-44 | F14 | Same-uid caller (A6: the prompt-injected agent) runs `$BROWSER` with a genuine claude.ai OAuth URL whose `redirect_uri` is its own localhost port: the window opens with that account's **persistent** cookies, the user clicks Authorize, the attacker receives a code it can redeem (its own PKCE) → a token for the account. | S/I | L | H | medium | SEC-34, accepted remainder |
| T-45 | F14 | Web data isolation: wry/WebKitGTK puts cookies, local storage, IndexedDB or HTTP cache in a shared default (`~/.local/share/<prgname>`, `~/.cache/vr-ide`) → accounts mix, data survives removal; data dir a symlink / wider mode / another user's; two windows for one account share a locked store; a JS bridge (IPC handler, custom protocol, init script, devtools) exposed to web content; helper window never closes (detached from claude, outlives the tab). | I/E/D | M | M | medium | SEC-34, SEC-36, SEC-37 |
| T-46 | F14 | Logging / notices: a refused URL logged or shown with its query (OAuth `code`, `state`, email) or with escape characters. | I/S | M | M | medium | SEC-34 |
| T-47 | F15 | Removing an account follows its `projects` link and deletes the **shared** history; leaves the web data (cookies) behind; deletes the web data while a login window uses it. | T/I | L | H | medium | SEC-37 (+ SEC-17 kept) |
| T-48 | all | Env leakage (A8 and outward): `CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `BROWSER` set with `set_var`, or inherited ones that point into vr-ide's own account folders / helper reused for Default sessions, shells, ssh, `/run`. | I/E | M | M | medium | SEC-38 (+ SEC-18 kept) |

## Automated abuse (scrapers, bots, AI crawlers/agents)
| Flow | Applies? |
|------|----------|
| F9-F15 | No public flow, no listener of vr-ide's own; not applicable. The only network use is the user-driven web login (F14), bounded by SEC-34..SEC-37. |

## Changes to run-4 items
- **T-20 / SEC-16 — amended** by SEC-19: inside an account folder vr-ide may now (a) `lstat`, create, and
  replace the one history entry (`projects` for claude, `sessions` for codex) and (b) walk that entry once
  for the migration, listing names and types and renaming files without opening or reading them. Everything
  else in SEC-16 stays (0700, owner, no symlinked account folder, nothing else inside opened/listed/copied,
  names never logged or shown). The SEC-16 grep check now allows `history.rs` (only `*at` calls on fds rooted
  at the account folder's history entry).
- **T-23 — rewritten** as T-39: "`--continue` resumes another account's conversation" is **intended** for
  accounts that share history; the remaining risk is resuming another *tab's* conversation, handled by
  SEC-31. "Restore never falls back to Default/another account" (SEC-18) stays.
- **SEC-17 — kept, extended** by SEC-37: removal also deletes the account's web data folder; the history
  link inside the account is unlinked, never followed (shared history stays).
- **SEC-18 — kept, extended** by SEC-33 and SEC-38 (`BROWSER` joins the per-spawn variables; inherited
  `CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`BROWSER` that point at vr-ide's own folders/helper are dropped).
- **SEC-8 — kept, one scoped exception**: vr-ide still starts everything as argv; the statusLine command is
  run by claude's own shell, so SEC-22 governs its quoting. The `-c notify=` channel (SEC-32) must be argv.
- **SEC-10 — extended** by SEC-21: the shared history folders join `changes_excluded`.
- SEC-14 (notification text) and SEC-15 (account names; also applies to the helper's `<account>` argument)
  unchanged. SEC-1..SEC-7, SEC-9, SEC-11..SEC-13 unaffected.

## Security requirements for implementation

Tests use temp `HOME` / `VR_IDE_STATE_DIR`, fake agent scripts (as `ai_tests.rs`), no real accounts, no
network. Fd-based means `openat`/`mkdirat`/`renameat2`/`unlinkat`/`symlinkat` on descriptors opened with
`O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` (as `accounts::remove_tree_at`).

### Shared history (T2)
- **SEC-19 (one entry, no-follow) — T2.** `history::ensure` opens the account folder with
  `O_DIRECTORY|O_NOFOLLOW` after `accounts::secure`, and does every operation relative to that fd on the
  single name `projects`/`sessions` (from the fixed tool map, never from input). States: absent →
  `symlinkat(shared, fd, entry)`; symlink whose `readlinkat` equals the expected target byte-for-byte →
  `Linked`; symlink to anything else, regular file, FIFO, socket, device → `NotShared(reason)`, entry left
  exactly as found (never unlinked, never followed); real directory → SEC-20. No other name in the account
  folder is opened, listed or stat'ed. Reasons in notices are fixed texts (no file names).
  Verify: `link_elsewhere_is_refused_and_kept` (link to temp dir with a file → target and link untouched),
  regular file and FIFO entry → `NotShared`, no hang; strace-free grep: `history.rs` uses no path-based
  `rename`/`remove_*`/`read_dir`.
- **SEC-20 (migration is a move, never a loss) — T2.** Before migrating, take the account's lock
  **exclusively, non-blocking** (`accounts/.<n>.lock`, as `remove`); busy (a session on it in any window) →
  `NotShared("in use")`, retried on a later start. The walk is fd-based on both sides: each source level is
  opened `O_NOFOLLOW` relative to its parent fd, each destination level opened/created (`mkdirat` 0700, then
  opened `O_NOFOLLOW`) relative to the shared fd; entries are classified with `fstatat(AT_SYMLINK_NOFOLLOW)`;
  a symlink, FIFO, socket or device anywhere inside → stop, `NotShared`, nothing further moved. Files move
  only with `renameat2(…, RENAME_NOREPLACE)` (macOS: `renameatx_np(RENAME_EXCL)`); `EEXIST` → the file stays
  in the account folder and the result is `NotShared`; `EXDEV` or no no-replace primitive → `NotShared`
  without moving anything (no copy-then-delete). File contents are never opened. Emptied source dirs are
  removed with `unlinkat(AT_REMOVEDIR)` only (fails on non-empty — never a recursive delete); the entry is
  replaced by `unlinkat(entry, AT_REMOVEDIR)` then `symlinkat`; if either fails (claude recreated files) →
  `NotShared`, retried later. The merge is idempotent (a crash leaves each file in exactly one place; the
  next run continues). Bounds: ≤ 200 000 entries and depth ≤ 32, else `NotShared` with nothing moved past
  the bound; runs off the UI thread or before the spawn with the bound enforced.
  Verify: `merge_moves_nested_files`, `name_clash_is_not_shared` (both files byte-equal to before); a test
  seam that swaps a source sub-dir for a symlink to a temp "outside" dir between levels → outside dir
  unchanged, `NotShared`; seam that creates the destination file between check and move → destination
  content unchanged; account lock held by a fake session → nothing moved.
- **SEC-21 (the shared folder) — T2.** Computed from the fixed map only: claude
  `$HOME/.claude/projects` (see contradiction C1), codex `$CODEX_HOME/sessions` else
  `$HOME/.codex/sessions`, where an inherited `CODEX_HOME`/`CLAUDE_CONFIG_DIR` that is inside
  `<state>/accounts/` is ignored (SEC-38). Canonicalised once; refused (`NotShared`, warning naming the
  folder) when the canonical path is inside `<state>/accounts/`, equals the account's own entry, is not
  owned by the user, or has group/other write **or** read bits — vr-ide does not chmod the user's default
  config (spec: "not otherwise touched"); the warning says `chmod 700 <dir>`. Missing → created with
  `create_private_dir` (0700), parents too. The link target is that canonical absolute path. The shared
  folders are added to `app.ai.changes_excluded` (SEC-10). Removing an account leaves the shared folder
  intact (SEC-37 verify).
  Verify: `codex_shared_dir_honours_codex_home`; `CODEX_HOME=<state>/accounts/x` → default path used;
  shared dir 0755 → `NotShared`, mode unchanged; shared dir == entry → refused.

### statusLine helper and report (T3)
- **SEC-22 (settings file and command line) — T3.** `<state>/ai-usage/` created with `create_private_dir`
  (0700) and checked by `accounts::secure`-like rules (real dir, owner). Settings file name
  `<vr-ide pid>-<serial>-<32 hex from getrandom>.json`, created `O_CREAT|O_EXCL|O_NOFOLLOW` 0600, content
  produced by `serde_json` and containing **only** `{"statusLine":{"type":"command","command":…}}` (no
  permissions, hooks, env or other keys). The command is `<exe> --ai-statusline <report>` with each path
  POSIX single-quoted (`'` → `'\''`); the exe is `current_exe()`, must be absolute, an existing regular
  file, not ending in ` (deleted)`, and neither path may contain a control character or newline — else no
  `--settings` is added (usage unknown, one notice). `--settings <file>` is passed as two argv elements.
  Files are removed when the session closes or is replaced; at startup only files whose pid prefix is not
  a live process owned by the user are removed (`unlinkat`, no follow), so other windows' files stay.
  Precedence is verified against the installed claude: a fake project `.claude/settings.json` with its own
  `statusLine` in the root does not replace vr-ide's (record the claude version in the test/notes).
  Verify: exe path `/tmp/a b/'x$y` → quoted command parses back (`sh -c` test prints argv) to the exact
  path; path with `\n` → no `--settings`; settings file mode 0600 with umask 022; JSON has one top key.
- **SEC-23 (helper process) — T3.** `vr-ide --ai-statusline <path>` is recognised only as exactly that form
  (two args) and runs before panic hook, session, config, recovery or terminal setup; it never touches any
  file but the report. The report path must be absolute, its parent a real directory named `ai-usage`
  owned by the user with no group/other bits, and its file name match
  `^[0-9]+-[0-9]+-[0-9a-f]{32}\.report$`; else exit 0 with no output and no write. Stdin read ≤ 64 KiB + 1
  with a 2 s overall deadline (over → nothing written). Only these fields are kept: `session_id` (SEC-24),
  `model.display_name` (sanitized with `safe_text::sanitize`, ≤ 64 chars), `rate_limits.five_hour` /
  `seven_day` `used_percentage` (finite, clamped 0..=1000) and `resets_at` (SEC-25 range); nothing else
  (no `transcript_path`, `cwd`, cost, workspace). Written to a temp `O_EXCL|O_NOFOLLOW` 0600 file in the
  same dir, `fsync`, `rename` (a symlink at the path is replaced). The printed status line is built from
  the kept fields only, control characters removed, ≤ 120 chars. Exit 0 always. No network.
  Verify: `parse_status_rejects_bad_id_and_oversize`; helper with `~/.bashrc`-like path in temp HOME →
  file unchanged; JSON with `\x1b[2J` model name → sanitized; stdin never closed → exits ≤ 2 s.
- **SEC-24 (conversation id into argv) — T3 (validation), T6/T7 (use).** A session id is accepted only as
  a canonical lowercase UUID `^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$` (this replaces
  the plan's `[A-Za-z0-9-]{1,64}`, which admits `--dangerously-skip-permissions`). It is validated in the
  helper, again when vr-ide reads the report, and again right before argv is built; the resume words are
  `tool.args + ["--resume", id]` (codex: `["resume", id]`) as separate argv elements, never via a shell,
  and never combined with `continue_args`. Verify: ids `--dangerously-skip-permissions`, `-x`,
  `abc-123`, uppercase UUID, UUID + `\n` → rejected, no switch; fake agent ARGV dump shows exactly
  `--resume|<uuid>|`.
- **SEC-25 (report read by vr-ide) — T3.** Opened `O_RDONLY|O_NOFOLLOW|O_NONBLOCK`, must be a regular file
  owned by the user, ≤ 4 KiB (the helper writes far less), read only when `(mtime, size)` changed and at
  most once per second per session; every field re-validated as SEC-23/24 (the agent can write this file);
  `resets_at` accepted only within [now − 1 day, now + 35 days], else `None`; a bad or missing file is
  ignored silently (no notice per tick). Values shown in the label and `/ai usage` are numbers and the
  sanitized model; reset times are formatted by vr-ide.
  Verify: report replaced by a symlink to a big file / FIFO → ignored, no hang; `used_percentage: NaN`,
  `1e308`, `resets_at: 253402300799` → clamped / `None`.

### Limits and account state (T5)
- **SEC-26 (limit detection is conservative) — T5.** Patterns are fixed per tool and matched against whole
  lines of the **primary screen** (not scrollback), after trimming the tool's box-drawing border, among the
  last 12 non-empty lines, and only when the session's output has been quiet for ≥ 1 s and the line is
  still present (a real limit stops the tool). A line inside the tool's input box (the prompt the user is
  editing) is not matched. One limit event per session until the tool prints output after the user's next
  input or a restart; at most one limit notice per session per 60 s. Notice text is built from fixed
  strings, the validated account label and the parsed time only — never screen text. The parsed reset time
  is accepted only within [now, now + 8 days]. Verify: limit line followed by more output within 1 s →
  no event; limit text above the last 12 lines → none; limit phrase pasted into the input box → none;
  `resets 3pm` → today/tomorrow 15:00; `try again at 2099-01-01` → event without reset time.
- **SEC-27 (account state file) — T5.** `<state>/ai-account-state`: 0600, atomic (temp `O_EXCL`, fsync,
  rename), read with `O_NOFOLLOW`, regular file, ≤ 64 KiB, read-modify-write under an exclusive `flock` on
  `<state>/ai-account-state.lock` (as `ai-accounts.lock`); keys `tool/account` with tool from the fixed map
  and account `default` or SEC-15-valid, other keys dropped; `limited_until` clamped to ≤ now + 8 days and
  cleared when a validated report for that account shows both windows < 100 %; `works = true` only from a
  validated report carrying a UUID id (or a finished turn) on that account. Garbage → empty map, file
  rewritten only after a successful lock+read. Verify: `state_roundtrip_and_mode_0600`,
  `state_ignores_oversize_or_garbage`, key `claude/../x` dropped, two threads updating different accounts
  → both kept.

### Auto-switch (T6)
- **SEC-28 (switch order: no overlap, locks first) — T6.** Before touching A: pick B; `accounts::hold(B)`
  (Default: none) and `accounts::ready(B)`; `history::ensure(B)` returns `Linked`; any failure → next
  account (each at most once per event). Then end A (process group hang-up as `close`) and **wait for it to
  exit** (≤ 5 s, then the group is terminated forcibly and waited for) before B is spawned — not
  `TermTab::replace` with an already-started B while A is dropped off-thread (commands/terminal.rs:312).
  B failing to start → the tab shows exited with a notice, B's hold dropped, A's hold dropped (A is ended),
  no further attempt. A's hold is released only after A has exited; the session's `account` and
  `account_lock` change together. The same applies to `restart` when a resume id is used (SEC-31).
  Verify: fake A that ignores the hang-up for 1 s and appends to a file → B's first write to that file is
  after A's last; B removed in another "window" (lock held exclusively) → next account used; all fail →
  session exited, notice, no lock left (a later `remove` succeeds).
- **SEC-29 (no loops) — T6.** At most one switch per limit event; each account tried at most once per
  event; at most 3 automatic switches per session per 30 minutes — beyond that auto-switch turns off for the
  session (marker shown) with one notice; a new limit within 60 s of a switch counts as a new event only
  after a validated report or a finished turn on B; an exited session is never auto-restarted. Login screen
  on B → B `works = false`, next account. Verify: fake agent printing the limit line on every account →
  ≤ 3 restarts in the window, then notice-only; `login_screen_target_is_skipped`.
- **SEC-30 (continue prompt only where the user would type) — T6.** The prompt is written to **that
  session's own terminal** (by session serial and the process generation/pid of B), never through
  `send()`/the active tab; if the session was closed, replaced again, or is not running → not written.
  Written only when (a) a positive "ready" pattern of the tool's idle input prompt matches on the primary
  screen after ≥ 1 s of quiet, and (b) no dialog pattern matches: folder trust, login/onboarding/theme,
  API-key approval, "resume from summary"/session choice, permission prompt; (c) no key, paste or mouse
  report reached that session since the limit was detected (including during the restart); (d) once per
  event; (e) within 30 s of B's start — otherwise not typed and a notice "type to continue" is shown. The
  text is the validated setting (T1: one line, no control characters, ≤ 200) followed by one `\r`; wrapped
  in bracketed paste when B enabled it. Verify: `no_continue_prompt_after_user_input`; fake B showing a
  trust question (`Do you trust the files in this folder?` + options) → nothing typed, notice; user
  switches to another tab during the restart → prompt lands only in B (other fake agent's input log empty).
- **SEC-31 (resume by id, not by "latest") — T6.** With shared history, `--continue` / `resume --last` no
  longer select "this tab's" conversation. When a validated id (SEC-24) is known for a session: `restart`
  (Enter on an exited session) uses the resume words with that id instead of `continue_args`, and the id is
  stored in `session.toml` with the tab (validated on load; invalid → dropped) so restore resumes it.
  Without an id the current `again_mode` behaviour stays (documented in README: may pick the most recent
  conversation of any account in that folder). Never used for a switch: `--continue`, `resume --last`.
  Verify: two tabs (work, personal) in one root with ids X and Y, restore → ARGV `--resume|X|` and
  `--resume|Y|`.

### Codex (T7)
- **SEC-32 (codex id only from a channel vr-ide controls) — T7.** Auto-switch for codex only if the id
  arrives through a channel vr-ide sets up and does not need codex's files (e.g. `-c notify=[…]` naming the
  vr-ide exe in an argv array with a report path under `ai-usage/`, same rules as SEC-22/23, TOML/JSON
  built by a serializer, no shell); id validated as SEC-24; switch argv `resume <uuid>`. Not allowed: newest
  rollout file, `resume --last`, screen-scraped ids, reading `$CODEX_HOME`. The finding (codex 0.144.1,
  command, fields) is recorded; otherwise codex is notice-only (SEC-26 applies). Overriding the user's own
  `notify` setting is stated in the README. Verify: `codex_limit_notifies_without_switch` or
  `codex_switch_resumes_by_id` with id `--dangerously-bypass-approvals-and-sandbox` rejected.

### Login window (T8)
- **SEC-33 (`BROWSER` value and scope) — T8.** `browser_env` builds `<exe> --login-window <tool> <account>`
  where tool ∈ {claude}, account is `default` or SEC-15-valid, and the exe (`current_exe()`, absolute,
  regular, not ` (deleted)`) contains only `[A-Za-z0-9/._+-]` — no space, `:`, `%`, quote, `$`, backtick,
  backslash or control character (consumers word-split it, treat `:` as a list and `%s` as a placeholder);
  otherwise `BROWSER` is not set (claude prints the URL / paste fallback) and one notice says why. Set only
  in that claude spawn's env (SEC-18): never `set_var`, never for codex/other tools, shells, ssh or `/run`;
  built with feature `login-window` off → not set. Verify: `claude_gets_browser_env`; exe in
  `/tmp/a b/` or `/tmp/a:b/` → no `BROWSER`, notice; a shell tab opened after → no `BROWSER` from vr-ide.
- **SEC-34 (helper process) — T8.** Recognised only as exactly `--login-window <tool> <account> <url>`
  (4 args; tool/account as SEC-33; url ≤ 8 KiB, no control characters), handled before any TUI/session/
  recovery setup. The **start URL** must itself pass SEC-35 as https (else exit 1, no window); the callback
  port is taken only from its `redirect_uri` query parameter, which must be `http://localhost:<port>/…` or
  `http://127.0.0.1:<port>/…` with 1024 ≤ port ≤ 65535 (otherwise no localhost URL is allowed at all). One
  window per account: exclusive non-blocking `flock` on `<state>/ai-login/<tool>-<account>.lock` (busy →
  exit 1). The window title always names the tool and account. Lifetime ≤ 15 min, then exit 1; closes and
  exits 0 after the callback page loads. No display (`DISPLAY`/`WAYLAND_DISPLAY` unset on Linux), feature
  off, or web view error → exit 1 with no window. A refused navigation is logged/shown as scheme + host
  only (sanitized, never path/query — they carry OAuth `code`/`state`), at most one line per second.
  Verify: `no_display_exits_1`; cli `login_window_flag_form` (3 or 5 args, `../x` account → refused);
  start URL `https://evil.com/` → exit 1, no window; refused URL with `?code=SECRET` → log has no `SECRET`.
- **SEC-35 (URL allow-list) — T8.** `allowed(url, port)` parses with the `url` crate (WHATWG, IDNA to ASCII)
  and accepts only: scheme `https`, no username/password, port absent (443), host (lower-case ASCII,
  no trailing dot, not an IP literal) exactly `claude.ai`, or ending in `.anthropic.com` (dot-anchored;
  the apex `anthropic.com` only if the builder finds the flow needs it), or exactly `accounts.google.com`,
  `appleid.apple.com` (any further IdP host only after the manual check shows the flow needs it, listed in
  code with the reason); or scheme `http`, host exactly `localhost` or `127.0.0.1`, port == the SEC-34
  callback port. Every other scheme is refused (`javascript:`, `data:`, `file:`, `blob:`, `about:`,
  `view-source:`, `ws:`, …). Verify (`allowed_urls` table): accepted — `https://claude.ai/login`,
  `https://CLAUDE.AI/x`, `https://console.anthropic.com/oauth`, `https://accounts.google.com/o/oauth2`,
  `http://localhost:54545/callback?code=x` with port 54545; refused — `http://claude.ai/`,
  `https://claude.ai.evil.com/`, `https://evilanthropic.com/`, `https://x.anthropic.com.evil.com/`,
  `https://claude.ai@evil.com/`, `https://u:p@claude.ai/`, `https://claude.ai:8443/`, `https://claude.ai./`,
  `https://xn--laude-3ve.ai/` / `https://сlaude.ai/`, `https://evil.com/?r=https://claude.ai/`,
  `http://localhost:1/` (wrong port), `http://localhost.evil.com:54545/`, `http://0.0.0.0:54545/`,
  `http://[::1]:54545/`, `http://127.1:54545/`, `javascript:alert(1)`, `data:text/html,x`,
  `file:///etc/passwd`, `about:blank`, `blob:https://claude.ai/x`.
- **SEC-36 (enforcement in the web view) — T8.** The check runs on the URL the engine is about to load, for
  **every top-level navigation**: initial load, link clicks, form posts, JS `location` changes and server
  redirects (verify on WebKitGTK that redirects reach the handler; if not, block redirect targets with the
  engine's policy API); a refused navigation is cancelled (the page stays). New-window requests: allowed
  URL → loaded in the same view, else refused. Downloads refused. No IPC handler, no custom protocol, no
  initialization script, no `file://` access, devtools only in debug builds, no clipboard/permission grants
  (camera, mic, geolocation, notifications) — all permission requests denied. Sub-resources (scripts,
  iframes such as captcha) are not filtered; this is documented. Manual check in the task notes: log in to
  two accounts (the second gets a clean login page); start the helper with an allowed start URL whose
  `redirect_uri` port serves a local test page that redirects to `https://example.com` and calls
  `window.open("https://example.com")`: both refused, the window stays on the test page.
- **SEC-37 (web data folder) — T8 (+ SEC-17).** `data_dir` = `<state>/ai-login/<tool>-<account|default>`,
  account SEC-15-valid; `ai-login/` and the folder made with `create_private_dir` (0700) and checked with
  `accounts::secure` before each use (real dir, owner; a symlink refused). **All** website data — cookies,
  local storage, IndexedDB, service workers and HTTP cache — is configured under that folder; verify after a
  manual login that nothing new appeared in `~/.local/share/vr-ide*`, `~/.cache/vr-ide*` or
  `~/.local/share/webkitgtk` (if the engine cannot place the cache there, it is disabled). `accounts::remove`
  also deletes `ai-login/<tool>-<name>` with the fd-based no-follow delete (`remove_tree_at`), never to the
  trash, refused while the window's lock is held (busy → whole removal refused before anything is deleted);
  the remove dialog says the web sign-in is deleted locally and stays valid at the provider. Removal unlinks
  the account's `projects` link and never enters it. Verify: `data_dir_per_account_0700`,
  `remove_account_removes_login_data`; account with `projects → <shared>` removed → shared folder and its
  files intact; `ai-login/claude-work` a symlink to a temp dir → refused, target intact.

### Environment and resources (T2, T3, T8)
- **SEC-38 (env scope, inherited values) — T2 (accounts), T8 (`BROWSER`).** `CLAUDE_CONFIG_DIR`,
  `CODEX_HOME` and `BROWSER` are added only to the spawn env of the agent session that needs them
  (SEC-18). An inherited `CLAUDE_CONFIG_DIR`/`CODEX_HOME` whose canonical path is inside
  `<state>/accounts/`, or an inherited `BROWSER` containing `--login-window`, is removed from the env of
  every child vr-ide starts (Default agents, shells, ssh, `/run`) and not used for SEC-21 — one notice at
  startup. Verify: start vr-ide with `CLAUDE_CONFIG_DIR=<state>/accounts/work` → Default fake agent env
  has no such variable; shell tab env lacks it; a Default fake agent with an unrelated inherited
  `CLAUDE_CONFIG_DIR=/x` keeps `/x`.
- **SEC-39 (bounded work) — T3, T6.** Report polling is one `fstatat` per session per refresh, a read at
  most once per second per session (SEC-25); ≤ 2 files (settings, report) per live session; the startup
  sweep is bounded to 1 000 entries; restarts bounded by SEC-29; helper processes bounded by claude's own
  statusLine rate and the 2 s deadline (SEC-23); the migration by SEC-20; login windows one per account
  (SEC-34). Verify: 20 fake sessions idle for 10 s → no report reads without changes (counter seam).

## Accepted risks
- Same-uid programs (including the agent on any account) can read every account's tokens, the web cookies
  in `ai-login/`, and forge reports; SEC-23..SEC-25 keep a forged report from escalating (no option
  injection, bounded values), but a well-formed foreign id can still make a switch resume another of the
  user's own conversations (T-31).
- T-44: a same-uid caller can open the login window for an account with a genuine claude.ai OAuth URL and a
  localhost redirect of its own; the user sees the window titled with the account and must approve. Same
  uid can already read the account's token file, so the gain is small; not further mitigated.
- A limit message that matches all SEC-26 conditions but is not real (an agent deliberately printing the
  exact tool message and going quiet) can still cause ≤ 3 restarts per 30 minutes per session; the resumed
  conversation is the same one, but the in-flight turn of A is lost.
- Shared history makes every account's conversations visible to every account of that tool and to `claude
  --continue`/`/resume` (user's choice, approach 1).
- Web sign-ins stay valid at the provider after removal (local deletion only), like SEC-17 tokens.
- Sub-resources in the login window are not allow-listed (SEC-36).

## Contradictions / gaps against the approved spec and plan (for the leader; spec unchanged)
- **C1** Spec/plan fix claude's shared folder at `$HOME/.claude/projects`, while the Default account runs
  with the inherited `CLAUDE_CONFIG_DIR` (SEC-18: "leaves the inherited environment as is"). A user with
  `CLAUDE_CONFIG_DIR` set globally gets Default history in `$CLAUDE_CONFIG_DIR/projects`, so the shared pool
  is not the Default account's and switches to/from Default resume ids that are not there. Suggest: claude
  shared = `$CLAUDE_CONFIG_DIR/projects` when inherited (and not inside `<state>/accounts/`), like codex.
- **C2** Spec "statusLine … absolute path, no shell": claude runs statusLine commands through a shell; the
  plan already quotes. SEC-22 makes the quoting the control.
- **C3** Plan's id rule `[A-Za-z0-9-]{1,64}` allows option-like ids → replaced by the UUID rule (SEC-24).
- **C4** Plan Task 6 says "restart in the same tab via `TermTab::replace`": as implemented, replace starts
  the new process before the old one is ended → two writers on one transcript. SEC-28 requires end-and-wait
  first.
- **C5** Spec forbids `--continue` only for a switch; restart/restore still use `continue_args`, which with
  shared history can pick another tab's/account's conversation (spec anti-goal). SEC-31 adds resume-by-id
  for restart/restore (touches `session.toml` — leader to confirm this is in T6's scope).
- **C6** Spec "only allow-listed URLs load; anything else opens nowhere": `BROWSER` is inherited by all of
  claude's children, so agent-run `xdg-open http://localhost:3000`, `gh auth login`, `npm login` and MCP
  server OAuth now open nowhere. Secure as specified; the user may want non-allow-listed URLs handed to the
  system opener instead (a product decision, not taken here).
- **C7** Spec "a later re-login of the same account is quick" (persistent cookies) is what enables T-44;
  accepted above.
- **C8** "Known to work" accounts may still show claude's folder-trust dialog in a new folder (trust lives
  in the per-account config); the spec's "ready" signal must exclude dialogs (SEC-30), else the typed
  prompt + Enter trusts the folder.

## Amendment (run 5, T8, D63/D64)
claude 2.1.332 invokes `$BROWSER` as one program path with the URL as its only argument (execFile, no word splitting). SEC-33: the BROWSER value is the per-account launcher symlink path (whole path checked against `[A-Za-z0-9/._+-]`). SEC-34: the form claude produces is argv[0] = `vr-ide-login.<tool>.<account>` + exactly one URL argument; the explicit `--login-window <tool> <account> <url>` form is used only by vr-ide itself for the detached window. SEC-38: an inherited BROWSER whose file name starts with `vr-ide-login.` or that contains `--login-window` is dropped. The xdg-open hand-off for non-login URLs runs with BROWSER removed.
