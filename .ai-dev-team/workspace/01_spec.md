---
has_ui: true            # TUI: tab label, notices, /ai commands (no web UI packs)
security_sensitive: true  # account folders, tokens, process restarts, embedded web login
approved: user, 2026-11-27 (design + Task 8 login window)
---

# AI usage display and automatic account switch — design

Date: 2026-11-27. Status: approved.

## Acceptance criteria

- AC-1: `ai_auto_switch` (default true) and `ai_auto_switch_prompt` (default "Continue where you left off.") are read from settings; bad prompt → warning, default kept.
- AC-2: Every named claude/codex account's history entry is a link to the default account's history folder; existing content is merged recursively; a clash, a foreign link or a file leaves the account "not shared" with a warning and touches nothing.
- AC-3: A claude session started by vr-ide reports its session id and 5-hour/weekly usage to vr-ide through the statusLine helper.
- AC-4: The tab label shows the usage percent (weekly marked `w` when higher), warning style at ≥ 90 %; a 90 % notice once per window; `/ai usage` lists both windows, reset times and the account.
- AC-5: A limit screen gives one notice per event with the reset time when shown; 100 % alone never ends a process.
- AC-6: With auto-switch on, a limit screen restarts the session in the same tab on the next working, unlimited, shared account with `--resume <its own id>` and types the continue prompt once, unless the user typed since the limit.
- AC-7: No id, no eligible account, auto-switch off (setting or `/ai autoswitch off`): notice only, session untouched; a login screen on the target marks it not working and the next one is tried.
- AC-8: Codex: switch by id only if a reliable id channel exists; else limit notices only, documented.
- AC-9: claude's login opens a vr-ide login window with per-account web data; only allow-listed URLs load; without a display the URL/paste fallback works; removing an account removes its web data.

## Intent

The user runs AI agents (claude, codex) in vr-ide's AI panel and keeps several accounts per tool
(D11: one config folder per account, `CLAUDE_CONFIG_DIR` / `CODEX_HOME`). When an account hits its
usage limit mid-task, work stops until the user notices, picks another account and finds the
conversation again. They want:

1. To see how much of the limit the running agent's account has used, and to be warned before
   and when it runs out.
2. By default, when the running agent hits its limit, vr-ide switches that session to another
   account of the same tool and **continues the same conversation**, unless the user has told it
   not to.

## Goals

- Per session: the account's usage (percent of the 5-hour and weekly windows, and the reset time)
  where the tool exposes it; a notice when it crosses a warning threshold and when the limit is hit.
- Auto-switch on limit, on by default, with a global setting and a per-session command to turn it
  off/on.
- The switched session resumes the exact conversation (same session id) under the new account.
- Accounts of one tool share conversation history (chosen approach, below); logins/tokens stay
  per account.

## Non-goals

- Reading the tools' private APIs or calling Anthropic/OpenAI usage endpoints over the network.
- Usage for tools without accounts (gemini, aider, cursor-agent): only limit detection by screen
  text, no switch.
- Balancing load across accounts before a limit is hit.

## Anti-goals (failure even if it "works")

- Switching to an account that is itself limited, or looping between limited accounts.
- Losing or duplicating the user's prompt / the agent's in-flight work, or resuming a different
  conversation than the one that was running.
- Account tokens of one account ending up readable through another account's folder.
- Typing into an agent the user is actively typing in.

## Approaches considered

1. **Shared history folder for all accounts of a tool (chosen).** Each account folder's history
   folder (`projects/` for claude, `sessions/` for codex) is a link to one shared folder; a switch
   restarts the tool on account B resuming the same session id. Simple, robust to tool updates,
   `/resume` lists conversations of every account. Trade-off: history is no longer separated
   between accounts (tokens still are).
2. Copy only the running session's transcript into account B on switch. Keeps history separate,
   but vr-ide must read inside account folders (forbidden by SEC rules today) and depends on the
   tools' internal file layout.
3. Option 1 opt-in per account (share groups). More flexible, more configuration and cases.

The user picked 1 on the recommendation that accounts exist for extra quota, not for hiding
history between the user's own accounts.

## Decided parts of the how

### Shared history

- Shared folder: the **default account's** history folder (`~/.claude/projects`, `$CODEX_HOME`
  default `~/.codex/sessions`), so the Default account and every named account see one history.
  The user's own default config is not otherwise touched.
- On account creation, and once for existing accounts (on first use after upgrade), vr-ide makes
  `<account>/projects` (claude) or `<account>/sessions` (codex) a symlink to the shared folder.
  An existing real folder with content is migrated with a **recursive merge** (designer's call
  after review: claude's per-project folders like `-home-rainy-DEV-vr-ide` and codex's
  `YYYY/MM/DD` folders clash by design, so renaming a top-level entry would hide those
  conversations): directories are merged level by level, files are moved into the matching
  shared directory; a file name that already exists there is left in place in the account folder
  (conversation files are id-named, so this is a near-impossible case) and the account is then
  marked "not shared" with a warning instead of being linked. When everything moved, the emptied
  folder is replaced by the link. This one-time migration is the only time vr-ide walks inside an
  account folder, and only inside that one history entry. Failure leaves the account usable without sharing and shows a
  warning; auto-switch to/from that account is then off.
- Only that one entry inside an account folder is created/replaced; nothing else inside account
  folders is opened, listed or copied. This narrows the SEC-16/T-20 rule: update the threat model
  (`.ai-dev-team/workspace/02_threat-model.md`, T-23 — "`--continue` resumes another account's
  conversation" becomes intended for the shared folder) and document it in the README.

### Usage data (claude)

- vr-ide starts claude with `--settings <file>` (a vr-ide-owned JSON file, 0600, in the state
  folder) that sets `statusLine` to a command running the vr-ide binary itself in a helper mode
  (`vr-ide --ai-statusline <session file>`, absolute path, no shell).
- The helper reads the status JSON claude sends on stdin (fields include `session_id`,
  `rate_limits.five_hour` / `seven_day` with `used_percentage` and `resets_at`), writes a bounded,
  sanitized copy atomically to its per-session file (0600, random name), and prints a compact
  status line for claude to show (model, 5h %).
- vr-ide reads that file on its existing refresh tick. It gives the session's **conversation id**
  (needed to resume) and the usage numbers.
- While claude runs inside vr-ide, this replaces a statusLine the account may have configured
  (the account's settings are not read).

### Usage data (codex) and limit detection (all tools)

- Limit hit is detected from the session's screen text with a fixed per-tool list of patterns
  (claude: "limit reached", "hit your session limit", …; codex: "You've hit your usage limit", …),
  matched on the last screen lines only, plus `used_percentage >= 100` where known.
- Codex usage percent: shown only if codex exposes it in a form vr-ide can read without opening
  its files; otherwise codex shows limit hits only. The builder verifies against the installed
  codex and records the finding.
- Codex conversation id (designer's call after review): auto-switch for codex is on **only if**
  the builder finds a reliable way to learn the running session's id without opening codex's
  files (and resume it by id). `codex resume --last` is not used: with shared history it can pick
  another tab's or another account's conversation. Without a reliable id, codex sessions get usage
  display / limit notices only, and the README says so.

### When a limit counts as hit (designer's call after review)

- A **limit event** is the tool showing its limit-reached message on screen (the tool has stopped
  working). `used_percentage >= 100` alone never ends a process: with extra usage / overage the
  tool keeps working past 100%, so it only drives the warning colour and notice.
- No conversation id known yet (e.g. the limit hits on the first prompt, before any status
  report): no switch, only the limit notice. `--continue` is never used for a switch.

### Display

- AI tab label gets a usage suffix when known: `1 claude@work 63%` (5-hour window; weekly shown
  instead when it is the higher one), coloured warning at ≥ 90%.
- Notices: once when a session crosses 90% of a window; once on limit hit (with reset time).
- `/ai usage` shows, for the active session, both windows, reset times, and the account.

### Auto-switch

- Setting `ai_auto_switch = true` (default) in `settings.toml`. Per session:
  `/ai autoswitch off|on` (shown in the tab label as a marker when off).
- On limit hit, if auto-switch is on for the session:
  1. Pick the next account of the same tool, in the account list order after the current one,
     wrapping, including Default; skip accounts known limited (last seen limit with a `resets_at`
     in the future, remembered in vr-ide state per account), accounts without shared history, and
     accounts **not known to work** (designer's call after review): an account is known to work
     once vr-ide has received a status report from the tool running on it (claude) or seen it
     finish a turn; this is remembered per account in vr-ide state. An account never used in
     vr-ide is therefore not a target until it has been used once by hand. If the tool started on
     account B shows a login screen instead (screen patterns), B is marked not working, the
     continue prompt is not typed, and the next account is tried (each account at most once per
     limit event).
  2. None left: notice "all <tool> accounts are limited until <earliest reset>"; no switch.
  3. Otherwise: end the current process (the existing close path, process group), restart the
     tool in the same tab with account B and the resume arguments for the conversation id
     (claude `--resume <id>`), keep the tab, name, folder, change tracker; release/take account
     locks accordingly. Notice "switched claude from work to personal (work limited until 15:00)".
  4. After the resumed tool is ready, type a short continue prompt
     (`ai_auto_switch_prompt`, default "Continue where you left off.") and Enter — only if the user
     has not typed into the session since the limit was detected.
- At most one switch per session per limit event; a second limit within a short window on the new
  account is treated as a new event only after the tool has run a turn.

### Login inside vr-ide (added 2026-11-27, approved by the user)

Problem: adding a claude account opens the system browser, which is already logged in to
another account through its cookies; the user has to switch or quit browsers for every account.

- vr-ide sets `BROWSER` for the claude process (claude reads it to open the login URL) to the
  vr-ide binary in a helper mode: `vr-ide --login-window <tool> <account>` (absolute path, argv).
- The helper opens a vr-ide login window with an embedded web view (`wry` + `tao`; WebKitGTK on
  Linux, WKWebView on macOS, WebView2 on Windows) showing the URL. Each account has its own web
  data folder `<state>/ai-login/<tool>-<account>/` (0700), so cookies never mix between accounts or
  with the user's browser, and a later re-login of the same account is quick.
- Only `https://claude.ai/`, `https://*.anthropic.com/`, the identity providers the login page
  uses (Google, Apple), and the `http://localhost:<port>/` callback of the local claude process
  are navigable; anything else opens nowhere (logged as a notice). The window closes itself after
  the localhost callback loads.
- No display (SSH, `DISPLAY`/`WAYLAND_DISPLAY` unset on Linux) or web view unavailable: the helper
  exits non-zero, claude falls back to showing the URL and "Paste code here", and vr-ide shows a
  notice explaining it.
- Removing an account removes its web data folder too.
- Build: the web view is behind a cargo feature `login-window`, on by default; Linux needs
  `libwebkit2gtk-4.1`. Documented in README requirements.

## Left to the builder

- Exact screen patterns per tool (from the installed binaries' strings and real output), and the
  "ready" signal before typing the continue prompt.
- Threshold constant (90%), the label format details, where the per-account limit memory lives
  (vr-ide state folder, 0600).
- Codex usage/id findings (above).
- Migration edge cases (empty folder, link already correct, link pointing elsewhere → refuse and
  warn).
- Tests: unit tests for pattern matching, account choice, migration; integration tests with fake
  agent scripts as in `ai_tests.rs`.
