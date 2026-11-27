# Review M5-1 — vr-term crate (code + security of process handling)

Verdict: **CHANGES_REQUESTED** (2 major, 3 minor)

## Findings

**1. [major / security medium] crates/vr-term/src/terminal.rs:75-78 — CWE-770: emulator replies skip the input cap, so the queue can grow without limit**
Data path: the program's output (any file it prints) → `Term` handles DA `ESC[c` and DSR `ESC[6n` → `Event::PtyWrite` → `Listener` runs `pending.fetch_add` and then `input.send` with no check against `MAX_PENDING_INPUT`.
Exploit: `cat big.txt`, where the file repeats `\e[c`. `cat` never reads stdin, so the slave's input buffer fills and `write_loop` blocks in `write_all`. Each 3-byte query then adds a `Vec` and a channel slot (about 80 B of heap). A 100 MB file holds about 33 M queries, which is several GB of memory and can OOM the IDE. This breaks D198 ("memory stays bounded") and AC-14c ("memory stays bounded by the scrollback setting"). It also means user typing is refused with `InputBacklog` for good.
Fix: apply the same cap in `Listener::send_event`. When `pending + len > MAX_PENDING_INPUT`, drop the reply, because a program that does not read its input cannot use the answer anyway. Add a test that prints many `\e[c` from a program that never reads (`printf` in a loop, then `sleep`) and checks that `pending` stays at or below the cap. Exposing pending through a test hook is fine, or the test can assert `write()` still gets `InputBacklog` and not unbounded RSS.

**2. [major] crates/vr-term/src/terminal.rs:175-178, 388-404, 417-427 — CWE-772: Drop leaks the reader thread, a master fd and the whole `Term` (up to ~30 MB at 10k×120) for as long as a stray process keeps the slave open. The pty is never hung up.**
Scenario: in the terminal, the user runs `firefox &`, `some-server & disown`, or a job started before `exit`. Then the tab closes. `kill()` signals only the shell's process group. A job-control background job lives in its own pgid, and after the shell has exited `kill()` does nothing at all. `master.take()` does not close the master because the reader holds a dup from `try_clone_reader`. So the slave is never hung up: the stray process keeps writing into the detached `Term`, and the reader thread, its fd and its scrollback stay alive until that process exits, which can be days. In xterm the master closes, so the job gets EIO or SIGHUP and the memory is freed. The notes call this "held until that process exits" without saying the reader's own dup is what keeps the pty alive. Grade: major. A normal workflow reaches it, and each closed tab leaks one thread, one fd and up to the scrollback size of memory.
Fix (pick one):
- (a) Make the reader wait in `rustix::event::poll` on the master fd plus a wake pipe or eventfd. `Drop` writes to the wake fd, the reader returns and drops its dup, the master is fully closed and the kernel hangs up the slave.
- (b) At minimum, have `Drop` also send SIGHUP and then SIGKILL to the pty's foreground group (`MasterPty::process_group_leader()` / `tcgetpgrp`), and have the reader stop feeding the emulator once `closed` is set. That bounds CPU, but not the fd.

Also add a test where `sh -c 'setsid sleep 30 & echo x'` exits and the terminal is dropped: assert that the `vr-term-reader` thread count, or the fd count, returns to its old value. This path is untested today.

**3. [minor] crates/vr-term/tests/flood.rs:66-67 — wall-clock asserts (`p99 < 5 ms`, `max < 20 ms`) on a host the notes call loaded (load 9–11)**
Scenario: CI or a parallel QC run deschedules the test thread for more than 20 ms, which already happened at 14.4 ms in the developer's own run, and the test fails without any defect. It also takes 22 s and runs by default inside `cargo test -p vr-term`.
Fix: assert only on p99, or loosen `max` (for example 100 ms), state the runner assumption (`BASE-42`), and consider `#[ignore]` plus an explicit run in the qc/bench step.

**4. [minor] crates/vr-term/src/terminal.rs:154-161 — the outer terminal's variables leak into the child**
`CommandBuilder::new` copies the IDE's full environment (no secrets beyond the parent's own, which is acceptable). Started from inside tmux, `TMUX`/`TMUX_PANE` make `tmux` in the IDE terminal refuse to run as nested. `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `VTE_VERSION`, `COLUMNS` and `LINES` also describe the wrong terminal.
Fix: `env_remove` those keys before applying `opts.env`.

**5. [minor] .ai-dev-team/workspace/04_dev_M5-1.md:92-119 — the rule table is incomplete**
`COST | pass` is one row for the whole pack, not one per rule. BASE IDs such as BASE-7, BASE-9, BASE-12 and BASE-42 appear neither as rows nor on an `n-a:` line ("n-a: none"). Fix: one row or `n-a` entry per ID.

## Verified OK
- **Process start:** setsid, controlling tty, signal dispositions and mask reset, fds above 2 closed (portable-pty pre_exec); SIGPIPE reset by std.
- **Kill:** the pid cannot be recycled — `signal_group` signals only while the status lock reads `Running`, and the reap happens under that lock. Escalation is HUP → 300 ms → KILL → 300 ms. Reaping is `waitid(NOWAIT)` and then `child.wait()`, so no zombies.
- **Drop timing:** bounded at about 0.8 s, then the helper threads are detached.
- **Locks:** lock order is size → term and term → title, with no inversion. Snapshot copies the cells under the lock and converts them after releasing it.
- **Notify coalescing:** the last update cannot be lost. `dirty` is cleared under the term lock before the capture, and `mark_dirty` runs after each parse.
- **Write backlog:** `write` cannot race itself, because `Terminal` is `!Sync`.
- **Key encoding:** app-cursor SS3, modifier parameter 1+s+2a+4c, F1–4 as SS3 or `CSI 1;m P..S`, F5–12 codes 15/17/18/19/20/21/23/24, ctrl+space/@/2 → NUL, alt → ESC prefix, BackTab `CSI Z`.
- **Mouse:** SGR encoding, with drag and move gated on 1002/1003.
- **Paste:** in bracketed mode every ESC is stripped, so an embedded `ESC[201~` cannot close the bracket early (test at input/tests.rs:172).
- **Snapshot:** wide and leading spacers become `""`, zero-width marks are appended to the cell, viewport line math is correct with an offset, and the cursor is hidden when scrolled out.
- **Errors and resize:** BadSize/BadCwd/Spawn errors, failure-after-spawn cleanup, and resize ordering are handled.
- **Dependencies:** `=0.9.0` and `=0.26.0` pinned exact; alacritty_terminal has default features off. Every new crate is MIT, Apache-2.0, or offers one of them (serial2 BSD-2-Clause OR Apache-2.0; cursor-icon adds Zlib). The `[profile.dev.package]` opt-level overrides are acceptable (D195). No dependency audit was run: `cargo-audit` and `cargo-deny` are not installed, so dependencies were checked statically only.

## Files read
- Cargo.toml — read
- Cargo.lock — skipped (lockfile; new package names and licences checked through `cargo metadata`)
- crates/vr-term/Cargo.toml, src/lib.rs, src/types.rs, src/input.rs, src/input/tests.rs (test names and paste cases), src/snapshot.rs, src/terminal.rs — read
- crates/vr-term/tests/common/mod.rs, tests/pty.rs (lifecycle tests 195-321, names of the rest), tests/flood.rs — read
- portable-pty 0.9.0 unix.rs spawn and cmdbuilder base env; vte 0.15 OSC buffer; alacritty_terminal PtyWrite sites — callees followed

## Rule check M5-1
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: Cargo.toml, Cargo.lock, crates/vr-term/** only |
| BASE-2 | pass | API matches plan Contracts; the extras are additive (notes, API summary) |
| BASE-3 | pass | red runs logged per group against stubs. The two never-red tests are acceptable: the mouse test checks a negative (empty output), and the backlog test would fail without the cap because the channel is unbounded and no chunk would be refused. Missing tests for findings 1 and 2. |
| BASE-8 | pass | exact pins, licences verified, default-features off on alacritty_terminal |
| BASE-25 | pass | fmt --check and clippy -p vr-term -D warnings clean (notes); not re-run |
| BASE-42 | fail (minor) | finding 3: wall-clock asserts with no runner assumption stated |
| New states | fail | finding 2: the "Call never returns" cell says the fd and thread are held "until that process exits", which misses that the reader's dup keeps the pty alive with no bound; no test covers it. Finding 1: the reply path is not in the input-queue row. |
| AC-14c (backend) | fail | finding 1: memory is not bounded under reply-generating output |
| AC-14b, AC-14d (backend) | pass | tests `shell_sees_term_colorterm_cwd_and_extra_env`, `exit_code_is_reported`, `kill_hangs_up_the_whole_group_and_reaps`, `drop_kills_reaps_and_returns_promptly`, `a_child_ignoring_sighup_is_killed_after_the_grace_period` |

## Round 1 re-check
Scope: `git diff` against wip 97d6f24, i.e. crates/vr-term/Cargo.toml, src/terminal.rs, tests/pty.rs and tests/flood.rs, plus the new tests/env.rs and tests/leak.rs, all read in full. Notes: `## Round 1` of 04_dev_M5-1.md.

Verdict: **APPROVED** (one new minor, below).

### Earlier findings
| # | Status | Evidence |
|---|---|---|
| 1 major (reply queue unbounded) | fixed | `reserve()` makes one atomic check with `try_update` (stable on 1.100). The Listener arm `PtyWrite(text) if reserve(..).is_ok()` drops replies over the cap, and `write()` shares the same check, so a key and a reply can no longer both pass a stale check. Test `replies_to_queries_from_a_program_that_never_reads_stay_under_the_input_cap`: red with 3.7 MB queued, now green. Raw mode is the right setup, because canonical mode discards input that overflows. |
| 2 major (reader leak, pty kept alive) | fixed | `pty_io()` gives the reader and writer their own CLOEXEC, O_NONBLOCK copies of the master. `ready()` polls the fd plus the wake eventfd, and Drop writes the eventfd before closing the master. `leak.rs` covers the TIOCNOTTY daemon case: red `(1, 1)` threads/fds left, now `(0, 0)`, with drop under 1 s. |
| 3 minor (flood test timing) | fixed | The default test keeps the memory bound and p99 < 50 ms; the strict limits are in an `#[ignore]` release test; the runner assumption is in the file header. |
| 4 minor (outer-terminal env) | fixed | `OUTER_TERMINAL_ENV` is removed before `opts.env` is applied, so explicit values still win; covered by `env.rs`. |
| 5 minor (rule table) | fixed | One row per rule ID; the rest are on `n-a:` lines. |

### Non-blocking I/O and poll loops
- **No lost wakeup.** The eventfd is written once and never read, so it stays readable (level-triggered). Both threads see it on their next poll, whichever order things happen in.
- **No busy-wait.** `poll` has no timeout. When the slave goes away the master reports POLLHUP or POLLERR, `ready()` returns true, and the next read or write gets EIO, so the loop ends. The `Ok(_) => continue` branch only runs if poll returns with no events, which an infinite timeout never does.
- **EAGAIN, EINTR and partial writes are handled.** Read: EAGAIN → poll, EINTR → retry. Write: it resumes from what is left after a partial write, EAGAIN → poll, and any other error ends the writer with `pending` decremented for that message. After that, `write()` reports `Closed` because the receiver is gone.
- **Making the master non-blocking breaks nothing.** O_NONBLOCK is set on the shared open file description, but portable-pty only uses that description for ioctls (resize and size).
- **Errors in `pty_io` clean up.** A failure there takes the existing path that kills and reaps the child.

### Option (b) skipped: acceptable
Once every master fd is closed, the kernel hangs up the slave itself. Any process still holding the slave gets EIO, and the session or foreground group gets SIGHUP — the same as when xterm closes. A foreground job in another process group that ignores SIGHUP survives here just as it does in xterm, and it no longer holds any vr-ide resource. Signalling `tcgetpgrp` after the session leader is gone could hit a process group that is no longer ours. The rationale holds.

### New finding
**[minor] crates/vr-term/src/terminal.rs:read_loop — the reader checks the wake fd only after an EAGAIN**
Scenario: after the tab closes, a stray process that still holds the slave writes faster than the reader parses. Every read then returns data, the reader never reaches `poll`, and it keeps parsing into the detached `Term` (CPU and a master fd held) until that process stops writing. Reachability is uncertain. The kernel pty path tops out around 7 MB/s on this host, which is below the release-mode parse rate, so EAGAIN usually comes soon. In the debug test profile (about 4.4 MB/s) it may not.
Fix: return at the top of each loop iteration when `shared.closed` is set (Drop sets it before firing the wake).

### Regressions
None found. The public API is unchanged apart from the additive `pending_input()`. Lock order, snapshot, Notify coalescing and the kill/reap path are untouched. The writer no longer sends portable-pty's EOT when dropped, which does not matter because the child is hung up anyway.

### Rule check M5-1 (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-term/** only this round |
| BASE-2 | pass | additive `pending_input()` only |
| BASE-3 | pass | red recorded for env, the reply cap and the leak (TIOCNOTTY case) |
| BASE-8 | pass | no new crate; rustix gains features `event` and `fs` |
| BASE-25 | pass | fmt and clippy `-p vr-term` clean (notes) |
| BASE-42 | pass | flood.rs header states the runner assumption; strict limits are ignored by default |
| New states | pass | `## New states (round 1)` covers the wake fd, the reader, the writer and the reply cap |
| AC-14b/c/d (backend) | pass | tests listed in the Round 1 rule table |

## Quick pass 1 re-check
APPROVED: the optional minor is closed. `read_loop` returns when `shared.closed` is set, checked before every read; Drop sets that flag before firing the wake. terminal.rs differs from round 1 only by that 5-line check. leak.rs adds a `yes flood` TIOCNOTTY daemon case as a regression test, which kills the grandchild by its pid; it could not be made red first because the kernel pty path is slower than the parser, an acceptable BASE-3 reason. No other change.

## Round 2 oracle check
APPROVED (test-only change): the new assertion `pending_input() <= MAX_PENDING_INPUT` after the refusal still catches all three defect classes. (1) Unbounded queue growth: with no cap or a broken one, all 64 chunks (4 MiB) are accepted and `expect("backlog refused")` fails; a cap set above 1 MiB leaves pending above MAX and fails the new assertion. (2) A blocking write: the `< 1 s` check is unchanged. (3) No refusal: the `expect` and the `InputBacklog` match are unchanged. The kernel tty buffer is finite, so the refusal still comes well within 64 chunks. The old `i * 64KiB` bound measured accepted bytes rather than the queue, which is why it was flaky.
