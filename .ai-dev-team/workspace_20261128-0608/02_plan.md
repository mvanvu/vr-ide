# AI usage display and automatic account switch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show each AI session's account usage, warn near and at the limit, and by default move a limited claude (and, if reliable, codex) session to another working account, resuming the same conversation.

**Architecture:** Accounts of one tool share the default account's history folder through one symlink per account folder (`ai/history.rs`). Claude reports usage and its session id through a statusLine command that runs vr-ide itself in a helper mode and writes a small JSON report vr-ide reads on its refresh tick (`ai/usage.rs`). Limit and login screens are recognised from the session's screen text (`ai/limits.rs`); a per-account state file remembers which accounts work and until when they are limited. `ai/switch.rs` picks the next account and restarts the tool in the same tab with `--resume <id>`, then types a continue prompt.

**Tech Stack:** Rust (edition 2024), ratatui/crossterm, `vr-term` (alacritty_terminal + portable-pty), serde + toml; adds `serde_json`.

**Spec:** `.ai-dev-team/specs/2026-11-27-ai-usage-and-account-switch-design.md` (approved 2026-11-27). Read it before any task.

## Global Constraints

- Code, comments, docs, commit messages in English; no AI attribution in commits (no `Co-Authored-By: Claude`, no "Generated with", no 🤖).
- Stage files by name only (never `git add -A`, `.`, `-u`).
- Tool names with accounts: `claude` (`CLAUDE_CONFIG_DIR`, history entry `projects`), `codex` (`CODEX_HOME`, history entry `sessions`). The map stays fixed in code (SEC-18).
- Shared history folder = the default account's: `$HOME/.claude/projects`; `$CODEX_HOME` unset → `$HOME/.codex/sessions`.
- Inside an account folder vr-ide only creates/replaces the history entry (and walks it once, for the migration). Nothing else inside account folders is opened, listed or copied.
- Program output (screen text, status JSON) is untrusted: sanitize with `crate::ui::safe_text::sanitize`, cap sizes (status JSON read ≤ 64 KiB).
- Files vr-ide writes for this feature: mode 0600, under the state folder (`app.ai.state_dir`), written atomically (temp + rename).
- Programs are started as argv, never through a shell (SEC-8); the helper is the vr-ide binary by absolute path (`std::env::current_exe()`).
- Settings: `ai_auto_switch = true`, `ai_auto_switch_prompt = "Continue where you left off."`.
- Warning threshold: 90 % of a window.
- `used_percentage >= 100` never ends a process; only a limit screen does. `--continue` / `codex resume --last` are never used for a switch.
- Verification for every task: `cargo test -p vr-ide` (and `-p vr-config` when touched), `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Review Focus

1. Claude running with extra usage past 100 %: no switch, no kill, only the warning colour — test in Task 6 (`no_switch_at_100_percent_without_limit_screen`).
2. Two claude tabs in the same folder, one limited: the switched tab resumes its own id, the other tab is untouched — test in Task 6 (`switch_resumes_own_session_id_with_two_tabs`).
3. The user is typing in the session when the limit screen appears: no continue prompt is typed — test in Task 6 (`no_continue_prompt_after_user_input`).
4. Every other account limited or never used: notice with the earliest reset, nothing restarted — test in Task 6 (`all_limited_notifies_and_keeps_session`).
5. An account folder whose `projects` is a symlink pointing somewhere else (or a file): refused, account marked not shared, never deleted — test in Task 2 (`link_elsewhere_is_refused_and_kept`).

---

### Task 1: Settings

**Files:**
- Modify: `crates/vr-config/src/settings.rs` (struct `Settings`, validation), `crates/vr-config/defaults/settings.toml` (documented keys, after `ai_tools`)
- Test: `crates/vr-config/tests/config.rs`

**Interfaces:**
- Produces: `Settings::ai_auto_switch: bool`, `Settings::ai_auto_switch_prompt: String`.

- [ ] **Step 1: Failing test** `ai_auto_switch_defaults_and_override`: default settings → `ai_auto_switch == true`, `ai_auto_switch_prompt == "Continue where you left off."`; a user file with `ai_auto_switch = false` and `ai_auto_switch_prompt = "go on"` → those values; a prompt with a control character or longer than 200 chars → warning, default kept.
- [ ] **Step 2: Run** `cargo test -p vr-config ai_auto_switch` → FAIL (unknown fields).
- [ ] **Step 3: Implement** the two fields, defaults in `settings.toml` with a comment block (what it does, `/ai autoswitch off` per session), and the prompt check in the existing validation (one line, no control characters, 1..=200 chars).
- [ ] **Step 4: Run** `cargo test -p vr-config` → PASS.
- [ ] **Step 5: Commit** `Add ai_auto_switch and ai_auto_switch_prompt settings`.

### Task 2: Shared history between accounts

**Files:**
- Create: `crates/vr-ide/src/ai/history.rs` (+ `mod history;` in `ai/mod.rs`)
- Modify: `crates/vr-ide/src/ai/accounts.rs` (`create` links the new account; module doc updated), `crates/vr-ide/src/ai/mod.rs` (`spawn_session` / `start_program` ensure the link before starting on a named account), `.ai-dev-team/workspace/02_threat-model.md` (T-20, T-23, SEC-16 wording), `README.md` (accounts section: history is shared, tokens are not)
- Test: `crates/vr-ide/src/ai/history.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub fn entry(tool: &str) -> Option<&'static str>` — `"projects"` / `"sessions"`.
  - `pub fn shared_dir(tool: &str, home: &Path, codex_home: Option<&OsStr>) -> Option<PathBuf>`.
  - `pub enum Shared { Linked, NotShared(String) }`.
  - `pub fn ensure(account_dir: &Path, tool: &str, shared: &Path) -> Shared` — creates `shared` (0700) if missing; link already correct → `Linked`; missing entry → create symlink; real folder → recursive merge then link; anything else → `NotShared(reason)`.

- [ ] **Step 1: Failing tests:**
  - `new_account_gets_link` — empty account dir → `Linked`, `read_link(account/projects) == shared`.
  - `correct_link_is_kept` — second call → `Linked`, link unchanged.
  - `merge_moves_nested_files` — account `projects/-p/a.jsonl` and shared `projects/-p/b.jsonl` → both in `shared/-p/`, account entry is now a link.
  - `name_clash_is_not_shared` — same file name on both sides → `NotShared`, both files still present with their contents, account entry still a real folder.
  - `link_elsewhere_is_refused_and_kept` — entry is a symlink to another dir, or a regular file → `NotShared`, target and file untouched.
  - `codex_shared_dir_honours_codex_home` — `shared_dir("codex", home, Some("/x"))` → `/x/sessions`; `None` → `home/.codex/sessions`; `shared_dir("gemini", ..)` → `None`.
- [ ] **Step 2: Run** `cargo test -p vr-ide history::` → FAIL.
- [ ] **Step 3: Implement** `history.rs`. Merge: walk with `symlink_metadata` (never follow links inside the account entry; a link found inside → `NotShared`), `rename` files into place, create missing dirs 0700, remove emptied dirs bottom-up; only when the entry is empty replace it with the symlink.
- [ ] **Step 4: Wire it**: `accounts::create` calls `ensure` after the folder is made; `start_program` calls it for a named account of `claude`/`codex` and keeps the result on the session (`AiSession::shared: bool`, false when `NotShared`, with one warning notice naming the reason).
- [ ] **Step 5: Run** full checks → PASS. Update threat model + README.
- [ ] **Step 6: Commit** `Share conversation history between accounts of one AI tool`.

### Task 3: Claude usage report (statusLine helper)

**Files:**
- Create: `crates/vr-ide/src/ai/usage.rs`
- Modify: `Cargo.toml` + `crates/vr-ide/Cargo.toml` (`serde_json = "1.0.145"`), `crates/vr-ide/src/cli.rs` (flag `--ai-statusline <file>`, only form with one argument), `crates/vr-ide/src/main.rs` (run the helper before any terminal setup and exit), `crates/vr-ide/src/ai/mod.rs` (`start_program` for `claude`: write the settings file, add `--settings <file>` to the args; `AiSession` fields; `refresh` reads reports)
- Test: `crates/vr-ide/src/ai/usage.rs` tests, `crates/vr-ide/src/cli.rs` tests, `crates/vr-ide/src/ai_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Window { pub used_percentage: f64, pub resets_at: Option<i64> }` (unix seconds)
  - `pub struct Usage { pub session_id: Option<String>, pub five_hour: Option<Window>, pub seven_day: Option<Window>, pub model: Option<String> }`
  - `pub fn parse_status(json: &[u8]) -> Option<Usage>` — reads `session_id`, `model.display_name`, `rate_limits.five_hour|seven_day.{used_percentage,resets_at}`; `resets_at` accepted as unix seconds or RFC 3339; session id kept only if `[A-Za-z0-9-]{1,64}`; > 64 KiB → `None`.
  - `pub fn status_line(u: &Usage) -> String` — e.g. `Opus 5.5 · 5h 63%`.
  - `pub fn write_report(path: &Path, u: &Usage) -> io::Result<()>` / `pub fn read_report(path: &Path) -> Option<Usage>` (TOML or JSON, builder's choice; 0600, atomic).
  - `pub fn run_helper(report: &Path) -> i32` — stdin → parse → write → print `status_line`; exit 0 even on bad input (prints nothing).
  - `AiSession::usage_file: Option<PathBuf>`, `AiSession::usage: Option<usage::Usage>`; `pub(crate) fn read_usage(app: &mut App) -> bool` called from `refresh`.
  - Also sets the per-account "works" flag (Task 5 API) when a report with a session id arrives.

- [ ] **Step 1: Failing tests:** `parse_status_reads_limits_and_id` (sample JSON with both windows, model, session id → values), `parse_status_rejects_bad_id_and_oversize`, `resets_at_rfc3339_and_seconds`, `status_line_format` (`"Opus 5.5 · 5h 63%"`), cli `ai_statusline_flag_alone_with_one_arg`, ai_tests `claude_session_reads_usage_report` (fake claude script that runs the `--settings` statusLine command itself with a sample JSON on stdin, then `exec cat`; after `refresh`, `sessions[0].usage.five_hour.used_percentage == 63.0` and `session_id == Some("abc-123")`; the settings file exists with mode 0600 and names the absolute vr-ide path).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** as in Interfaces. Settings file: `<state>/ai-usage/<serial>-<random>.json` holding `{"statusLine":{"type":"command","command":"<exe> --ai-statusline <report>"}}` with both paths quoted for the shell claude uses for statusLine commands (paths come from vr-ide, not from output); removed when the session closes. Without `state_dir` (tests that set none) nothing is added.
- [ ] **Step 4: Run** full checks → PASS.
- [ ] **Step 5: Commit** `Read claude usage and session id through a statusLine helper`.

### Task 4: Usage display

**Files:**
- Modify: `crates/vr-ide/src/ai/mod.rs` (`AiPanel::label` / `short_label`), `crates/vr-ide/src/ui/ai_view.rs` (warning style), `crates/vr-ide/src/ai/attention.rs` or a small `notify_usage` in `usage.rs` (90 % notice once per window per session), `crates/vr-ide/src/commands/slash.rs` (`/ai usage`), `docs/commands.md`, `README.md`
- Test: `crates/vr-ide/src/ai_tests.rs`, `crates/vr-ide/src/slash_tests.rs`, snapshot tests if the tab bar has them

**Interfaces:**
- Consumes: `AiSession::usage` (Task 3).
- Produces: `pub fn usage_suffix(u: &Usage) -> Option<(String, bool)>` — text (` 63%`, the higher of the two windows, `w` appended for weekly: ` 91%w`) and `warn = pct >= 90.0`.

- [ ] **Step 1: Failing tests:** `label_shows_usage_suffix` (`"claude@work 63%"`), `label_weekly_when_higher` (`" 91%w"`, warn), `notice_once_at_90_percent` (two refreshes ≥ 90 → one notice), `ai_usage_command_lists_windows` (`/ai usage` → info naming account, both percents, reset times as local `HH:MM`), `ai_usage_without_data` (`/ai usage: no usage reported yet for claude`).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; `/ai usage` handled in `fn ai` like `rename`.
- [ ] **Step 4: Run** full checks → PASS; docs table test passes.
- [ ] **Step 5: Commit** `Show AI account usage in the tab label, /ai usage and a 90% notice`.

### Task 5: Limit and login detection, per-account state

**Files:**
- Create: `crates/vr-ide/src/ai/limits.rs`
- Modify: `crates/vr-ide/src/ai/mod.rs` (`refresh` checks the screen of sessions with new output)
- Test: `crates/vr-ide/src/ai/limits.rs` tests

**Interfaces:**
- Produces:
  - `pub struct LimitHit { pub resets_at: Option<i64> }`
  - `pub fn limit_hit(tool: &str, screen: &str) -> Option<LimitHit>` — last 12 non-empty lines only; fixed patterns per tool (claude: `limit reached`, `hit your session limit`, `usage limit`; codex: `hit your usage limit`); reset time parsed when shown (`resets 3pm`, `resets 15:00`, `try again at …`), else `None`.
  - `pub fn login_screen(tool: &str, screen: &str) -> bool` — claude `/login`/"Select login method", codex "Sign in with ChatGPT" (builder confirms the exact strings from the installed binaries and records them in a comment).
  - `pub struct AccountState { pub works: bool, pub limited_until: Option<i64> }`
  - `pub fn load_state(state: &Path) -> BTreeMap<String, AccountState>` / `pub fn save_state(state: &Path, map: &BTreeMap<String, AccountState>) -> io::Result<()>` — file `<state>/ai-account-state`, 0600, atomic, keyed `tool/account` (`claude/default` for Default), ≤ 64 KiB.
  - `AiSession::limit_seen: bool` — set on a new hit, cleared when the tool prints output after the user's next input or a restart.
  - Limit notice: `"claude@work hit its usage limit (resets 15:00)"`, once per event.

- [ ] **Step 1: Failing tests:** patterns for each tool (real sample lines taken from the binaries' strings), `ignores_old_lines` (limit text above the last 12 lines → `None`), `reset_time_parsing`, `login_screen_detected`, `state_roundtrip_and_mode_0600`, `state_ignores_oversize_or_garbage`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; on a hit, store `limited_until` for that account and emit the notice.
- [ ] **Step 4: Run** full checks → PASS.
- [ ] **Step 5: Commit** `Detect AI usage limit and login screens, remember account state`.

### Task 6: Auto-switch

**Files:**
- Create: `crates/vr-ide/src/ai/switch.rs`
- Modify: `crates/vr-ide/src/ai/mod.rs` (`AiSession::auto_switch: bool` from settings, `user_typed_since_limit: bool` set by key/paste paths; call `switch::on_limit` from `refresh`), `crates/vr-ide/src/commands/slash.rs` (`/ai autoswitch on|off`), tab label marker when off (`⊘` after the label, builder may choose another single glyph), `docs/commands.md`, `README.md`
- Test: `crates/vr-ide/src/ai_tests.rs` ("auto-switch" section)

**Interfaces:**
- Consumes: `history::Shared` / `AiSession::shared` (Task 2), `AiSession::usage.session_id` (Task 3), `limits::{limit_hit, login_screen, load_state, save_state}` (Task 5), settings (Task 1).
- Produces:
  - `pub fn next_account(current: Option<&str>, all: &[String], state: &BTreeMap<String, AccountState>, tool: &str, now: i64, tried: &[Option<String>]) -> Option<Option<String>>` — order: Default (`None`) then `accounts::for_tool` order, starting after `current`, wrapping; skips `tried`, not `works`, `limited_until > now`, not shared.
  - `pub fn on_limit(app: &mut App, i: usize)` — the whole flow from the spec (no id → notice only; none left → `"all claude accounts are limited until 15:00"`; else restart in the same tab via `TermTab::replace` with args `tool.args + ["--resume", id]` and account env of the target, swap account locks, keep name/root/changes; notice `"switched claude from work to personal (work limited until 15:00)"`).
  - Continue prompt: when the restarted session's screen is ready (no login screen, output settled for 1 s), and `!user_typed_since_limit`, write `ai_auto_switch_prompt` + `\r` once. Login screen instead → mark target `works = false`, try the next account (each at most once per event).

- [ ] **Step 1: Failing tests** (fake agents: a script that prints a limit line for account A — detect via `$CLAUDE_CONFIG_DIR` — and `exec cat`, printing `ARGV:` like `ECHO_AGENT`):
  - `switches_to_next_working_account_and_resumes` — ARGV of the new run contains `--resume|abc-123|`, label shows the new account, notice text exact, continue prompt echoed once.
  - `switch_resumes_own_session_id_with_two_tabs`
  - `no_switch_at_100_percent_without_limit_screen`
  - `no_continue_prompt_after_user_input`
  - `all_limited_notifies_and_keeps_session`
  - `no_session_id_notifies_only`
  - `autoswitch_off_per_session` (`/ai autoswitch off` → limit → notice only; label marker shown) and setting `ai_auto_switch = false` → same.
  - `login_screen_target_is_skipped` — target prints a login line → marked not working, next account used, no prompt typed into the login screen.
  - `next_account_order_and_skips` (unit).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** `switch.rs` and the wiring.
- [ ] **Step 4: Run** full checks → PASS.
- [ ] **Step 5: Commit** `Switch a limited AI session to another account and resume its conversation`.

### Task 7: Codex

**Files:**
- Modify: `crates/vr-ide/src/ai/usage.rs`, `ai/mod.rs`, `ai/switch.rs` as needed; `README.md`
- Test: `crates/vr-ide/src/ai_tests.rs`

- [ ] **Step 1: Investigate** the installed `codex` (`codex --help`, its config docs, `strings`) for a way to learn the running session's id and usage **without opening codex's files** (e.g. a `-c notify=[...]` program receiving JSON with a thread/session id, an env or OSC output). Record the finding (command, field names, codex version) in a comment at the top of the codex code or in the README.
- [ ] **Step 2a (a reliable id exists):** failing test `codex_switch_resumes_by_id` with a fake codex, then implement the same flow as Task 6 with `codex resume <id>`; usage shown if codex exposes it.
- [ ] **Step 2b (no reliable id):** failing test `codex_limit_notifies_without_switch`, then make `on_limit` notice-only for codex; README states that codex shows limit notices but does not auto-switch, and why.
- [ ] **Step 3: Run** full checks → PASS.
- [ ] **Step 4: Commit** `Handle codex usage limits` (with the finding in the body).

### Task 8: Login window inside vr-ide

**Spec section:** "Login inside vr-ide".

**Files:**
- Create: `crates/vr-ide/src/ai/login.rs` (helper mode, URL allow-list, per-account data folder), behind cargo feature `login-window` (default on) in `crates/vr-ide/Cargo.toml` (`wry`, `tao`, exact versions at least two weeks old)
- Modify: `crates/vr-ide/src/cli.rs` (`--login-window <tool> <account>`, only that form), `crates/vr-ide/src/main.rs` (run the helper before any terminal setup), `crates/vr-ide/src/ai/mod.rs` (`start_program` sets `BROWSER` to `<exe> --login-window <tool> <account|default>` for `claude`), `crates/vr-ide/src/ai/accounts.rs` (`remove` also removes `<state>/ai-login/<tool>-<account>/`), `README.md` (requirements: `libwebkit2gtk-4.1` on Linux; how login works; SSH fallback)
- Test: `crates/vr-ide/src/ai/login.rs` tests, `crates/vr-ide/src/cli.rs` tests, `crates/vr-ide/src/ai_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn allowed(url: &str, callback_port: Option<u16>) -> bool` — `https://claude.ai/…`, `https://*.anthropic.com/…`, `https://accounts.google.com/…`, `https://appleid.apple.com/…`, and `http://localhost:<port>/…` / `http://127.0.0.1:<port>/…` only for the port found in the start URL's `redirect_uri`.
  - `pub fn data_dir(state: &Path, tool: &str, account: Option<&str>) -> PathBuf` — `<state>/ai-login/<tool>-<account|default>`, created 0700 and checked with `accounts::secure`.
  - `pub fn display_available() -> bool` — Linux: `DISPLAY` or `WAYLAND_DISPLAY` set; macOS/Windows: true.
  - `pub fn run_helper(tool: &str, account: &str, url: &str) -> i32` — no display / feature off / web view error → exit 1 (claude then shows the URL and paste prompt); else window until the callback loads, exit 0.
  - `BROWSER` value builder: `pub fn browser_env(exe: &Path, tool: &str, account: Option<&str>) -> OsString`. Claude invokes `$BROWSER <url>`; the helper takes the URL as its last argument.

- [ ] **Step 1: Failing tests:** `allowed_urls` (each allowed host, a lookalike `https://claude.ai.evil.com/` refused, `http://claude.ai/` refused, localhost only on the redirect port), `data_dir_per_account_0700`, `no_display_exits_1` (env without `DISPLAY`/`WAYLAND_DISPLAY`), cli `login_window_flag_form`, ai_tests `claude_gets_browser_env` (fake claude prints `$BROWSER`; it names the absolute vr-ide path, `--login-window claude work`), `remove_account_removes_login_data`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; the window itself (tao event loop + wry web view with `with_data_directory`/per-platform equivalent and a navigation handler calling `allowed`) is checked by hand: document the manual check in the task notes (add an account, log in, the window closes, claude continues; a second account gets a clean login page).
- [ ] **Step 4: Run** full checks (with and without `--no-default-features`) → PASS.
- [ ] **Step 5: Commit** `Log AI accounts in through a vr-ide login window with per-account web data`.
