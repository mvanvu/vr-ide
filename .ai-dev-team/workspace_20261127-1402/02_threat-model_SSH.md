# Threat model: FR-25 SSH sessions (D317, AC-S1..S6)

Task FR-25-TM. Inputs: `01_spec_FR17-21.md` § "SSH sessions (FR-25, D317)", D317, the terminal code
(`crates/vr-term/src/terminal.rs`, `input.rs`; `crates/vr-ide/src/commands/terminal.rs`,
`ui/terminal_view.rs`), `02_threat-model_M7.md` (style and reused protections). Nothing here is built yet.
Local ssh: OpenSSH 9.6p1. Facts checked with `ssh -G` (no connection): `-o` options on the command line
win over `~/.ssh/config` (a config `LocalCommand` / `PermitLocalCommand no` is replaced by ours); ssh
refuses a destination like `-oProxyCommand=x` given after `--` ("hostname contains invalid characters").

Setting: local single-user TUI; vr-ide starts the system `ssh` in a pty tab. ssh itself runs
`LocalCommand`, `ProxyCommand`, `Match exec`, `KnownHostsCommand` through `$SHELL -c`; that is ssh's
behaviour for the user's own config and is accepted. vr-ide's job is to add nothing an attacker controls to
that and to keep its own marker secret. Attackers:

- **B1, remote server / remote account** (likely): every byte after connect (banner, MOTD, programs run
  there): escape sequences, fake markers, fake vr-ide UI in full screen.
- **B2, typed target** (the user, a pasted string): option-like, `%`-tokens, whitespace, control chars.
- **B3, ssh config content** (`~/.ssh/config` and its `Include`s, often from dotfiles repos or generated
  by tools): odd `Host` names, huge files, Include loops, FIFOs/devices, globs.
- **B4, other local processes / other vr-ide tabs**: print a marker into another tab's stream, read
  `/proc/<pid>/cmdline`, tamper with the recent-list file (same uid only).
- **B5, the environment vr-ide was started from**: outer terminal variables, `PATH` with `.`, `SHELL`.

## Assets
| ID | Asset | Why it matters |
|----|-------|----------------|
| AS-1 | Code execution as the user | A typed target or a config alias must not run a local program. |
| AS-2 | Secrets typed into the tab (passwords, passphrases, OTP) | Must go only to ssh's pty; never stored, logged or shown by vr-ide. |
| AS-3 | Login state shown to the user (full screen = "logged in to `<target>`") | A spoofed marker or label makes the user type secrets into the wrong place. |
| AS-4 | The per-session nonce | Whoever knows it can fake "login succeeded". |
| AS-5 | The user's ssh security settings (host-key checking, agent, mux) | vr-ide must not weaken or silently change them. |
| AS-6 | Clipboard, host terminal, vr-ide's own UI | Hostile escape sequences (OSC 52, titles, cursor moves into vr-ide rows). |
| AS-7 | UI responsiveness and process hygiene | Config parsing, `ssh -G`, many sessions, quit with live sessions. |

## Trust boundaries & data flows
| ID | Flow | Boundary crossed |
|----|------|------------------|
| S-F1 | `ssh` palette list: parse `~/.ssh/config` + Includes, read recent file | config files / state file → vr-ide parser → palette rows |
| S-F2 | Typed `[user@]host[:port]` | palette text → validator → argv |
| S-F3 | Pre-flight `ssh -G` (user's effective LocalCommand) | argv → ssh child (runs `Match exec`) → parsed output |
| S-F4 | Spawn `ssh` in a pty tab | argv + env → ssh → `$SHELL -c <LocalCommand>` locally |
| S-F5 | pty output → marker matcher → emulator → grid/scrollback | remote bytes + local ssh/LocalCommand bytes → vr-term |
| S-F6 | Keys / paste → pty | user → vr-ide key handler → ssh → remote |
| S-F7 | Full screen, status row, tab label, notices | vr-ide state + remote title → screen |
| S-F8 | Recent list write | validated target → `state_dir()` file |
| S-F9 | Exit, restart, close, quit | vr-ide → ssh process group |

## Threats (STRIDE per flow)
| # | Flow | Threat | Cat. | L | I | Sev. | Mitigation |
|---|------|--------|------|---|---|------|------------|
| ST1 | S-F2/S-F1 | Option injection: target or alias `-oProxyCommand=sh -c ...`, `-F/tmp/x`, `-E file`, leading `-`, `user@-x`; whitespace splitting if argv were built from a string. | E | M | H | high | SEC-S1, SEC-S2 |
| ST2 | S-F2/S-F4 | `%`-tokens (`%h`, `%d`) or `${VAR}` in a target reaching LocalCommand / ControlPath / Hostname expansion; `ssh://` URI form changing port/user parsing; control chars / BIDI in a target shown as tab label. | T/E | L | M | medium | SEC-S1, SEC-S3, SEC-S12 |
| ST3 | S-F4 | LocalCommand string run by `$SHELL -c`: if it contained the target, a `%` token, `$`, backticks or quotes, the user's shell would execute attacker-influenced text. | E | L | H | high | SEC-S3 |
| ST4 | S-F4 | `PermitLocalCommand=yes` enables the user's own `LocalCommand` that they disabled; our `-o LocalCommand` silently replaces the user's own. | E/T | M | M | medium | SEC-S4 |
| ST5 | S-F3 | `ssh -G` runs `Match exec` and may hang (prompt, slow command); run for every listed host it would execute config commands on palette open. | D/E | M | M | medium | SEC-S4 |
| ST6 | S-F4/S-F9 | ControlMaster auto + ControlPersist: vr-ide's ssh becomes a daemonised master that outlives the tab and vr-ide, holds the pty slave, keeps an authenticated channel open; mux clients never run LocalCommand. | D/I | M | M | medium | SEC-S5, SEC-S17 |
| ST7 | S-F5 | Marker spoofing: remote prints a marker-shaped sequence (pre-auth banner, MOTD, `cat` of a file) → full screen before login, user types password believing they are in a shell; another tab prints this tab's marker. | S | M | H | high | SEC-S6, SEC-S8 |
| ST8 | S-F5/S-F7 | Nonce leak: marker drawn on screen, kept in scrollback, copied by selection, written to logs/notices/recent file → later replay by B1 (e.g. user pastes scrollback into a remote shell). | I/S | M | M | medium | SEC-S6, SEC-S7 |
| ST9 | S-F6/S-F8 | Secrets stored: pty input logged by tracing, a password mistyped into the palette echoed in a refused-target notice or saved as a recent target. | I | M | H | high | SEC-S9, SEC-S10 |
| ST10 | S-F8 | Recent file world-readable (host inventory), replaced through a symlink, or tampered with to inject an option-like target. | I/T | L | M | low | SEC-S10 |
| ST11 | S-F1 | Config parsing DoS: multi-GB file, `Include /dev/zero` or a FIFO (blocks forever), Include loop, glob over `/`, 1 M Host lines → frozen palette / memory blow-up. | D | L | M | medium | SEC-S11 |
| ST12 | S-F7 | Label spoofing: remote OSC 0/2 title `ssh bank-prod` replaces the tab label (today `TermTab::title()` prefers the OSC title, unsanitized in `ui/terminal_view.rs`); cursor addressing writes into the status row; remote draws a fake vr-ide editor in full screen. | S | M | M | medium | SEC-S12, SEC-S14 |
| ST13 | S-F5 | Hostile escapes: OSC 52 clipboard write, OSC 8 links, query floods filling the input queue, title-report answerback. | T/D | M | M | medium (mostly already mitigated) | SEC-S13 |
| ST14 | S-F6 | Full screen key capture: remote sets kitty keyboard / application modes so ctrl+` reaches ssh and the user cannot leave; paste breaks out of bracketed paste. | D/T | L | M | low | SEC-S14 |
| ST15 | S-F4 | Env leak / redirection: outer terminal IPC variables, `VR_IDE_*`, a nonce variable sent to the remote via `SendEnv`; `./ssh` in the workspace run via relative `PATH`. | I/E | L | M | low | SEC-S2, SEC-S15 |
| ST16 | S-F9 | Many sessions (each a process + scrollback); quit drops live sessions without asking; restart reuses an old nonce. | D | L | M | low | SEC-S16, SEC-S17, SEC-S18 |

Automated abuse (scrapers, bots, AI crawlers): not applicable. No public flow, endpoint or listener; the
only network use is the user's own `ssh` to a host the user chose.

## Security requirements for implementation
Tests use a fake `ssh` shim on an absolute `PATH` (records argv/env to a file, prints scripted bytes, can
run the LocalCommand it received through `sh -c`), `HOME`/`VR_IDE_STATE_DIR` in a temp dir, and the
existing `TermTab::fake` / snapshot helpers. qc checks against a real `sshd` only on localhost.

### Target, argv, LocalCommand
- **SEC-S1 (target validator)**: one function validates every target (typed, config alias, recent-file
  line) as ASCII `[user@]host[:port]` or `[user@][v6]:port`: user `[A-Za-z0-9._][A-Za-z0-9._-]{0,63}`,
  host `[A-Za-z0-9][A-Za-z0-9._-]{0,252}` or a bracketed IPv6 literal, port decimal 1..65535; it refuses
  a leading `-` in any part, whitespace, C0/C1/DEL, `%`, `$`, `/`, `\`, quotes, `` ` ``, `;`, a second
  `@`, `ssh://` and non-ASCII; refused input spawns nothing. Verify: table test with `-oProxyCommand=x`,
  `-F/tmp/c`, `a -p 1`, `a\tb`, `a\x1b[2Jb`, `%h`, `a$HOME`, `u@-x`, `u@h@h`, `ssh://h:2`, `h:0`,
  `h:65536`, `h:+22`, `‮h` → refused and the shim log is empty; `bob@h:2222`, `[::1]:22`, `h` accepted.
- **SEC-S2 (argv only, no weakening)**: ssh is spawned directly (no `sh -c`, no command string) with argv
  exactly `<abs ssh> [-l <user>] [-p <port>] -o ControlMaster=no [-o PermitLocalCommand=yes -o
  LocalCommand=<SEC-S3>] -- <host>`; the binary is resolved once from `PATH` skipping empty and relative
  entries (same rule as vr-git `find_git`); vr-ide never passes `StrictHostKeyChecking`,
  `UserKnownHostsFile`, `BatchMode`, `-F`, `-i`, forwarding options or anything else, and never writes an
  answer to a prompt. Verify: unit test on the built argv for each target shape; shim `./ssh` in the cwd
  with `PATH=.:<shimdir>` → the absolute shim runs, `./ssh` does not; grep of the ssh module for
  `"sh"`, `"-c"`, `StrictHostKeyChecking` is empty.
- **SEC-S3 (fixed LocalCommand)**: the LocalCommand value is the constant
  `printf '\033]<N>;vr-ide-ssh;<nonce>\007'` (fixed private OSC number N), the nonce is 128 bits from the
  OS CSPRNG as 32 lowercase hex chars, fresh for every spawn (restart included); the value contains no
  target text and matches `^printf '\\033\][0-9]+;vr-ide-ssh;[0-9a-f]{32}\\007'$` (so no `%`, `$`, `` ` ``,
  `"`, newline). Verify: property test over 10 000 generated values (regex, uniqueness); run the value
  with `sh -c`, `bash -c`, `dash -c` and, when installed, `zsh -c`/`fish -c` → stdout is exactly
  `ESC ] N ;vr-ide-ssh; <nonce> BEL`; `ssh -G -F /dev/null -o LocalCommand=<value> -- h` echoes it
  unchanged.
- **SEC-S4 (respect the user's LocalCommand, bounded `-G`)**: before each spawn vr-ide runs
  `ssh -G [-l][-p] -- <host>` (without its own `-o` options) once, with stdin null, no controlling terminal
  (new session), 5 s timeout, 256 KiB output cap and the whole group killed on timeout; when the output has
  a non-empty `localcommand`, or `-G` fails or times out, the session is spawned without the
  `PermitLocalCommand`/`LocalCommand` pair (plain tab, no full screen); `PermitLocalCommand=yes` is never
  passed without vr-ide's own LocalCommand; `-G` is never run to build the palette list. Verify: config
  with `LocalCommand touch <marker>` → argv lacks both options and the user's `PermitLocalCommand no`
  stays in force; shim whose `-G` sleeps → killed at 5 s, tab opens plain, no child left (`/proc` scan);
  opening the palette with 50 hosts → shim log has no `-G` call.
- **SEC-S5 (no vr-ide-owned mux master)**: `-o ControlMaster=no` is always passed, so vr-ide's ssh never
  starts a (persistent) master; an existing master is used as a client and the tab simply stays plain.
  Verify: argv test; qc with `ControlMaster auto` + `ControlPersist 10m` + localhost sshd → after logout no
  ssh process whose session began in vr-ide remains and no new socket at `ControlPath`.

### Marker
- **SEC-S6 (marker matcher before the emulator)**: in vr-term the raw pty bytes of a session spawned with a
  nonce pass a byte-level matcher before `parser.advance`; the exact sequence
  `ESC ] N ;vr-ide-ssh; <nonce> BEL` is removed (every occurrence) and only the first one, while the
  process runs, raises the login event; at most `len(marker)-1` bytes are held back and are released as
  soon as a byte disproves the match; a sequence with another nonce or shape is not an event. Verify: feed
  the marker split at every offset across reads → event once, `Grid` cells, scrollback and the text from
  `selected_text` over all rows contain neither `vr-ide-ssh` nor the nonce; feed the marker twice →
  one event; feed a wrong nonce and a 31-char nonce → no event; feed `ESC ] N ;vr-ide-ss` then `x` →
  the held bytes reach the emulator in order.
- **SEC-S7 (nonce secrecy)**: the nonce exists only in vr-ide memory, the ssh argv (visible to the same
  uid via `/proc`, accepted) and the LocalCommand output; it is never in tracing output, notices, tab
  labels, the status row, env, the recent file or crash/recovery reports. Verify: run a shim session to
  login and exit with tracing at `trace` level → grep the log dir, state dir and every rendered frame
  (`TestBackend`) for the nonce → none.
- **SEC-S8 (per-tab binding)**: a login event changes only the tab whose own stream carried its own
  nonce; bytes written to other tabs (including that exact marker) and markers after the process exited
  change nothing. Verify: two ssh tabs A/B, write A's marker into B's stream → neither goes full screen;
  shim prints the marker after exiting its pty child → no full screen.

### Secrets, files, config parsing
- **SEC-S9 (no secret capture)**: neither vr-term nor the panel logs key events or pty input/output bytes
  for any tab; a refused target produces a fixed notice ("SSH: not a valid target") that does not echo the
  input; the palette keeps no history of typed `ssh` text. Verify: type a password-shaped string in the
  tab and an invalid one in the palette with tracing at `trace` → string absent from log dir, state dir,
  notices and recent file.
- **SEC-S10 (recent file)**: `state_dir()/ssh-recent` holds only SEC-S1-valid target strings, one per line,
  at most 20, most recent first; a target is recorded only after its login event or a 0 exit; the
  directory/file are created like `recovery.rs` `create_private_dir`/`create_private_file` (0700/0600) and
  replaced atomically (0600 temp file + rename), never following a symlink at the final path; on load a
  file over 64 KiB is ignored and each line is re-validated, invalid lines dropped. Verify: perms test
  (0600 file, 0700 dir, also with umask 022); recent file containing `-oProxyCommand=x`, `a b`, 10 000
  lines → palette lists only valid ones / nothing; failed login (exit 255, no marker) → not recorded;
  symlink at the path → replaced by a regular file, target untouched.
- **SEC-S11 (bounded config parser)**: the list reads `~/.ssh/config` and its `Include`s only: regular files
  checked by metadata before open (no FIFO, device, directory), 1 MiB per file, 64 files total, Include
  depth 16, each file once (canonical path), relative Include paths resolved against `~/.ssh`, at most 256
  glob matches per Include, at most 1 000 aliases; lines it does not understand are skipped; `Match` is
  never evaluated; only concrete `Host` tokens passing SEC-S1 are offered (`*`, `?`, `!`, quoted names
  skipped); parsing runs off the UI thread or within these bounds. Verify: fixtures with a 50 MiB file,
  `Include /dev/zero`, a FIFO, a self-Include and A↔B loop, `Include /*`, `Host -oProxyCommand=x a*b
  !c "d e" ok` → palette opens in < 200 ms, lists `ok` only, no hang.

### Screen, keys, env, lifecycle
- **SEC-S12 (labels from vr-ide only)**: the ssh tab label is `ssh <target>` and the full-screen status row
  `SSH <target>` + the key hint, both built from the validated target, never from the remote OSC title;
  the pty is sized to the window minus the status row, which vr-ide draws, so remote cursor addressing
  cannot reach it; OSC titles of any tab, where shown, pass `ui::safe_text::sanitize`. Verify: shim prints
  `ESC]2;ssh other BEL`, `ESC[999;1H` + text, `‮` title → tab label, status row and tab bar unchanged /
  placeholders visible (`TestBackend`).
- **SEC-S13 (escape sequences unchanged in full screen)**: ssh tabs use the same emulator config as other
  tabs: `Osc52::Disabled`, OSC 8 links inert (no open on click), only the existing `PtyWrite` answers
  capped by `MAX_PENDING_INPUT`; the host terminal title (`main.rs` `SetTitle`) never takes remote text.
  Verify: shim sends `ESC]52;c;ZXZpbA==BEL` → app clipboard unchanged; OSC 8 link + click → nothing
  opened; 10 MiB of `ESC[6n` → memory stays bounded and UI responsive.
- **SEC-S14 (reserved keys)**: in full screen ctrl+`, F1 and ctrl+F7..F9 are matched before `encode_key`
  whatever modes the remote set (kitty keyboard, application cursor/keypad, mouse reporting); paste goes
  through `encode_paste` (ESC dropped inside bracketed paste). Verify: key-level test with each mode
  enabled by the shim → ctrl+` returns to the editor and no byte for it reaches the shim.
- **SEC-S15 (child env)**: ssh gets the terminal env (`OUTER_TERMINAL_ENV` removed, `TERM=xterm-256color`,
  `child_env`) with `SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `DISPLAY`, `SSH_ASKPASS*` left as inherited; vr-ide
  adds no variable carrying the nonce or target and removes `VR_IDE_*`; cwd is `$HOME`. Verify: shim dumps
  its env → `SSH_AUTH_SOCK` equals vr-ide's, no `VR_IDE_*`, no `KITTY_*`/`WEZTERM_*`, nonce absent.
- **SEC-S16 (session limit)**: at most 16 live ssh tabs and one `ssh -G` at a time; a further request gets a
  notice and spawns nothing; each tab uses the panel's scrollback cap. Verify: open 17 shim sessions →
  17th refused with a notice, 16 shim processes.
- **SEC-S17 (close and quit)**: live ssh tabs count as running terminals in `quit_allowed` (T26) and the
  close dialog; quitting or closing kills and reaps the ssh process group as other terminals do; full
  screen ends first so the dialog is visible. Verify: quit with one live shim session → dialog shown;
  Quit anyway → no shim process remains (`/proc` scan).
- **SEC-S18 (exit and restart)**: on exit or failure full screen ends whether or not the marker came; with no
  marker the tab never goes full screen (no timeout fallback); "Enter to restart" re-runs SEC-S1..S4 on the
  stored validated target with a new nonce. Verify: shim exits 255 before the marker → plain tab with
  `[process exited 255]`; restart → second argv has a different nonce, and the first nonce printed into the
  new stream raises no event.

## Accepted risks / open points for the leader
- `$SHELL -c` runs our fixed LocalCommand (ssh's design); SEC-S3 keeps it constant apart from hex.
- The nonce is readable in `/proc/<pid>/cmdline` by the same uid (and others without `hidepid`); such a
  process still cannot write into this tab's pty (SEC-S8), so it gains nothing.
- An explicit `PermitLocalCommand no` without a LocalCommand is overridden by ours (indistinguishable from
  the default in `-G` output); harmless because only the SEC-S3 constant runs. If the user wants it
  respected, add a vr-ide setting `ssh.fullscreen = false` instead.
- With an existing mux master (SEC-S5) or the user's own LocalCommand (SEC-S4) there is no full screen; the
  spec's "tab stays a normal panel tab" covers it, but the user may see it as a bug.
- A remote can draw a fake vr-ide UI inside full screen; the vr-ide-owned status row (SEC-S12) is the only
  tell. Accepted.
- Existing (non-ssh) tabs: `TermTab::title()` shows OSC titles without `safe_text::sanitize`; SEC-S12
  covers it for all tabs, so the developer fixes it in FR-25.
