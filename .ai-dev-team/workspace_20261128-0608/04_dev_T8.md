# T8 Login window inside vr-ide

Task: implement (be), base 44d6c2e / HEAD 826f510. Notes are written as work proceeds.

## Progress
- read skills, plan Task 8, spec "Login inside vr-ide", SEC-33..38, decisions D6/D7/D11/D21
- read code: cli.rs, main.rs, ai/accounts.rs, ai/mod.rs (start_program, remove dialog), ai/history.rs (stray_vars), ai/usage.rs (prepare), commands/terminal.rs (without_stray_vars)
- investigated how claude 2.1.332 opens the browser (strings/grep of the binary only, claude never run) -> contract problem, see below
- STOPPED before any repo change: `BLOCKED T8` (plan interface `browser_env` / SEC-33 / SEC-34 cannot work as written). No repo file changed, no test written yet, nothing running.

## How claude 2.1.332 calls BROWSER (finding)
Binary: `~/.local/share/claude/versions/2.1.332` (`readlink -f ~/.local/bin/claude`), read with `grep -a -o` only.
Minified source of the opener (Linux build):

```js
async function d(e){try{let r=tp()?.browser,o=r!==void 0?r??void 0:s.BROWSER,n="linux";
  if(!o&&m())return{ok:!1,reason:"no_display"};
  let i=o||"xdg-open";
  return x(await vt(i,[e],A()==="macos"&&g(i)==="open"?{...p,cwd:xmt()}:p))}catch(r){...}}
var p={useCwd:!0,useToolMemoryCgroup:!1}
function m(){return A()==="linux"&&!s.DISPLAY&&!s.WAYLAND_DISPLAY}
function Cj(){if(!process.stdout.isTTY)return!0;if(s.BROWSER&&s.BROWSER!=="true")return!1;if(s.SSH_CONNECTION)return!0;return m()}
function x(e){ code 0 -> ok; timedOut -> "timeout"; code 127 / ENOENT -> "opener_missing"; EACCES/EPERM -> "spawn_error"; code>0 -> "nonzero_exit" }
async function Ret(e){ f(e) /* URL must parse and be http: or https: */ ; return d(e)}
```

- `s` = `process.env`; `vt` is the exec-file wrapper (execa underneath, argv, no shell).
- So claude runs **`execFile(<whole BROWSER value>, [url])`** and awaits it: the BROWSER value is ONE program
  path, never word-split, no `%s`. `BROWSER="/abs/vr-ide --login-window claude work"` gives ENOENT
  ("opener_missing" -> "Couldn't open your browser", URL shown). The helper would receive exactly one argument, the URL.
- `tp()?.browser` (a string from a terminal/host capability record) wins over `BROWSER` when present.
- `Cj()`: with `BROWSER` set (and not `true`) claude assumes a browser can open even over SSH / without a display.
- claude awaits the opener process; a `timedOut` result is not treated as a failure. The wrapper's timeout value
  was not found in readable form -> a helper that lives as long as the window may be killed or may hold the
  login flow; the helper should hand the window to a detached process and return like `xdg-open` does.
- Also seen: `xdg-open` itself reads `$BROWSER` (list split on `:`) -> the D6 hand-off to the system opener
  must run it with `BROWSER` removed, or it calls the helper again (loop).

## Why this blocks (contract)
Plan Task 8 interface: `browser_env(exe, tool, account) -> OsString` = `<exe> --login-window <tool> <account>`;
SEC-33 fixes the same value; SEC-34 recognises the helper only as exactly `--login-window <tool> <account> <url>`
(4 args); SEC-38 detects an inherited helper by "BROWSER containing `--login-window`". With claude 2.1.332 a
BROWSER value holding arguments is never executed, so AC-9 ("claude's login opens a vr-ide login window") cannot
be met by the contract as written. Any working form needs BROWSER = one executable path that gets only the URL.

## Options (for the leader / user)
1. **Per-account launcher link, no shell (proposed default).** `<state>/ai-login/bin/vr-ide-login.<tool>.<account>`
   is a symlink to the vr-ide exe (made 0700 dir, checked with `accounts::secure`, link target compared
   byte-equal on each use, replaced otherwise). `BROWSER` = that path (charset rule of SEC-33 applied to the
   whole path, state folder included). The helper mode is recognised by argv[0]'s file name
   `vr-ide-login.<tool>.<account>` (tool in {claude}, account `default` or SEC-15-valid) plus exactly ONE
   argument (the URL); the explicit form `--login-window <tool> <account> <url>` stays for the detached window
   process and for tests. SEC-38 detection becomes: inherited BROWSER whose file name starts with
   `vr-ide-login.` or that contains `--login-window`. Removal deletes the link with the web data.
   Cost: SEC-33/34/38 wording changes; a link per account under the state folder; argv[0] dispatch in main.
2. Per-account launcher script `<state>/ai-login/<tool>-<account>.sh` (0700): `#!/bin/sh` +
   `exec '<exe>' --login-window <tool> <account> "$1"`. Keeps SEC-34's 4-arg form unchanged, but starts the
   helper through `/bin/sh` (plan constraint "never through a shell", SEC-8) and leaves a same-uid-writable
   script that claude and its children execute.
3. A second small binary target (`vr-ide-login`) installed beside vr-ide: cannot carry tool/account either
   (still needs 1 or 2), and changes packaging.
4. Drop the login window for this run (keep only the wording fix for the remove dialog/README).

Second point to confirm with either working option: the process claude starts validates the URL, starts the
window as a detached process (own session, no stdio), waits up to ~5 s for its "window is up" byte on a pipe and
exits 0 (exit 1 when there is no display / the feature is off / the URL is refused by policy / the window
failed) — so claude is never held by, and never kills, the window. A URL that is not a claude login start URL is
handed to `xdg-open`/`open`/`start` with `BROWSER` removed (D6).

## Reuse (planned, nothing built yet)
| Needed | Looked in | Result |
|---|---|---|
| private folder + check | `recovery::create_private_dir`, `accounts::secure` | reuse |
| no-follow delete of web data | `accounts::remove_tree_at` | reuse (inside `accounts::remove`, under the index lock) |
| per-account busy lock | `accounts::unlock`, flock pattern of `open_account_lock` | reuse pattern for `<state>/ai-login/<tool>-<account>.lock` |
| helper exe checks | `usage::command_path` / `prepare_channel` (absolute, regular, not ` (deleted)`), `app.ai.usage_exe` test seam | reuse |
| inherited-variable removal | `history::stray_vars`, `terminal::without_stray_vars` | extend with `BROWSER` |
| helper-mode dispatch | `cli::ai_statusline` / `ai_codex_notify`, first lines of `main` | same pattern |

## Rule compliance
Not filled: the task stopped at the investigation step (BLOCKED before any change). BASE-49 kept (claude/codex never run; binary read with grep only). BASE-51: no mutation or experiment made. Nothing left running.
- leader decided D63 (launcher symlink) + D64 (detached window); building test-first
- red: 13 new login:: tests failed against stubs ("0 passed; 13 failed"); green after the implementation: `cargo test -p vr-ide --lib -- login:: cli::` -> 23 passed
- red->green: accounts::login_tests (3 failed -> 16 accounts tests pass), history stray BROWSER (failed -> ok), ai_tests claude_gets_browser_env + unusable path (2 failed -> ok), tests/login_helper.rs (4 pass on the real binary), remove dialog wording + snapshot, README; next: clippy both feature sets, full vr-ide tests both feature sets
- clippy `--workspace --all-targets -- -D warnings`: clean with default features and with `--no-default-features`; `cargo fmt --all --check`: clean
- full `cargo test -p vr-ide` (run-heavy): default features `1103 passed; 0 failed; 7 ignored` (+ desktop_cli 3, login_helper 4, statusline_helper 4); `--no-default-features` the same counts; logs `qa-artifacts/T8/test-default.log`, `test-nodefault.log`

## Handover (written at ~300k context; a fresh developer continues here)

### State of the tree
Compiles, all tests green, fmt + clippy clean in both feature sets (lines above). Nothing staged, nothing
committed. No background task left (the lock-date script `b3sq1jxzh` was stopped unfinished, see "Left").
BASE-51: no red-proof mutation or experiment copy is outstanding. The only copy in `qa-artifacts/T8/` taken from the
tree is `Cargo.lock.base` (the lock file BEFORE the dependencies were added; reference for the date check, not to
be restored). `login_impl.rs.part`, `login.rs.new`, `cli.patch`, `accounts.patch` are build scraps of this task
(already in the tree; may be deleted).

### Files touched
- `Cargo.toml` (workspace deps `url = "=2.5.8"`, `wry = "=0.57.2"` (default-features off, `os-webview`), `tao = "=0.37.2"` (default-features off, `rwh_06`, `x11`)), `Cargo.lock`
- `crates/vr-ide/Cargo.toml` (feature `login-window`, default on, = `dep:wry`, `dep:tao`; `url` always)
- `crates/vr-ide/src/ai/login.rs` (new: allow-list, start URL, launcher link, lock, data dir, launcher, window)
- `crates/vr-ide/src/cli.rs` (`LOGIN_WINDOW_FLAG`, `login_window`, `LoginLauncher`, `LauncherCall`, `login_launcher` + tests)
- `crates/vr-ide/src/main.rs` (launcher by argv[0] first, then `--login-window`, before everything else)
- `crates/vr-ide/src/ai/mod.rs` (`pub mod login`; `AiPanel.login_display`, `login_warned`; `login_browser` in `start_program`; `login_hint` in `add_account`; remove-dialog wording)
- `crates/vr-ide/src/ai/accounts.rs` (`hold_login_data`, `remove_login_data`, called by `remove`; `mod login_tests`)
- `crates/vr-ide/src/ai/history.rs` (`stray_vars` / `stray_warnings` also cover an inherited launcher `BROWSER`; NOT in the board row's file list — it is the reuse seam the brief names for SEC-38)
- tests: `crates/vr-ide/src/ai_tests.rs`, `ai_account_tests.rs`, `crates/vr-ide/tests/login_helper.rs` (new), snapshot `ui/tests/snapshots/…ai_account_remove_80x24.snap` (regenerated once)
- `README.md` (Requirements: WebKitGTK/GTK packages and `--no-default-features`; Accounts → Remove wording; new "Login window" section)

### Built and tested (test → what it proves)
- `ai::login::tests::allowed_urls` — SEC-35 table (+ `claude.com`, `platform.claude.com`), lookalikes, userinfo, IDN, `\`, ports, schemes, `127.1`
- `…::start_url_and_callback_port` — SEC-34 start URL, port only from `redirect_uri` (1024..=65535, localhost/127.0.0.1 exact), two `redirect_uri` refused
- `…::refusals_show_scheme_and_host_only`, `…::refusal_lines_are_rate_limited` — SEC-34/T-46
- `…::display_needs_a_variable_on_linux`, `…::no_display_exits_1` (through `prepare`, never a window)
- `…::data_dir_per_account_0700`, `…::one_window_per_account` — SEC-37, SEC-34 lock
- `…::browser_env_is_the_launcher_link`, `…::helper_browser_values_are_recognised` — SEC-33 (D63), SEC-38
- `…::opener_gets_the_argument_without_browser`, `…::launcher_waits_for_the_window_byte`, `…::launcher_refuses_a_login_without_window` — D6, D64
- `cli::tests::login_window_flag_form`, `cli::tests::login_launcher_name_form`
- `ai::accounts::login_tests::{remove_account_removes_login_data, remove_is_refused_while_the_login_window_is_open, remove_never_follows_links_in_the_login_data}`
- `ai::history::tests::an_inherited_login_launcher_is_a_stray_browser`
- `ai_tests::claude_gets_browser_env`, `ai_tests::an_unusable_vr_ide_path_gives_no_browser_and_one_notice`
- `ai_account_tests` remove-dialog wording (3 variants) and the snapshot
- `tests/login_helper.rs` (real binary, no display, stand-in `xdg-open`): `launcher_hands_other_urls_to_the_system_opener`, `launcher_exits_1_for_a_login_without_a_display`, `launcher_takes_exactly_one_argument`, `window_process_exits_1_without_a_window`
Red seen first: login stubs `0 passed; 13 failed`; accounts login_tests `0 passed; 3 failed`; history test failed
(`left: [] right: ["BROWSER"]`); ai_tests 2 failed (`left: "unset"` / `left: []`). The cli tests and
tests/login_helper.rs were written together with their code (no red run): a BASE-51 mutation proof is still owed
for them (see Left 2).

### Design as built
- claude 2.1.332: `execFile($BROWSER, [url])`, awaited (finding at the top of these notes).
- `BROWSER` = `<state>/ai-login/bin/vr-ide-login.claude.<account|default>` → symlink to the vr-ide exe
  (`login::browser_env`; exe and link path must be `[A-Za-z0-9/._+-]`, folders 0700 + `accounts::secure`; only a
  symlink is ever replaced). Set only in a claude session's spawn env (`ai/mod.rs::login_browser`), not when the
  feature is off, there is no state folder or no display.
- `main`: argv[0] file name `vr-ide-login.*` → `cli::login_launcher` → `Run` (absolute `…/ai-login/bin/<name>`,
  tool claude, valid account, exactly 1 arg; state folder = 3 levels up) or `Refused` (exit 2, never the editor).
- `login::run_launcher`: claude login start URL (`login_start`: https, first-party host, path ends
  `/oauth/authorize`) → `spawn_window` = detached `<current_exe> --login-window <tool> <account> <url>` with
  `VR_IDE_STATE_DIR=<state>`, `BROWSER` removed, own process group, stdout a pipe; waits ≤ 5 s for the byte `1`
  (0), early end (1), timeout (0). Anything else → `exec xdg-open <arg>` (`open` on macOS) with `BROWSER` removed.
  No display / feature off → 1.
- `login::run_helper` → `prepare` (tool/account → start URL → display → feature → lock → data dir) → `window::run`:
  tao window titled `vr-ide login - claude@work`, wry web view on `WebContext::new(Some(data_dir))` (wry sets
  WebKit base data + cache dir and cookie file there), navigation handler = `decide` (Allow / Callback / Refuse),
  new-window requests denied (an allowed URL is loaded in the same view), downloads and permissions denied, no
  IPC / custom protocol / init script, devtools only in debug builds. Refusal: title `… - blocked <scheme>://<host>`
  + stderr, ≤ 1/s. Exit 0 at the first page finished after the callback was requested (or 5 s after), 1 on close
  or after 15 min. A GTK start panic is caught → 1.
- `accounts::remove`: takes the window lock(s) `ai-login/<tool>-<name>.lock` (busy → "its login window is open",
  nothing deleted), deletes the account folder, then `ai-login/<tool>-<name>` (fd-based, no-follow; a link in its
  place is unlinked), the launcher link and the lock file. No `ai-login/` → nothing made.
- SEC-38: `history::stray_vars` drops an inherited `BROWSER` for which `login::is_helper_browser` holds.

### Decisions to append to `$TEAM_WS/decisions.md` (NOT yet appended)
- D65 (T8): allow-list also has the exact hosts `claude.com` and `platform.claude.com`: claude 2.1.332 starts on
  `https://claude.com/cai/oauth/authorize` / `https://platform.claude.com/oauth/authorize` and ends on
  `https://platform.claude.com/oauth/code/success` (strings of the binary); without them no login can start.
  Extends SEC-35's list. confirm-on-return.
- D66 (T8): a "claude login start URL" (D6) = https, allowed first-party host (not an identity provider), path
  ending `/oauth/authorize`; every other argument of the launcher (also `https://claude.ai/upgrade`, files) goes to
  the system opener unchanged. More than one `redirect_uri` → not a start (goes to the opener).
- D67 (T8): `data_dir` returns `Result<PathBuf, &'static str>` (plan: `PathBuf`) and `browser_env` takes the state
  folder (`browser_env(state, exe, tool, account) -> Result<OsString, &'static str>`), following D63.
- D68 (T8): the local callback must be written exactly `http://localhost:<port>` / `http://127.0.0.1:<port>`
  (`127.1`, decimal, upper case refused although an engine reads them as the same host); URLs with `\`, blanks or
  control characters are refused before parsing.
- D69 (T8): no display → `BROWSER` is not set at all (claude keeps its own URL + paste flow; other children keep
  `xdg-open`); the notice is the "Account … created" one, extended with "(no display for the login window: open
  the URL it shows and paste the code)", not a notice at every claude start. An unusable vr-ide / state path gives
  one warning per run: `claude login window not available (<why>)`.
- D70 (T8): on Linux every navigation the engine asks about is checked, frames included (wry's handler cannot tell
  a frame from the top level) — stricter than SEC-36; a login page whose captcha / identity frame is refused shows
  `blocked <host>` in the title; hosts are added only after the manual check, in `login::identity_provider` with
  the reason.
- D71 (T8): the window closes at the first page that finishes loading after the callback was requested, or 5 s
  after the request; closing by hand or 15 min → exit 1.
- D72 (T8): remove dialog: "…its <tool> login and settings, and the web sign-in vr-ide's login window kept for
  it." (claude only) + one history line by the account's history entry: a link → "The shared conversation history
  stays; only this account's link to it goes."; a real folder → "Its conversation history is not shared and is
  deleted too."; none → no line.
- D73 (T8): the launcher derives the state folder from its own path (agent children lose `VR_IDE_*`), and hands it
  to the window process as `VR_IDE_STATE_DIR`.

### Left to do (in order)
1. BASE-8 date check of the transitive crates the lock gained: `python3 -I $TEAM_WS/qa-artifacts/T8/lock_new.py
   $TEAM_WS/qa-artifacts/T8/Cargo.lock.base Cargo.lock` (1 request/s to crates.io, a few minutes → run as a
   background task; it was stopped unfinished, `lock_new.txt` is empty). Direct deps are fine: wry 0.57.2
   2026-11-10, tao 0.37.2 2026-10-23, url 2.5.8 2026-01-05 (today 2026-11-28). A transitive crate newer than
   2026-11-14 → pin an older one with `cargo update -p <crate> --precise <ver>` or report it.
2. BASE-51 red proofs for the code written with its tests: e.g. in `cli.rs::login_launcher` drop the `[target]`
   arity (→ `login_launcher_name_form` and `launcher_takes_exactly_one_argument` fail); in `login::opener_command`
   drop `.env_remove("BROWSER")` (→ `opener_gets_the_argument_without_browser`,
   `launcher_hands_other_urls_to_the_system_opener` fail). Copy the file to `qa-artifacts/T8/` first, `cp` back,
   `cmp`, one notes line per file.
3. Append D65–D73 to `$TEAM_WS/decisions.md`; add `ai::login` to `$TEAM_STATE/capabilities.md` (BASE-16).
4. `## Rule compliance` table (every BASE / COST id once, AC-9, SEC-33..38) — not written yet. Evidence = the test
   names above; SEC-36 and the "all data under the folder" half of SEC-37 are `pass` only for the code path
   (`decide` tests, wry `WebContext` source read) and need the manual check below.
5. Re-run after 1–2 if anything changed: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D
   warnings` (+ `--no-default-features`), `cargo test -p vr-ide` both feature sets through run-heavy.
6. Optional: the profile maps `Cargo.toml`/`Cargo.lock` changes to `cargo build --workspace` (not run; the test
   and clippy builds passed).

### Not verified (say so in the report)
- The window was never opened here (no virtual display tool; the brief forbids opening one in tests). Unknown
  until the manual check: that WebKitGTK asks the handler for server redirects, that Google / Apple / a captcha
  work inside the embedded view without further hosts, that nothing lands in `~/.local/share` / `~/.cache`.
- macOS: the non-Linux branch (`builder.build(&window)`) was not compiled.

### Manual check for the user (window)
1. `cargo install --path crates/vr-ide`; start vr-ide in a desktop session; f12 → claude → `+ Add account…` →
   `work` → in its tab `/login`. Expect: a window titled `vr-ide login - claude@work` with claude's sign-in page (not
   the browser). Sign in and approve → the window closes by itself, claude continues signed in.
2. Add a second account `alt`, `/login`: a clean sign-in page (not signed in as the first account).
3. `/login` again on `work`: its page already knows the account (quick re-login).
4. In a claude tab ask it to run `xdg-open https://example.com`: opens in the normal browser, no vr-ide window.
5. Isolation: `ls ~/.local/state/vr-ide/ai-login/` shows `bin/`, `claude-work/`, `claude-alt/`, `*.lock` (0700 /
   0600); nothing new in `~/.local/share/vr-ide*`, `~/.cache/vr-ide*`, `~/.local/share/webkitgtk`.
6. SEC-36: serve a local page on a port P that redirects to `https://example.com` and calls
   `window.open("https://example.com")`, then run
   `vr-ide --login-window claude work 'https://claude.ai/oauth/authorize?redirect_uri=http://localhost:P/callback'`
   and navigate to it through the flow: both are refused, the title shows `blocked https://example.com`.
7. Remove `alt` in the picker (del): the question names the web sign-in; afterwards `ai-login/claude-alt/`, its
   `.lock` and `bin/vr-ide-login.claude.alt` are gone. With the login window of an account open, removing it is
   refused ("its login window is open").
8. Over SSH (no display): adding an account says "(no display for the login window: …)" and claude shows its URL
   and the paste prompt.
If step 1 shows `blocked <host>` in the title or a provider refuses the embedded view, note the host: it is added
in `login::identity_provider` / `first_party` with the reason (D70).
- close-out started: lock date check running in background
- decisions.md D? (T8) x9 and capabilities.md ai::login appended; red proofs done (below)

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-9 | pass (code path) | `ai::login::tests` (allowed_urls, data_dir_per_account_0700, no_display_exits_1, browser_env_is_the_launcher_link), `accounts::login_tests::remove_account_removes_login_data`, `tests/login_helper.rs` (4 on the real binary); the window itself was never opened here: manual check list in the Handover is open |
| SEC-33 | pass | `ai::login::tests::browser_env_is_the_launcher_link`, `ai_tests::claude_gets_browser_env`, `an_unusable_vr_ide_path_gives_no_browser_and_one_notice` (contract changed by D63/D67) |
| SEC-34 | pass | `login::tests::start_url_and_callback_port`, `refusals_show_scheme_and_host_only`, `refusal_lines_are_rate_limited`, `one_window_per_account`, `no_display_exits_1`, `cli::tests::login_window_flag_form`, `login_launcher_name_form`, `login_helper::{launcher_takes_exactly_one_argument, window_process_exits_1_without_a_window}` |
| SEC-35 | pass | `login::tests::allowed_urls` (table + D65 hosts) |
| SEC-36 | pass (code path only) | `decide` tests; wry navigation handler/denied new-window/download/permission read in `login.rs`; real redirect/`window.open` check needs the manual step 6 (not run) |
| SEC-37 | pass (code path only) | `data_dir_per_account_0700`, `remove_account_removes_login_data`, `remove_is_refused_while_the_login_window_is_open`, `remove_never_follows_links_in_the_login_data`; "nothing under ~/.local/share, ~/.cache" needs manual step 5 (not run) |
| SEC-38 | pass | `history::tests::an_inherited_login_launcher_is_a_stray_browser`, `login::tests::helper_browser_values_are_recognised` |
| BASE-1 | pass | files listed in Handover; `ai/history.rs` is the only file outside the board row (reuse seam for SEC-38) |
| BASE-2 | pass | contract deviations recorded as D63/D64, D67 after the BLOCKED round; no silent change |
| BASE-3 | pass | red runs in Progress (login 13 failed, accounts 3, history 1, ai_tests 2); cli + login_helper tests mutation-proved below |
| BASE-4 | pass | Progress lines: full `cargo test -p vr-ide` 1103 passed both feature sets, clippy + fmt clean; logs `qa-artifacts/T8/test-default.log`, `test-nodefault.log` |
| BASE-5 | pass | full runs went through run-heavy; mutation runs were single-test cargo invocations; nothing left running |
| BASE-6 | pass | English code/comments/notes |
| BASE-7 | n-a (no commit made) | nothing staged or committed |
| BASE-8 | pass with 4 findings | direct deps pinned: wry 0.57.2 (2026-11-10), tao 0.37.2 (2026-10-23), url 2.5.8 (2026-01-05); transitive date check: see Close-out; 4 younger than 14 days reported for the leader |
| BASE-9 | pass | SEC-33..38 rows |
| BASE-10 | pass | single-test runs in close-out; full vr-ide package run was done once at build end |
| BASE-14 | pass | the 157-crate date check ran as a background task, no polling |
| BASE-15 | pass | no pkill/killall; only own background task |
| BASE-16 | pass | `## Reuse` table above (planned reuse built as planned); `ai::login` added to `$TEAM_STATE/capabilities.md` |
| BASE-19 | pass | progress lines from the start |
| BASE-22 | pass | file names `login.rs`, `login_helper.rs`, test names follow neighbours (`statusline_helper.rs`) |
| BASE-23 | pass | entry paths: launcher by argv[0], explicit `--login-window`, window process, system-opener hand-off: tests per path in `login_helper.rs` and `cli::tests` |
| BASE-24 | pass | no git add/commit/restore run |
| BASE-25 | pass | guard line `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` clean (Progress); close-out changed no file (cmp), so not rerun |
| BASE-32 | pass | D6, D7, D11, D21, D63, D64 honoured |
| BASE-38 | pass | `cargo fmt --all --check` only (no formatter run over the tree) |
| BASE-45 | pass | AC-9 is in `01_spec.md` line 21 |
| BASE-46 | pass | red-proof and test runs are on the current tree (restored byte-identical, `cmp`) |
| BASE-48 | pass | reported states (mutation restored) read back with `cmp`; window behaviour is stated as not verified |
| BASE-49 | pass | claude/codex never run; claude's opener read from the binary with grep only |
| COST-1 | pass | full runs via run-heavy |
| COST-3 | pass | no sleep/poll |
| COST-4 | pass | no whole-suite run in close-out; one full package run at build end |
| COST-5 | pass | nothing left running |
| COST-8 | pass | edits via sed on mutations only (restored by cp), appends by `cat >>` |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set: cli.rs, login.rs, login_helper.rs mention checked by name |
| COST-13 | pass | scripts/copies under `qa-artifacts/T8/` |
| COST-16 | pass | full outputs in `qa-artifacts/T8/test-*.log` |
n-a: BASE-11..13, BASE-17, BASE-18, BASE-20, BASE-21, BASE-26..31, BASE-33..37, BASE-39..44, BASE-47 (no shared-tree git ops, no removal, no secrets output, no schema/UI/other-repo/rewrite/DB work) · COST-2, COST-6, COST-7, COST-11, COST-12, COST-14, COST-15, COST-17..21 (no QC role, no UI sweep, no CI wait, no images; BASE-18/17 kept: no agents started, no toolchain installed). BASE-50/BASE-51 do not exist in this rule-packs version; the leader's "BASE-51" red proof is covered under BASE-3.

## Close-out
1. BASE-8 transitive date check: `python3 -I qa-artifacts/T8/lock_new.py Cargo.lock.base Cargo.lock` -> 157 added, 0 removed, 0 errors, 0 yanked (output `qa-artifacts/T8/lock_new.txt`). Younger than 14 days (cutoff 2026-11-14), NOT changed, for the leader:
   - cookie 0.18.3 (2026-11-23)
   - toml_edit 0.25.20+spec-1.1.0 (2026-11-21)
   - form_urlencoded 1.2.3 (2026-11-20)
   - bytes 1.12.2 (2026-11-17)
   Everything else is 2026-11-10 or older (wry 0.57.2 2026-11-10).
2. BASE-3/51 red proofs (originals copied to `qa-artifacts/T8/*.orig`, restored with `cp`, `cmp` identical):
   - `crates/vr-ide/src/cli.rs`: `[target]` -> `[target, ..]` in `login_launcher`; `cargo test -p vr-ide --lib -- cli::tests::login_launcher_name_form` -> FAILED (0 passed; 1 failed); `cargo test -p vr-ide --test login_helper -- launcher_takes_exactly_one_argument` -> FAILED. Restored, cmp OK.
   - `crates/vr-ide/src/ai/login.rs` (mutated only for the helper/opener tests): drop `.env_remove("BROWSER")` in `opener_command`; `... --lib -- opener_gets_the_argument_without_browser` -> FAILED; `... --test login_helper -- launcher_hands_other_urls` -> FAILED. Restored, cmp OK.
   - `tests/login_helper.rs` has no mutated copy of its own: its tests above turned red through the two product mutations.
3. D65-D73 appended to `workspace/decisions.md` as nine `D? (T8)` lines; `ai::login` entry added to `.ai-dev-team/capabilities.md`.
4. No tracked file differs from before close-out (cmp), so fmt/clippy/tests were not rerun. Not staged, not committed. Nothing left running.

## Fix round 1
1. CWE-88 (low): `login::launch` returns `Launch::Fail` for a target whose first byte is `-` (checked before the opener branch), so exit 1 and the opener never runs. Red first: `launcher_refuses_a_login_without_window` failed (cases `--help`, `-oProxyCommand=x`, `-https://x/--version`, `-`) and new `login_helper::launcher_never_passes_an_option_to_the_opener` failed; green after the fix.
2. `accounts::remove`: the web data (`remove_login_data`) is now removed before the account folder, so its failure returns before the folder, the lock file and the index change. Red first: new `login_tests::a_failed_web_data_removal_keeps_the_account_listed` (0500 subfolder makes the delete fail with EACCES) failed; green; a retry after restoring the mode removes everything.
3. Busy second `/login`: D? (T8 R1) records the choice (refuse + clear message + README line). `BUSY` text is now "a login window for this account is open: close it, then run /login again"; `one_window_per_account` asserts it and now compares against the constant.
4. D74 / BASE-8 pins (publish dates from crates.io): cookie 0.18.3 (2026-11-23) -> 0.18.2 (2026-08-08); toml_edit 0.25.20 (2026-11-21) -> 0.25.19+spec-1.1.0 (2026-11-06); form_urlencoded 1.2.3 (2026-11-20) -> 1.2.2 (2025-08-21); bytes 1.12.2 (2026-11-17) -> 1.12.1 (2026-07-08, above the 1.11.1 RUSTSEC-2026-0007 floor). Lock diff = these four plus the `syn` choice cookie's dependency lists (3.0.10 -> 2.0.119). `url =2.5.8` kept: 2.5.9 is younger than two weeks and RUSTSEC-2026-0592 needs `Url::origin()`, which vr-ide does not call. Date check re-run on the result (`qa-artifacts/T8/lock_new_r1.txt`): 157 added, 0 errors, newest is wry 0.57.2 (2026-11-10), none after the 2026-11-14 cut-off. `cookie` `private` feature stays off.
5. `--login-window` with any other than 4 arguments: main prints `usage: vr-ide --login-window <tool> <account> <url>` and exits 2 (test `login_window_flag_with_wrong_arguments_prints_usage`, red first).
- BASE-51/cmp: no new mutation this round (the fixes were driven by tests seen red first); pre-round copies are `qa-artifacts/T8/R1.*.orig` (reference only, not restored; the files carry the intended edits). Format: `rustfmt --edition 2024` on `login.rs` and `tests/login_helper.rs` only (BASE-38).
- Runs (through run-heavy, logs `qa-artifacts/T8/r1-*.log`): related set `--lib -- login:: cli:: accounts:: ai_account` 49 passed; `--test login_helper` 6 passed. `cargo test -p vr-ide` default features: 1104 passed, 0 failed, 7 ignored (+ desktop_cli 3, login_helper 6, statusline_helper 4); `--no-default-features`: same counts. Guard: `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` exit 0; same with `--no-default-features` exit 0.
- Nothing staged or committed; nothing left running. README, accounts.rs, login.rs, main.rs, tests/login_helper.rs, Cargo.lock touched.
