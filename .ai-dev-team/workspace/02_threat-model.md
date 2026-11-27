# Threat model: run 4 (D3 session file, D4 launcher, D5 `/ssh` argument line, D7 agents, D8 AI changes, D11 accounts)

Task TM. Inputs: `01_spec.md` (D1–D9, AC-1..AC-11), the FR-25 model
`../workspace_20261127-1402/02_threat-model_SSH.md` (SEC-S1..S18), code: `crates/vr-ide/src/ssh/{mod,recent}.rs`,
`commands/ssh.rs`, `commands/slash.rs` (`ai`, `run`, `one_line`), `commands/terminal.rs` (`spawn_program`,
`child_env`), `recovery.rs` (`create_private_dir/_file`), `cli.rs`, `main.rs`, `vr-core/src/watch.rs`,
`vr-core/src/document.rs` (`write_atomic`), `vr-core/src/safe_replace.rs` (`replace_if_unchanged`,
`create_if_absent`), `vr-fs/src/ops.rs` (`trash`), `vr-config/src/paths.rs`. Local ssh: OpenSSH 9.6p1.

Setting: local single-user TUI, no listener, no network service of its own. Automated abuse (bots,
crawlers, AI agents calling an API): not applicable — no public flow. The AI agents of D7 are local
programs the user starts; they are treated as an untrusted writer of workspace files (D8), not as a client.

Attackers:
- **A1 untrusted workspace content** (cloned repo, agent output): file names with control/BIDI chars,
  symlinks pointing outside the root, huge or binary files, FIFOs, a `ssh` / `nc` binary in the repo.
- **A2 the AI agent (or any program) writing the workspace while the agent runs**: races with Revert,
  swaps a file or a parent folder for a symlink, floods the tree with changes.
- **A3 typed / pasted text** (the user, a pasted line): option-like words, quotes, control chars.
- **A4 stored state replayed later**: `ssh-recent`, `session.toml`, the pre-image index (same uid can
  edit them; that uid can also edit `~/.bashrc`, so integrity loss there is low, but these files cause
  processes to start and files to be overwritten without fresh typing).
- **A5 install path / environment**: binary path with spaces, quotes, `%` or a newline; `PATH` with `.`.

## Assets
| ID | Asset |
|----|-------|
| AS-1 | Code execution as the user (ssh local-command options, agent spawn, desktop `Exec`). |
| AS-2 | Integrity of user files (Revert overwrites, created files go to the trash). |
| AS-3 | Secrets in workspace files (`.env`, `credentials/id_*`) copied into the pre-image store. |
| AS-4 | Truthful UI: the ssh tab label = what ssh connects to; the change list = what really changed. |
| AS-5 | FR-25 guarantees (SEC-S2..S18): marker secrecy, no vr-ide mux master, env, labels, limits. |
| AS-6 | Disk / responsiveness (pre-image store, watcher floods, session parsing). |
| AS-7 | Agent account folders (tokens written by claude / codex) and which account a session runs on. |

## Trust boundaries & data flows
| ID | Flow |
|----|------|
| F1 | `/ssh <line>` → shell-like split → argv → `ssh -G` probe → `ssh` in a pty (cwd = workspace folder) |
| F2 | `ssh-recent` file → palette rows / Tab completion → F1 again (replay) |
| F3 | `session.toml` ↔ `--session` instance (save debounced, load at icon launch) → roots, file tabs, agent tabs |
| F4 | `--install-desktop` → `.desktop` + icon under `~/.local/share` → GNOME launches `Exec` |
| F5 | AI panel → agent program spawn (settings command + args, continue args on restore) |
| F6 | watcher events in the agent's root → pre-image capture → `state_dir()/ai/` store |
| F7 | change list → diff view → Keep / Revert (atomic write or trash) |

## Threats
| # | Flow | Threat | Cat. | L | I | Sev. | SEC |
|---|------|--------|------|---|---|------|-----|
| T-1 | F1 | Words that make ssh run local programs (`-o ProxyCommand=`, `LocalCommand`, `KnownHostsCommand`, `PKCS11Provider`, `-I`, `SecurityKeyProvider`, `-F <repo file>` with `Match exec`). Typed by the user = accepted (same as a terminal); the risk is the same line arriving without fresh typing (F2) or vr-ide's own options being displaced. | E | M | H | high | SEC-1, SEC-2, SEC-5 |
| T-2 | F1 | vr-ide's prefix options vs the user's: ssh keeps the **first** `-o` value, so our `PermitLocalCommand=yes`/`ControlMaster=no` would silently override the user's own `LocalCommand`, `PermitLocalCommand no`, `ControlMaster`, and `-M` overrides ours. | T/E | M | M | medium | SEC-3 |
| T-3 | F1 | Label spoofing: vr-ide guesses the destination wrong (`-p bank evil`, `-voX`, options after the host, `ssh://`), so the tab/status row says `SSH bank` while ssh goes elsewhere; control/BIDI chars in the label. | S | M | M | medium | SEC-2 |
| T-4 | F1 | Split differs from a shell (unbalanced quote silently joined, `~` / `$` expanded, NUL/newline kept) → argv the user did not mean; a `--` or host appended by vr-ide after the user's words changes their meaning. | T | M | M | medium | SEC-1 |
| T-5 | F1 | Relative `-i/-F/-E/IdentityFile=` resolved against the wrong folder (another root, `$HOME`, the replay's current workspace) → a different repo's `config` or key used; `ProxyCommand nc` resolved through a relative `PATH` entry in a repo cwd. | E/T | L | M | medium | SEC-4 |
| T-6 | F2 | Replay: a recent row now carries options; a row truncated by width hides `-o ProxyCommand=…`; a tampered or group-writable file feeds lines; old `h:2222` targets are not valid ssh syntax. | E/S | L | M | medium | SEC-5 |
| T-7 | F1/F2 | Secrets: a remote-command word (`mysql -pX`) stored in `ssh-recent`, echoed in a refusal notice, or logged with the argv. | I | M | M | medium | SEC-1, SEC-5 |
| T-8 | F3 | Session file world-readable (paths, hosts), written through a symlink, torn by a crash or two `--session` instances, a corrupt/huge file crashing or freezing startup; paths in it to FIFOs/devices/`/` expanded deep. | I/T/D | L | M | medium | SEC-6 |
| T-9 | F3/F5 | Restore spawns programs without typing: an agent tool name / args taken from `session.toml` (or typed into a shell) → command injection (`claude; curl …\|sh`). | E | L | H | high | SEC-8 |
| T-10 | F4 | `Exec` built from `current_exe()` with spaces, `"`, `` ` ``, `$`, `\`, `%` or a newline → wrong program or injected desktop keys; uninstall deletes a file that is not vr-ide's; `(deleted)` exe path after reinstall. | E/T | L | M | low | SEC-7 |
| T-11 | F5 | Agent binary resolved from a relative `PATH` entry or the workspace (`./claude`); vr-ide-internal env passed on. | E | L | H | medium | SEC-8 |
| T-12 | F6 | Pre-image is the **after** content (inotify reports after the write), so Revert "restores" the agent's version or garbage while claiming safety. | T | H | H | high | SEC-9 |
| T-13 | F6 | Store leaks secrets (`.env`, private keys) with wider modes, in `/tmp`, in logs; store names built from workspace paths (`../`); store grows without bound (agent writes 100k files, a 5 GB file, `target/`); with the default root `~/` the store's own writes are inside the watched tree → feedback loop. | I/D | M | M | medium | SEC-10, SEC-11 |
| T-14 | F6 | Capture reads through a symlink (outside the root), opens a FIFO (blocks), or records files outside the agent's root. | I/D | M | M | medium | SEC-11 |
| T-15 | F7 | Revert TOCTOU: the agent/user writes after the diff was shown → their newer work overwritten; parent folder swapped for a symlink → write lands outside the workspace (`~/.ssh`); non-atomic write torn by a crash; created file unlinked instead of trashed, or the trash follows a link. | T | M | H | high | SEC-12 |
| T-16 | F7 | Revert of a file open in the editor with unsaved edits; mode bits (setuid) restored; binary written as text (BOM/encoding change). | T | M | M | medium | SEC-12 |
| T-17 | F6/F7 | Misleading list: watcher overflow / inotify limit drops events and the list looks complete; the diff buffer of a secret file ends up in recovery files, recent files or the session. | S/I | M | M | medium | SEC-13 |
| T-18 | D9 | Notification text from OSC 9/777 (any program, incl. a remote host in an ssh tab) shown unsanitized → fake vr-ide prompts, escape injection. | S | M | L | low | SEC-14 |

## Security requirements for implementation

Tests use the existing fake `ssh` shim on an absolute `PATH`, `HOME`/`VR_IDE_STATE_DIR`/`VR_IDE_CONFIG_DIR`
in a temp dir, `FakeTrash`, and the `safe_replace` seams. No real hosts.

### `/ssh` argument line (T3)
- **SEC-1 (split, no shell)** — one function turns the line into words: POSIX-like single quotes, double
  quotes (only `\"`, `\\`, `` \` ``, `\$` escapes inside), backslash outside quotes; no `$`, backtick,
  glob, `;`/`|`/`&`/redirection processing (they stay literal characters of a word); `~` expanded only
  when a word is exactly `~` or starts with `~/` (to `$HOME`, never `~user`). Refused, with a fixed notice
  that does not echo the line: unbalanced quote, trailing lone backslash, any control character
  (`one_line` rule) incl. NUL/newline, > 4 KiB, > 128 words, empty result. ssh is spawned with an argv
  (no `sh -c`); vr-ide never appends `--`, a host or anything else after the user's words. The argument
  line is never written to tracing at any level.
  Verify: table test `-i 'a b' h` → [`-i`,`a b`,`h`]; `"x\"y"`, `a\ b`, `'$HOME'`, `$HOME`, `*`, `~/k`,
  `~root/k`, `a;b`; `"abc` / `a\` / `a\nb` → refused, notice text constant, shim log empty; grep the module
  for `"sh"`, `"-c"` → none.
- **SEC-2 (destination and label, SEC-S1/S12 kept)** — the destination is found with ssh's own option
  grammar (OpenSSH 9.6 getopt `1246ab:c:e:fgi:kl:m:no:p:qstvxAB:CD:E:F:GI:J:KL:MNO:P:Q:R:S:TVw:W:XYy`:
  bundled flags `-vp22`, attached values `-p22`, `-oKey=v`; options also after the destination until the
  first non-option word; `--` ends options); the destination word must pass `Target::parse` (extended to
  accept `ssh://[user@]host[:port]`); no destination or an invalid one → refused (fixed notice). Tab label
  `ssh <destination>` and status row `SSH <destination>` are built only from that validated word (plus
  `-l`/`-p` values only if they also pass the SEC-S1 character rules), never from remote titles; a host
  word starting with `-` is never the destination (ssh reads it as an option, exactly as in a terminal).
  Verify: `-p bank evil` → label `ssh evil`; `-vp 22 h`, `h -p 2222`, `-oPort=1 h ls -la`,
  `-J jump host`, `ssh://u@h:2` → labels `h`/`h`/`h`/`host`/`u@h:2`; `-oProxyCommand=x` alone,
  `-- -oProxyCommand=x`, `h‮x` → refused, nothing spawned.
- **SEC-3 (vr-ide's own options never displace the user's)** — argv = `<abs ssh> [prefix] <user words>`
  where prefix is `-o ControlMaster=no [-o PermitLocalCommand=yes -o LocalCommand=<SEC-S3 constant>]`.
  The whole prefix is left out (plain tab, the existing "stays a panel tab" notice) when any user word
  contains, case-insensitively, `localcommand`, `controlmaster`, `controlpath` or `controlpersist`, or
  the parsed options include `-M`, `-S`, `-O`, `-G`, `-V`, `-Q`, `-f`. Otherwise the probe of SEC-S4 runs
  as `ssh -G <user words>` (same 5 s / 256 KiB / no tty / group kill), and its `localcommand` /
  ControlPath result decides as today. `PermitLocalCommand=yes` is never passed without our constant;
  nonce rules SEC-S3, S6–S8, S18 unchanged; SEC-S15 env unchanged except cwd (SEC-4).
  Verify: argv tests for `h`, `-o LocalCommand=x h`, `-oPermitLocalCommand=no h`, `-M h`,
  `-o controlpath=/tmp/s h` (→ no prefix); shim `-G` sees the user's `-F` and `-o` words; `-G` that sleeps →
  killed at 5 s, plain tab.
- **SEC-4 (relative paths, PATH)** — ssh's cwd is the canonical workspace root of the active file (else the
  active root; a missing root → refused with a notice, never a silent fallback to another folder);
  relative `-i/-F/-E/-S/-o …File=` therefore resolve exactly as in a terminal opened there. The ssh binary
  is still resolved from absolute `PATH` entries only (`find_in_path`), and the child's `PATH` has empty
  and relative entries removed, so a `ProxyCommand nc` run in a repo cwd cannot pick `./nc`. Tab
  completion of path arguments uses the same base folder and only lists directories (no program is run,
  bounded to 1 000 entries, non-blocking on FIFOs).
  Verify: two roots each with `credentials/k`, active file in root B → shim cwd = B; `PATH=.:/shim`
  with `./ssh` and `./nc` in the root → `/shim/ssh` runs and the child env `PATH` lacks `.`.
- **SEC-5 (recent list replay)** — `ssh-recent` keeps SEC-S10 (0600 file in 0700 dir, atomic temp +
  rename, symlink at the path replaced not followed, ≤ 64 KiB, ≤ 20 entries, recorded only after login or
  exit 0) and stores per entry the argument line **and the absolute base folder** it ran in; replay uses
  that folder (missing → refused with a notice). On load each line is re-run through SEC-1/SEC-2, invalid
  lines dropped; the file is ignored when not owned by the user or group/world-writable. A palette row
  shows the whole line (sanitized); when it does not fit, the row is truncated in the middle so the
  destination and the first option stay visible, and selecting a recent row that contains a word matching
  `command|pkcs11provider|securitykeyprovider|^-I` (case-insensitive) fills the input instead of connecting
  (Enter runs it). Old bare targets with `:port` are migrated to `-p <port> <user@host>`.
  Verify: perms test with umask 022; file with mode 0666 → ignored; line `-o ProxyCommand=x h` selected →
  input filled, shim log empty until Enter; `bob@h:2222` old line → argv `-p 2222 bob@h`; entry from root A
  replayed while root B is active → shim cwd A.

### Session file (T1)
- **SEC-6 (session file)** — `state_dir()/session.toml`: dir/file via `create_private_dir/_file`
  (0700/0600 regardless of umask), written to a pid-unique temp in the same dir, `fsync`, `rename`; a
  symlink at the final path is replaced, never followed. Only a `--session` instance writes it, holding an
  exclusive `flock` on `session.lock`; a second `--session` instance gets a notice and does not save. On
  load: regular file (by `symlink_metadata`, no FIFO open), owned by the user and not group/world-writable,
  ≤ 1 MiB, else ignored with one notice; parse errors → one notice, empty session (opens `~/`), the bad file
  renamed to `session.toml.bad` (0600); numbers (cursor, scroll, widths) clamped; ≤ 64 roots, ≤ 200 tabs,
  ≤ 2 000 expanded folders. Roots go through the `/ws add` W-1 checks (missing → `(missing)`); file tabs
  must be regular files under a restored root (others skipped with the one notice) and are opened with
  the normal size/binary rules; expanded folders only under a restored root. Unsaved buffer text, terminal
  commands, ssh lines and secrets are never stored; restore never runs a terminal command or ssh.
  Verify: perms test; symlink at the path → regular file written, target untouched; 2 MiB / garbage /
  `cursor = 1e30` / FIFO tab / tab outside roots → no panic, notices; two `--session` instances → one
  saves; kill -9 during save → old or new file, never partial.

### Desktop launcher (T2)
- **SEC-7 (desktop entry)** — `Exec` from `std::env::current_exe()`, refused when not absolute, not an
  existing regular file, ending in ` (deleted)`, or containing a control character/newline. Each `Exec`
  argument is quoted per the Desktop Entry spec (double quotes; `"`, `` ` ``, `$`, `\` backslash-escaped;
  then the value-level escaping of `\`; literal `%` written `%%`); GNOME Terminal form
  `gnome-terminal --title=vr-ide -- <exe> --session`, else `<exe> --session` with `Terminal=true`; plus
  `TryExec=<exe>` and `X-VrIde-Generated=true`. Files written atomically (temp + rename, 0644, symlink not
  followed). `--uninstall-desktop` removes only the two paths, and the `.desktop` only when it carries
  `X-VrIde-Generated=true`; nothing else under `~/.local/share` or other apps' settings is changed.
  Verify: exe path `/tmp/a b/"x$y%z\` → `desktop-file-validate` passes and a parser test splits Exec back
  to the exact path; path with `\n` → refused; uninstall with a hand-made `vr-ide.desktop` → kept.

### Agents in the AI panel (T5)
- **SEC-8 (agent spawn)** — an agent starts from settings only: the tool's `command` (a bare name resolved
  with `find_in_path` over absolute `PATH` entries, or an absolute path) and its `args` / continue `args`
  as TOML string lists, passed as argv without a shell (if T5 keeps typing into a shell, every word is
  POSIX-quoted and the name must still match a configured tool). `session.toml` stores only the tool
  **name**, root and account name (SEC-18); on restore the name must equal a tool in the current `ai_tools` and be installed, and
  the root must be a restored, existing root — else the tab is not reopened (one notice). cwd = that
  canonical root; env = terminal env (`OUTER_TERMINAL_ENV` removed, `VR_IDE_*` removed, relative `PATH`
  entries removed as SEC-4); restore never sends input to an agent.
  Verify: session with tool `claude; touch /tmp/p` → not spawned, no file; `ai_tools` command `./claude`
  with `./claude` in the root → refused; shim agent dumps argv/env/cwd → continue args exactly as in
  settings, no `VR_IDE_*`.

### AI changes: capture (T6)
- **SEC-9 (true pre-image or none)** — a pre-image is stored only from content known to predate the change
  (e.g. read when the agent starts / when vr-ide first sees the file unchanged, the open buffer's disk
  snapshot with matching stamp, or a git blob whose stat matches); content read after an event is never
  stored as the pre-image. Without one the file is listed "changed — no earlier version kept" and Revert
  is disabled (Keep only). Verify: agent shim rewrites a file vr-ide never read → listed, Revert disabled;
  file read at agent start then changed → Revert restores the exact earlier bytes.
- **SEC-10 (store)** — `state_dir()/ai/<agent-id>/` via `create_private_dir/_file` (0700/0600 whatever the
  original's mode; never `/tmp`); blob names generated (counter or hash), never derived from a workspace
  path; the index maps root-relative paths validated on load (no `..`, no absolute, under the root).
  Caps: 8 MiB per file, 256 MiB per agent, 512 MiB total, 5 000 records per agent; over a cap the file is
  listed with "too large / limit reached — no earlier version kept", existing pre-images are never evicted
  silently. Records and blobs are deleted on Keep, after a successful Revert, and when the agent's tab is
  closed and its list dismissed; at startup, store folders not referenced by a restored agent are removed
  (no symlink followed). Contents and paths are never logged. Excluded from recording: `state_dir()`,
  `config_dir()`, the trash dir, `.git/` internals, vr-ide's own saves and its own Revert writes
  (`own_writes`) — this also stops the feedback loop when the root is `~/`.
  Verify: perms with umask 022 on a 0644 `.env` → blob 0600; root = temp `HOME` containing the state dir →
  no record of the store's own files; 9 MiB file → no blob, Revert disabled; 6 000 created files → list
  capped with a notice, store ≤ caps; file named `../../x` in the index → record dropped.
- **SEC-11 (capture reads)** — capture uses `symlink_metadata` and reads only regular files with
  `O_NOFOLLOW|O_NONBLOCK`, at most cap + 1 bytes; symlinks, FIFOs, sockets, devices and paths whose
  canonical form is outside the agent's canonical root are listed (if inside) without a pre-image or
  ignored (if outside). Binary files are stored byte-exact. Verify: FIFO in the root → no hang; symlink
  `a → /etc/passwd` changed → no blob of `/etc/passwd`.

### AI changes: Revert / Keep (T6)
- **SEC-12 (revert)** — Revert asks first (dialog names the file and says changes by other programs are
  attributed to the agent). The expectation (disk stamp + full-content hash, racy rule of `document.rs`) is
  taken when the diff/dialog is shown; the write goes through `safe_replace::replace_if_unchanged` (or
  `create_if_absent` for a deleted file) with that expectation from a temp file in the same folder,
  byte-exact (no BOM/encoding/line-ending change), mode = recorded mode `& 0o777`, `fsync` file and dir;
  changed since → refused with a notice, nothing written. Right before the swap the parent's canonical path
  must still be inside the canonical root (a parent swapped for a symlink → refused); the final path is
  replaced, never written through. A file open with unsaved edits → refused with a notice; an open clean
  buffer reloads. A created file is moved with `vr_fs::trash` (link itself, never its target) after the
  same unchanged check; never `remove_file`; trash unavailable → file stays, error shown. Created folders
  are left.
  Verify: `safe_replace` seam writes between dialog and swap → refused, writer's bytes kept; parent
  replaced by symlink to a temp "outside" dir → refused, outside untouched; final path a symlink → link
  replaced by a regular file, target untouched; created file → in `FakeTrash`; binary pre-image restored
  byte-exact with mode 0755 kept and setuid dropped.
- **SEC-13 (honest list, no secret spill)** — a watcher `Error` or `Rescan` while an agent runs marks its list
  "may be incomplete" (until dismissed); file names in the list and dialogs pass `ui::safe_text::sanitize`;
  the diff buffer is read-only and is never written to recovery files, the recent-files list or
  `session.toml`. Verify: inject `FsEvent::Error` → banner shown; name with `ESC[2J`/BIDI → placeholders;
  panic hook with a diff open → no recovery file for it.

### Notify (T7)
- **SEC-14 (notification text)** — OSC 9/777 text is sanitized (`safe_text::sanitize`), capped at 200 chars,
  shown as "<tab label>: <text>" so it cannot pose as a vr-ide message, at most one notice per tab per
  second; notifications from ssh tabs are treated the same. Verify: OSC 9 with `ESC[2J`, 10 KiB, 1 000/s →
  one sanitized, capped notice per second.

### Accounts per agent session (D11, T8)
Flow F8: account name (typed when adding, read from `session.toml` on restore, listed from the folder) →
`state_dir()/accounts/<name>/` → `CLAUDE_CONFIG_DIR` / `CODEX_HOME` in that agent's child env only. The
tool writes its tokens into that folder; vr-ide never opens them.

| # | Flow | Threat | Cat. | L | I | Sev. | SEC |
|---|------|--------|------|---|---|------|-----|
| T-19 | F8 | Name → path traversal (`../..`, `/abs`, `.`, a name with `/`, NUL, control/BIDI chars) → config dir or a removal outside `accounts/`. | T/E | L | H | high | SEC-15, SEC-17 |
| T-20 | F8 | Token folder readable by others (umask 022, pre-existing wide folder) or `accounts/<name>` a symlink to a folder elsewhere (tokens written there, removal follows it). | I/T | L | H | medium | SEC-16 |
| T-21 | F8 | Remove deletes more than the account (follows a symlink, wrong name, the whole `accounts/`), deletes while a session runs on it, or leaves tokens in the trash. | T/I | L | H | medium | SEC-17 |
| T-22 | F8 | Env leakage: the variable set with `std::env::set_var` in vr-ide (every later shell, ssh, other agent inherits it → wrong account, tokens written into another account's folder); a variable name from settings (`LD_PRELOAD`, `PATH`) pointed at the folder. | I/E | M | M | medium | SEC-18 |
| T-23 | F3/F8 | Restore reopens a tab on the wrong account: missing folder → silent fallback to Default (or to another account), `--continue` resumes another account's conversation, a tampered name escapes the folder. | S/T | M | M | medium | SEC-15, SEC-18 |

- **SEC-15 (account name)** — one validator for every name (add, completion, list, restore): ASCII
  `[a-z0-9][a-z0-9_-]{0,31}` (so no `.`, `..`, `/`, `\`, whitespace, control or non-ASCII), and `default`
  (any case) reserved for "no variable". The path is always `state_dir()/accounts/` + the validated name,
  never another join. The account folder list is read by `read_dir` of `accounts/`, keeping only entries
  that pass the validator and are real folders (`symlink_metadata`). Names are shown as validated (tab
  label `claude · <name>`). Verify: table test `../x`, `/etc`, `a/b`, `.`, `..`, `A B`, `a\x1b`, `é`,
  `Default`, 33 chars → refused with a fixed notice, nothing created; `work`, `alt-2` accepted.
- **SEC-16 (folder permissions)** — `accounts/` and each `<name>/` created with `create_private_dir`
  (0700 regardless of umask). Before every use (start, restore) the folder must be a real directory (not a
  symlink, checked with `symlink_metadata`), owned by the user, and not group/world accessible; an existing
  folder with wider bits is set to 0700 with `fchmod` on an `O_DIRECTORY|O_NOFOLLOW` fd, else refused.
  vr-ide never reads, lists, copies or logs anything inside an account folder (not even file names), and
  the folder is excluded from AI-change recording (already under `state_dir()`, SEC-10), Tab completion
  of paths, the session file and recovery. Verify: umask 022 → 0700; pre-made 0755 folder → 0700 after
  start; `accounts/work` a symlink to a temp dir → refused, the target dir gets nothing; inotify/strace-free
  check: grep T8 code for `read_dir`/`File::open` under the account path → only the `accounts/` listing.
- **SEC-17 (remove account)** — asks first (names the account; suggests `/logout` in a tab of it first,
  since deleting the folder does not revoke the token); refused while a running agent tab uses that
  account. Deletion re-validates the name (SEC-15), checks `accounts/<name>` with `symlink_metadata` (a
  symlink is removed as a link only, its target untouched), then deletes it permanently with
  `std::fs::remove_dir_all` (does not follow symlinks inside) — not to the trash, so tokens do not stay
  in `~/.local/share/Trash`. Nothing outside `accounts/<name>` is touched; `accounts/` itself stays.
  Verify: remove `work` with a symlink inside to a temp "outside" dir → outside intact, `work` gone,
  sibling `alt` intact; remove while a shim agent runs on it → refused; name `..` via a crafted session
  entry → refused.
- **SEC-18 (env scope and restore)** — the variable is added only to the env of that agent's spawn
  (`SpawnOptions.env` / `Command::env`), never with `std::env::set_var` or in the shared terminal env, so
  shells, ssh, `/run`, other agents and later spawns are unaffected. The tool → variable mapping is fixed
  in code (`claude` → `CLAUDE_CONFIG_DIR`, `codex` → `CODEX_HOME`); a tool without a mapping cannot take an
  account (picker hidden). For an account session vr-ide sets the variable (overriding an inherited one);
  for Default it adds nothing and leaves the inherited environment as is. `session.toml` stores the
  account name per agent tab; on restore the name passes SEC-15 and the folder SEC-16, otherwise the tab is
  **not** reopened (one notice naming the account) — never a fallback to Default or another account; the
  continue arguments run with that tab's own variable. Verify: shim agent on account `work` dumps env →
  `CLAUDE_CONFIG_DIR=<state>/accounts/work`; a shell tab, an ssh shim and a Default agent opened after
  it → no `CLAUDE_CONFIG_DIR` (or the inherited value unchanged); restore with folder `work` deleted → no
  tab, notice, no Default spawn; two tabs `work`/`alt` restored → each shim sees its own folder.

## Accepted risks
- A typed (or knowingly re-run) `/ssh` line can run local programs through ssh options — same as a
  terminal (SEC-5 only makes replay visible). `-f`/`-M` with `ControlPersist` can leave ssh running after
  the tab closes; that is the user's explicit option.
- Same-uid programs can edit `ssh-recent`, `session.toml` and the store; checks above bound damage, they do
  not prevent it.
- Pre-images of secret files exist at 0600 under the state dir until Keep/Revert/close.
- Removing an account deletes its local token files but does not revoke them at the provider (SEC-17 suggests `/logout` first); same-uid programs can read account folders.
- The tiny window between the final canonical-parent check and `rename` (SEC-12) remains without
  `openat2(RESOLVE_BENEATH)`; T6 may use `openat` on the parent fd to close it.
- `/proc/<pid>/cmdline` shows the ssh argument line and nonce to the same uid (as FR-25).
